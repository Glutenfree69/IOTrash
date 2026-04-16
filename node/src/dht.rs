//! DHT11 temperature/humidity sensor reader using the ESP32 RMT peripheral.
//!
//! The RMT (Remote Control Transceiver) is a hardware peripheral that captures
//! GPIO transitions with microsecond precision, independent of the CPU and
//! FreeRTOS scheduler. This makes it the right tool for the timing-sensitive
//! DHT11 protocol — bit-banging the GPIO from Rust would be unreliable as soon
//! as another task preempts us mid-pulse.
//!
//! Protocol summary (DHT11 / DHT22, OneWire-style on a single GPIO):
//!   1. Host pulls line LOW for >= 18 ms, then HIGH for ~30 us (start signal).
//!   2. Sensor responds with 80 us LOW + 80 us HIGH (response signal).
//!   3. Sensor sends 40 bits. Each bit is encoded as:
//!        - 50 us LOW (bit-start)
//!        - HIGH for ~26-28 us (bit = 0) OR ~70 us (bit = 1)
//!   4. Line returns to idle HIGH.
//!
//! The 40 bits split into 5 bytes:
//!   [humidity_int, humidity_dec, temperature_int, temperature_dec, checksum]
//! checksum = (b0 + b1 + b2 + b3) & 0xFF
//!
//! DHT11 always reports 0 in the decimal bytes; DHT22 actually fills them.

use core::time::Duration;

use anyhow::{anyhow, bail, Result};

use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::InputPin;
use esp_idf_svc::hal::rmt::config::{ReceiveConfig, RxChannelConfig};
use esp_idf_svc::hal::rmt::{PinState, RxChannelDriver, Symbol};
use esp_idf_svc::hal::units::FromValueType;
use esp_idf_svc::sys::{
    esp_rom_delay_us, gpio_mode_t_GPIO_MODE_INPUT, gpio_mode_t_GPIO_MODE_OUTPUT_OD,
    gpio_pull_mode_t_GPIO_PULLUP_ONLY, gpio_set_direction, gpio_set_level, gpio_set_pull_mode,
};

/// 1 tick = 1 µs. Matches the DHT11 timing scale exactly.
const RMT_RESOLUTION_HZ: u32 = 1_000_000;

/// Threshold (in µs) used to decide a HIGH-pulse-encoded bit:
/// shorter than this is a 0, longer is a 1.
const BIT_THRESHOLD_US: u16 = 50;

/// Threshold (in µs) used to recognise the response symbol: both halves of
/// the response pulse are ~80 µs, much longer than any bit-start LOW (50 µs).
const RESPONSE_THRESHOLD_US: u16 = 60;

/// Buffer size for captured RMT symbols. We expect ~41 (1 response + 40 bits),
/// keep some margin for noise / partial frames.
const SYMBOL_BUFFER_LEN: usize = 64;

pub trait SensorReader {
    fn read(&mut self) -> Result<DhtReading>;
}

#[derive(Debug, Clone, Copy)]
pub struct DhtReading {
    pub temperature_c: f32,
    pub humidity_pct: f32,
}

pub struct Dht11Reader<'d> {
    rx: RxChannelDriver<'d>,
    pin_num: i32,
}

impl<'d> Dht11Reader<'d> {
    /// Configures GPIO `pin` as a DHT11 data line backed by an RMT RX channel.
    pub fn new<P: InputPin + 'd>(pin: P) -> Result<Self> {
        let pin_num = pin.pin() as i32;

        let config = RxChannelConfig {
            resolution: RMT_RESOLUTION_HZ.Hz().into(),
            ..Default::default()
        };

        let rx = RxChannelDriver::new(pin, &config)
            .map_err(|e| anyhow!("failed to allocate RMT RX channel: {e:?}"))?;

