mod dht;
mod lora;
mod packet;

use std::time::Duration;

use dht::{Dht11Reader, SensorReader};
use lora::Lora;

const READ_INTERVAL: Duration = Duration::from_secs(5);

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Sensor Sensei node starting...");

    let mut reader = Dht11Reader::new(peripherals.pins.gpio13)?;
    let lora_driver = Lora::new();
    // TODO:
    // 1. implement connection to LORA Network
    // 2. send data in network

    log::info!(
        "DHT11 reader ready on GPIO 13. Reading every {:?}.",
        READ_INTERVAL
    );

    loop {
        match reader.read() {
            Ok(r) => {
                log::info!("DHT11: {:.1}°C  {:.0}%RH", r.temperature_c, r.humidity_pct);
                // TODO: create from factory function instead?
                let packet = packet::SensorPacket {
                    msg_type: 0x01, // TODO:
                    battery_mv: 0,  // TODO: unmock
                    humidity_raw: r.humidity_pct(),
                    node_id: 0,  // TODO: unmock
                    pm10_raw: 0, // TODO: unmock
                    pm25_raw: 0, // TODO: unmock
                    temp_raw: r.temperature_c(),
                };
            }
            Err(e) => log::warn!("DHT11 read failed: {e}"),
        }
        std::thread::sleep(READ_INTERVAL);
    }
}
