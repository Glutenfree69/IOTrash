mod api;
#[cfg(feature = "lora")]
mod lora;
// `mock` is in the default features, so `--features lora` alone would enable
// both; `lora` takes priority so no `--no-default-features` is needed.
#[cfg(all(feature = "mock", not(feature = "lora")))]
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
    let wifi = wifi::connect(peripherals.modem, sysloop, Some(nvs))?;

    // Derive chip ID from MAC address
    let mac = wifi
        .wifi()
        .get_mac(esp_idf_svc::wifi::WifiDeviceId::Sta)?;
    let chip_id = format!("esp32-{:02x}{:02x}{:02x}", mac[3], mac[4], mac[5]);
    log::info!("Chip ID: {}", chip_id);

    let api = api::SensorApi::new(chip_id);

    // Create packet source (mock or lora depending on feature flag)
    #[cfg(all(feature = "mock", not(feature = "lora")))]
    let mut source = mock::MockSource::new(1);

    // Real LoRa receiver on the TTGO LoRa32's internal SX1276 (SPI2).
    // Pinout (TTGO LoRa32 v2.1): SCK=5, MISO=19, MOSI=27, CS=18, RST=23, DIO0=26.
    #[cfg(feature = "lora")]
    let mut source = {
        use esp_idf_svc::hal::gpio::{PinDriver, Pull};
        use esp_idf_svc::hal::spi::{config::Config as SpiConfig, SpiDeviceDriver, SpiDriver, SpiDriverConfig};
        use esp_idf_svc::hal::units::FromValueType;

        let pins = peripherals.pins;
        let driver = SpiDriver::new(
            peripherals.spi2,
            pins.gpio5,            // SCLK
            pins.gpio27,           // MOSI (SDO)
            Some(pins.gpio19),     // MISO (SDI)
            &SpiDriverConfig::new(),
        )?;
        let spi = SpiDeviceDriver::new(
            driver,
            Some(pins.gpio18),     // CS / NSS
            &SpiConfig::new().baudrate(8.MHz().into()),
        )?;
        let reset = PinDriver::output(pins.gpio23)?;
        let dio0 = PinDriver::input(pins.gpio26, Pull::Floating)?;
        lora::new_source(spi, reset, dio0)?
    };

    // Main loop: receive packet -> forward to API
    loop {
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
