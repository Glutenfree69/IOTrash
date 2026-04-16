mod dht;

use std::time::Duration;

use dht::{Dht11Reader, SensorReader};

const READ_INTERVAL: Duration = Duration::from_secs(5);

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Sensor Sensei node starting...");

    let peripherals = esp_idf_svc::hal::peripherals::Peripherals::take()?;
    let mut reader = Dht11Reader::new(peripherals.pins.gpio13)?;

    log::info!("DHT11 reader ready on GPIO 13. Reading every {:?}.", READ_INTERVAL);

    loop {
        match reader.read() {
            Ok(r) => log::info!(
                "DHT11: {:.1}°C  {:.0}%RH",
                r.temperature_c,
                r.humidity_pct
            ),
            Err(e) => log::warn!("DHT11 read failed: {e}"),
        }
        std::thread::sleep(READ_INTERVAL);
    }
}
