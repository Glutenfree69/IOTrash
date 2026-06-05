mod dht;
mod dust;

use std::time::Duration;

use dht::{Dht11Reader, SensorReader};
use dust::Gp2y1010Reader;

const READ_INTERVAL: Duration = Duration::from_secs(5);

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Sensor Sensei node starting...");

    let peripherals = esp_idf_svc::hal::peripherals::Peripherals::take()?;

    let mut dht = Dht11Reader::new(peripherals.pins.gpio13)?;
    log::info!("DHT11 ready on GPIO 13.");

    let mut dust = Gp2y1010Reader::new(
        peripherals.adc1,
        peripherals.pins.gpio35,
        peripherals.pins.gpio25,
    )?;
    log::info!("GP2Y1010 ready (AOUT=GPIO35, ILED=GPIO25).");

    log::info!("Reading every {:?}.", READ_INTERVAL);

    loop {
        match dht.read() {
            Ok(r) => log::info!(
                "DHT11: {:.1}°C  {:.0}%RH",
                r.temperature_c,
                r.humidity_pct
            ),
            Err(e) => log::warn!("DHT11 read failed: {e}"),
        }

        match dust.read() {
            Ok(r) => log::info!(
                "Dust:  {:.1} µg/m³  (mv_avg={} mV)",
                r.density_ugm3,
                r.mv_avg,
            ),
            Err(e) => log::warn!("Dust read failed: {e}"),
        }

        std::thread::sleep(READ_INTERVAL);
    }
}
