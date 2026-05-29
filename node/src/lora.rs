use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::spi::*;

// https://github.com/esp-rs/esp-idf-hal/blob/master/examples/spi_loopback_async.rs
// https://github.com/lora-rs/lora-rs/blob/main/examples/esp32/esp32_RFM95/src/bin/async_main.rs
// https://fr.wikipedia.org/wiki/Serial_Peripheral_Interface

pub struct Lora {
    pub spi_device: SpiDeviceDriver,
}

impl Lora {
    pub fn new() -> Self {
        let peripherals = esp_idf_svc::hal::peripherals::Peripherals::take()?;

        let sclk = peripherals.pins.gpio5; // Serial Clock
        let serial_in = peripherals.pins.gpio19;
        let serial_out = peripherals.pins.gpio23;
        let spi = peripherals.spi2;

        let driver = SpiDriver::new::<SPI2>(
            spi,
            sclk,
            serial_out,
            Some(serial_in),
            &SpiDriverConfig::new(),
        )?;
        let spi_device = esp_idf_hal::shared_bus::asynch::spi::SpiDevice::new();

        let config = config::Config::new().baudrate(26.MHz().into());
        let spi_device = SpiDeviceDriver::new(&driver, Some(cs_1), &config)?;
        Self { spi_device }
    }
}
