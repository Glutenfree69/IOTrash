mod miner;
mod wifi;

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("SensorSensei Miner starting...");

    let peripherals = esp_idf_svc::hal::peripherals::Peripherals::take()?;
    let sysloop = esp_idf_svc::eventloop::EspSystemEventLoop::take()?;
    let nvs = esp_idf_svc::nvs::EspDefaultNvsPartition::take()?;

    // Connect WiFi
    let wifi = wifi::connect(peripherals.modem, sysloop, Some(nvs))?;

    // Derive chip ID from MAC address
    let mac = wifi
        .wifi()
        .get_mac(esp_idf_svc::wifi::WifiDeviceId::Sta)?;
    let chip_id = format!("{:02x}{:02x}{:02x}", mac[3], mac[4], mac[5]);
    log::info!("Chip ID: {}", chip_id);

    // Start mining
    let duco_miner = miner::DucoMiner::new(&chip_id);
    duco_miner.run()
}