        Ok(Self { rx, pin_num })
    }

    /// Drives the start signal on the data line, then bypasses the RMT briefly.
    ///
    /// We touch the GPIO via raw `esp-idf-sys` calls because the safe Rust pin
    /// has been moved into the RMT driver. Switching direction this way is the
    /// pattern used by the official ESP-IDF DHT examples in C.
    fn send_start_signal(&self) {
        unsafe {
            gpio_set_direction(self.pin_num, gpio_mode_t_GPIO_MODE_OUTPUT_OD);
            gpio_set_pull_mode(self.pin_num, gpio_pull_mode_t_GPIO_PULLUP_ONLY);
            gpio_set_level(self.pin_num, 0);
        }
        // 20 ms LOW: datasheet asks for >= 18 ms; 20 gives margin and yields to FreeRTOS.
        FreeRtos::delay_ms(20);
        unsafe {
            gpio_set_level(self.pin_num, 1);
            // 30 µs HIGH: short, busy-wait is fine.
            esp_rom_delay_us(30);
            // Hand the line back to the input/RMT path before the sensor responds.
            gpio_set_direction(self.pin_num, gpio_mode_t_GPIO_MODE_INPUT);
        }
    }

    /// Locates the response symbol within the captured stream.
    ///
    /// The response is the only symbol where BOTH halves are around 80 µs
    /// (sensor pulls LOW for 80 µs, then releases HIGH for 80 µs). Bit
    /// symbols, by contrast, have a 50 µs LOW first half and a 26-70 µs HIGH
    /// second half. Searching for it dynamically guards us against captures
    /// that started mid-frame and shifted everything.
    fn find_response_index(symbols: &[Symbol]) -> Option<usize> {
        symbols.iter().position(|sym| {
            sym.level0().ticks.ticks() > RESPONSE_THRESHOLD_US
                && sym.level1().ticks.ticks() > RESPONSE_THRESHOLD_US
        })
    }

    /// Logs every captured symbol — used to diagnose decoding failures.
    fn dump_symbols(symbols: &[Symbol]) {
        for (i, sym) in symbols.iter().enumerate() {
            let l0 = sym.level0();
            let l1 = sym.level1();
            log::warn!(
                "  sym[{:02}] L0={:?}/{:>4}us  L1={:?}/{:>4}us",
                i,
                l0.pin_state,
                l0.ticks.ticks(),
                l1.pin_state,
                l1.ticks.ticks(),
            );
        }
    }

    fn decode(symbols: &[Symbol]) -> Result<DhtReading> {
        if symbols.is_empty() {
            bail!("no RMT symbols received");
        }

        let response_idx = Self::find_response_index(symbols).ok_or_else(|| {
            anyhow!(
                "no response symbol in {} captured symbols",
                symbols.len()
            )
        })?;

        let bit_start = response_idx + 1;
        if symbols.len() < bit_start + 40 {
            bail!(
                "not enough bit symbols after response (idx={}): have {}, need 40",
                response_idx,
                symbols.len() - bit_start,
            );
        }

        let mut bytes = [0u8; 5];
        for (i, sym) in symbols[bit_start..bit_start + 40].iter().enumerate() {
            // The HIGH half of each bit symbol encodes the value.
            let high_us = sym.level1().ticks.ticks();
            if high_us > BIT_THRESHOLD_US {
                bytes[i / 8] |= 1 << (7 - (i % 8));
            }
        }

        let expected = bytes[4];
        let actual = bytes[0]
            .wrapping_add(bytes[1])
            .wrapping_add(bytes[2])
            .wrapping_add(bytes[3]);
        if expected != actual {
            bail!(
                "checksum mismatch: expected 0x{:02X}, got 0x{:02X} (bytes: {:02X?}, response_idx={})",
                expected,
                actual,
                bytes,
                response_idx
            );
        }

        Ok(DhtReading {
            humidity_pct: bytes[0] as f32 + (bytes[1] as f32 / 10.0),
            temperature_c: bytes[2] as f32 + (bytes[3] as f32 / 10.0),
        })
    }
}

impl<'d> SensorReader for Dht11Reader<'d> {
    fn read(&mut self) -> Result<DhtReading> {
        self.send_start_signal();

        let mut symbols = [Symbol::default(); SYMBOL_BUFFER_LEN];
        let receive_config = ReceiveConfig {
            // Pulses shorter than 1 µs are noise/glitches.
            // (RMT hardware caps this filter at ~3.2 µs on ESP32.)
            signal_range_min: Duration::from_nanos(1_000),
            // Idle line longer than 200 µs ends the reception (longest valid
            // half is the 80 µs response, plus margin).
            signal_range_max: Duration::from_micros(200),
            // 1 second is plenty: a full DHT11 frame is ~5 ms.
            timeout: Some(1_000),
            ..Default::default()
        };

        let count = self
            .rx
            .receive(&mut symbols, &receive_config)
            .map_err(|e| anyhow!("RMT receive failed: {e:?}"))?;

        log::info!("RMT capture: {} symbols", count);

        let result = Self::decode(&symbols[..count]);
        if result.is_err() {
            log::warn!("decode failed, dumping {} captured symbols:", count);
            Self::dump_symbols(&symbols[..count]);
        }
        result
    }
}
