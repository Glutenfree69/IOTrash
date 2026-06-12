mod dht;
mod dust;
mod lora;

use std::time::Duration;

use dht::{Dht11Reader, SensorReader};
use dust::Gp2y1010Reader;
use lora::PacketSink;
use protocol::{SensorPacket, MSG_TYPE_SENSOR_DATA};

const READ_INTERVAL: Duration = Duration::from_secs(5);
const NODE_ID: u8 = 1;

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

    // LoRa TX on the TTGO LoRa32's internal SX1276 (SPI2).
    // Pinout (TTGO LoRa32 v2.1): SCK=5, MISO=19, MOSI=27, CS=18, RST=23, DIO0=26.
    let mut radio = {
        use esp_idf_svc::hal::gpio::{PinDriver, Pull};
        use esp_idf_svc::hal::spi::{
            config::Config as SpiConfig, SpiDeviceDriver, SpiDriver, SpiDriverConfig,
        };
        use esp_idf_svc::hal::units::FromValueType;

        let driver = SpiDriver::new(
            peripherals.spi2,
            peripherals.pins.gpio5,        // SCLK
            peripherals.pins.gpio27,       // MOSI (SDO)
            Some(peripherals.pins.gpio19), // MISO (SDI)
            &SpiDriverConfig::new(),
        )?;
        let spi = SpiDeviceDriver::new(
            driver,
            Some(peripherals.pins.gpio18), // CS / NSS
            &SpiConfig::new().baudrate(8.MHz().into()),
        )?;
        let reset = PinDriver::output(peripherals.pins.gpio23)?;
        let dio0 = PinDriver::input(peripherals.pins.gpio26, Pull::Floating)?;
        lora::new_sink(spi, reset, dio0)?
    };

    log::info!("Reading + transmitting every {:?}.", READ_INTERVAL);

    loop {
        let climate = dht.read();
        match &climate {
            Ok(r) => log::info!("DHT11: {:.1}°C  {:.0}%RH", r.temperature_c, r.humidity_pct),
            Err(e) => log::warn!("DHT11 read failed: {e}"),
        }

        let dust_reading = dust.read();
        match &dust_reading {
            Ok(r) => log::info!("Dust:  {:.1} µg/m³  (mv_avg={} mV)", r.density_ugm3, r.mv_avg),
            Err(e) => log::warn!("Dust read failed: {e}"),
        }

        if let (Ok(climate), Ok(dust_reading)) = (&climate, &dust_reading) {
            // The GP2Y1010 reports one total dust density, not a PM2.5/PM10
            // split, so the same value goes in both fields until an SDS011
            // replaces it.
            let density_raw = (dust_reading.density_ugm3 * 10.0).clamp(0.0, u16::MAX as f32) as u16;
            let packet = SensorPacket {
                msg_type: MSG_TYPE_SENSOR_DATA,
                node_id: NODE_ID,
                pm25_raw: density_raw,
                pm10_raw: density_raw,
                temp_raw: (climate.temperature_c * 100.0) as i16,
                humidity_raw: (climate.humidity_pct * 100.0).clamp(0.0, u16::MAX as f32) as u16,
                battery_mv: 0, // battery sensing not wired yet
            };
            if let Err(e) = radio.send(&packet) {
                log::warn!("LoRa send failed: {e}");
            }
        } else {
            log::warn!("Skipping LoRa send: incomplete sensor data.");
        }

        std::thread::sleep(READ_INTERVAL);
    }
}
