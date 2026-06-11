use anyhow::Result;
use embedded_svc::wifi::{AuthMethod, ClientConfiguration, Configuration};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::WifiModemPeripheral;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};

// SECURITY NOTE:
// `env!()` bakes these values into the compiled binary as plaintext at
// build time (see gateway/.env.example). This is convenient for dev but
// means anyone who dumps the device's flash can recover the WiFi password.
// Mitigations:
//  - Use a dedicated/guest WiFi network for deployed sensors (not your
//    main personal network credentials).
//  - For production, enable ESP-IDF flash encryption (see
//    gateway/sdkconfig.defaults) and/or move to the AP-mode runtime
//    provisioning flow described in PLAN.md instead of compile-time creds.
const SSID: &str = env!("WIFI_SSID");
const PASSWORD: &str = env!("WIFI_PASS");

pub fn connect<'d>(
    modem: impl WifiModemPeripheral + 'd,
    sysloop: EspSystemEventLoop,
    nvs: Option<EspDefaultNvsPartition>,
) -> Result<BlockingWifi<EspWifi<'d>>> {
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(modem, sysloop.clone(), nvs)?,
        sysloop,
    )?;

    let config = Configuration::Client(ClientConfiguration {
        ssid: SSID
            .try_into()
            .map_err(|_| anyhow::anyhow!("WIFI_SSID exceeds 32 chars"))?,
        password: PASSWORD
            .try_into()
            .map_err(|_| anyhow::anyhow!("WIFI_PASS exceeds 64 chars"))?,
        auth_method: AuthMethod::WPA2Personal,
        ..Default::default()
    });

    wifi.set_configuration(&config)?;

    log::info!("WiFi starting...");
    wifi.start()?;

    log::info!("WiFi connecting...");
    wifi.connect()?;

    log::info!("WiFi waiting for netif up...");
    wifi.wait_netif_up()?;

    let ip_info = wifi.wifi().sta_netif().get_ip_info()?;
    log::info!("WiFi connected — IP: {}", ip_info.ip);
    // SECURITY: do NOT log `SSID`/`PASSWORD` here (or anywhere else) — serial
    // monitor output is often left enabled and can leak credentials to
    // anyone with physical/USB access to the device.

    Ok(wifi)
}

/// RELIABILITY: `connect()` only runs the WiFi state machine once at boot.
/// If the AP goes down later (reboot, interference, ...) the driver doesn't
/// automatically rejoin, so every subsequent `post_sensor_data` call would
/// fail forever. Call this at the top of each main-loop iteration: it's a
/// cheap no-op when already connected, and re-runs connect/DHCP otherwise.
pub fn ensure_connected(wifi: &mut BlockingWifi<EspWifi<'_>>) -> Result<()> {
    if wifi.is_connected()? {
        return Ok(());
    }

    log::warn!("WiFi disconnected — reconnecting...");
    wifi.connect()?;
    wifi.wait_netif_up()?;

    let ip_info = wifi.wifi().sta_netif().get_ip_info()?;
    log::info!("WiFi reconnected — IP: {}", ip_info.ip);

    Ok(())
}
