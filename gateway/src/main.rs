mod api;
#[cfg(feature = "mock")]
mod mock;
mod packet;
mod wifi;

use packet::PacketSource;

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Sensor Sensei gateway starting...");

    let peripherals = esp_idf_svc::hal::peripherals::Peripherals::take()?;
    let sysloop = esp_idf_svc::eventloop::EspSystemEventLoop::take()?;
    let nvs = esp_idf_svc::nvs::EspDefaultNvsPartition::take()?;

    // Connect WiFi
    let mut wifi = wifi::connect(peripherals.modem, sysloop, Some(nvs))?;

    // Derive chip ID from MAC address
    let mac = wifi
        .wifi()
        .get_mac(esp_idf_svc::wifi::WifiDeviceId::Sta)?;
    let chip_id = format!("esp32-{:02x}{:02x}{:02x}", mac[3], mac[4], mac[5]);
    log::info!("Chip ID: {}", chip_id);

    let api = api::SensorApi::new(chip_id);

    // Create packet source (mock or lora depending on feature flag)
    #[cfg(feature = "mock")]
    let mut source = mock::MockSource::new(1);

    // Main loop: receive packet -> forward to API
    loop {
        // RELIABILITY: rejoin the AP if it dropped since the last iteration,
        // so a transient WiFi outage doesn't permanently stop data uploads.
        if let Err(e) = wifi::ensure_connected(&mut wifi) {
            log::error!("Failed to reconnect WiFi: {e}");
            std::thread::sleep(std::time::Duration::from_secs(10));
            continue;
        }

        match source.receive() {
            Ok(packet) => {
                log::info!(
                    "Packet from node {}: PM2.5={:.1} PM10={:.1} T={:.1}C H={:.1}%",
                    packet.node_id,
                    packet.pm25(),
                    packet.pm10(),
                    packet.temperature(),
                    packet.humidity(),
                );
                if let Err(e) = api.post_sensor_data(&packet) {
                    log::error!("Failed to post data: {e}");
                }
            }
            Err(e) => {
                log::error!("Failed to receive packet: {e}");
                std::thread::sleep(std::time::Duration::from_secs(10));
            }
        }
    }
}
