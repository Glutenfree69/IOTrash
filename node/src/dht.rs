//! DHT11 temperature/humidity sensor reader using the ESP32 RMT peripheral.
//!
//! The RMT (Remote Control Transceiver) is a hardware peripheral that captures
//! GPIO transitions with microsecond precision, independent of the CPU and
//! FreeRTOS scheduler. This makes it the right tool for the timing-sensitive
//! DHT11 protocol — bit-banging the GPIO from Rust would be unreliable as soon
//! as another task preempts us mid-pulse.
//!
//! Protocol summary (DHT11, OneWire-style on a single GPIO):
//!   1. Host pulls line LOW for >= 18 ms, then HIGH for ~30 µs (start signal).
//!   2. Sensor responds with 80 µs LOW + 80 µs HIGH (response signal).
//!   3. Sensor sends 40 bits. Each bit is encoded as:
//!        - 50 µs LOW (bit-start)
//!        - HIGH for ~26 µs (bit = 0) OR ~70 µs (bit = 1)
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
use esp_idf_svc::hal::rmt::{RmtChannel, RxChannelDriver, Symbol};
use esp_idf_svc::hal::units::FromValueType;
use esp_idf_svc::sys::{
    esp_rom_delay_us, gpio_mode_t_GPIO_MODE_INPUT, gpio_mode_t_GPIO_MODE_OUTPUT_OD,
    gpio_pull_mode_t_GPIO_PULLUP_ONLY, gpio_set_direction, gpio_set_level, gpio_set_pull_mode,
    ESP_ERR_TIMEOUT,
};

/// 1 tick = 1 µs. Matches the DHT11 timing scale exactly.
const RMT_RESOLUTION_HZ: u32 = 1_000_000;

/// Threshold (µs) used to decide a HIGH-pulse-encoded bit:
/// shorter is 0, longer is 1.
const BIT_THRESHOLD_US: u16 = 50;

/// Threshold (µs) used to recognise the response symbol: both halves of the
/// response pulse are ~80 µs, much longer than any bit-start LOW (50 µs).
const RESPONSE_THRESHOLD_US: u16 = 60;

/// 1 start-signal symbol + 1 response symbol + 40 bit symbols, with margin.
const SYMBOL_BUFFER_LEN: usize = 64;

/// FreeRTOS ticks to wait for a complete frame. At the default 100 Hz tick
/// rate that's 500 ms — plenty, the actual frame is ~5 ms.
const FRAME_WAIT_TICKS: u32 = 50;

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

        let mut rx = RxChannelDriver::new(pin, &config)
            .map_err(|e| anyhow!("failed to allocate RMT RX channel: {e:?}"))?;

        // Enable the channel up-front so the first read() doesn't pay the
        // enable cost inside the tight ~40 µs window between our start signal
        // and the sensor response.
        rx.enable()
            .map_err(|e| anyhow!("failed to enable RMT channel: {e:?}"))?;

        Ok(Self { rx, pin_num })
    }

    /// Drives the start signal on the data line, then hands it back to RMT.
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

    /// The response is the only symbol where BOTH halves are ~80 µs. Bit
    /// symbols have a 50 µs LOW half and a 26-70 µs HIGH half.
    fn find_response_index(symbols: &[Symbol]) -> Option<usize> {
        symbols.iter().position(|sym| {
            sym.level0().ticks.ticks() > RESPONSE_THRESHOLD_US
                && sym.level1().ticks.ticks() > RESPONSE_THRESHOLD_US
        })
    }

    fn decode(symbols: &[Symbol]) -> Result<DhtReading> {
        if symbols.is_empty() {
            bail!("no RMT symbols received");
        }

        let response_idx = Self::find_response_index(symbols).ok_or_else(|| {
            // Dump the first symbols to tell a silent sensor (only our own
            // ~20 ms start signal captured) from a wiring/pin problem.
            let dump = symbols
                .iter()
                .take(4)
                .map(|s| {
                    format!(
                        "({:?}:{}us {:?}:{}us)",
                        s.level0().pin_state,
                        s.level0().ticks.ticks(),
                        s.level1().pin_state,
                        s.level1().ticks.ticks(),
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            anyhow!(
                "no response symbol in {} captured symbols: {dump}",
                symbols.len()
            )
        })?;

        let bit_start = response_idx + 1;
        if symbols.len() < bit_start + 40 {
            bail!(
                "not enough bit symbols after response: have {}, need 40",
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
                "checksum mismatch: expected 0x{:02X}, got 0x{:02X}",
                expected,
                actual,
            );
        }

        Ok(DhtReading {
            humidity_pct: bytes[0] as f32 + (bytes[1] as f32 / 10.0),
            temperature_c: bytes[2] as f32 + (bytes[3] as f32 / 10.0),
        })
    }
}

impl<'d> Dht11Reader<'d> {
    /// Real DHT11 transaction: start signal, RMT capture, frame decode.
    fn read_raw(&mut self) -> Result<DhtReading> {
        let mut symbols = [Symbol::default(); SYMBOL_BUFFER_LEN];

        // Pulses shorter than 1 µs are glitches (RMT caps this filter at ~3.2 µs).
        // `signal_range_max` must be LONGER than our ~20 ms start LOW (which
        // can drift up to ~30 ms at 100 Hz FreeRTOS tick rate) so it isn't
        // mistaken for a stop signal, and doubles as the post-frame idle
        // timeout. Ceiling is ~32 ms (15-bit RMT counter at 1 µs resolution).
        let arm_config = ReceiveConfig {
            signal_range_min: Duration::from_nanos(1_000),
            signal_range_max: Duration::from_millis(30),
            timeout: Some(0),
            ..Default::default()
        };
        let wait_config = ReceiveConfig {
            signal_range_min: Duration::from_nanos(1_000),
            signal_range_max: Duration::from_millis(30),
            timeout: Some(FRAME_WAIT_TICKS),
            ..Default::default()
        };

        // 1. Arm the RMT FIRST, while the line is stable HIGH (no edges to
        //    capture yet). `timeout: Some(0)` triggers rmt_receive() to run
        //    (arming the peripheral) and then wait() returns ESP_ERR_TIMEOUT
        //    right away. The channel remains armed.
        //    We do this *before* the start signal because the buffer resize
        //    and syscall overhead inside receive() is enough (tens of µs) to
        //    miss the sensor's 80 µs response if we armed afterwards.
        match self.rx.receive(&mut symbols, &arm_config) {
            Ok(_) => {}
            Err(e) if e.code() == ESP_ERR_TIMEOUT => {}
            Err(e) => bail!("failed to arm RMT: {e:?}"),
        }

        // 2. Drive the start signal. The RMT captures it as the first symbol
        //    (LOW ~20 ms, HIGH ~30 µs), followed by the response (80/80 µs)
        //    and the 40 data bits. `find_response_index` skips over our start
        //    signal because its HIGH half is only ~30 µs.
        self.send_start_signal();

        // 3. Wait for the complete frame. EOF fires ~25 ms after the sensor
        //    releases the line (idle HIGH > signal_range_max).
        let count = self
            .rx
            .receive(&mut symbols, &wait_config)
            .map_err(|e| anyhow!("RMT receive failed: {e:?}"))?;

        Self::decode(&symbols[..count])
    }
}

impl<'d> SensorReader for Dht11Reader<'d> {
    fn read(&mut self) -> Result<DhtReading> {
        self.read_raw()
    }
}
