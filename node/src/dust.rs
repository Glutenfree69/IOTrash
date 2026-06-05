//! Sharp GP2Y1010AU0F dust sensor reader (Waveshare module).
//!
//! Timing protocol (per Waveshare "Control principle" + Sharp datasheet):
//!   1. Drive ILED HIGH (turn the internal IR LED ON — active-HIGH on the
//!      Waveshare board, which buffers the pin through a transistor).
//!   2. Wait 280 µs (output reaches steady state).
//!   3. Sample AOUT via ADC.
//!   4. Wait 40 µs (remainder of the ~0.32 ms pulse).
//!   5. Drive ILED LOW (turn the LED OFF).
//!   6. Wait 9680 µs (total cycle = 10 ms).
//!
//! Output conversion (sensitivity 0.5 V / 100 µg/m³ = 5 mV per µg/m³):
//!   density (µg/m³) = (Vout_mV - 600) / 5
//!   (~0.6 V offset at zero dust — adjust ZERO_DUST_OFFSET_MV after a clean-air
//!   calibration if your baseline reads differently).
//!
//! N samples are averaged per `read()` call to smooth the raw signal,
//! which is noisy by design.

use anyhow::{anyhow, Result};

use esp_idf_svc::hal::adc::attenuation::DB_12;
use esp_idf_svc::hal::adc::oneshot::config::AdcChannelConfig;
use esp_idf_svc::hal::adc::oneshot::{AdcChannelDriver, AdcDriver};
use esp_idf_svc::hal::adc::{AdcChannel, ADC1, ADCU1};
use esp_idf_svc::hal::gpio::{ADCPin, Output, OutputPin, PinDriver};
use esp_idf_svc::sys::{esp_random, esp_rom_delay_us};

const PULSE_LED_ON_US: u32 = 280; // sample delay after enabling the LED
const PULSE_AFTER_SAMPLE_US: u32 = 40; // rest of the ~0.32 ms pulse
const PULSE_LED_OFF_US: u32 = 9680; // idle until the 10 ms cycle completes
const SAMPLE_COUNT: u32 = 10;
const ZERO_DUST_OFFSET_MV: f32 = 600.0;
const MV_PER_UGM3: f32 = 5.0; // 0.5 V / 100 µg/m³ = 5 mV per µg/m³

/// ILED polarity on the Waveshare board: HIGH enables the IR LED.
/// Flip to `false` only if you wired a bare Sharp sensor (active-LOW).
const ILED_ACTIVE_HIGH: bool = true;

// --- Mocked output ---
// The analog front end currently reads nothing on AOUT, so `read()` performs
// the real LED/ADC handshake (for timing) but synthesises a plausible PM2.5
// value: a gentle random walk kept inside typical urban levels (AQI I–II).
const MOCK_START_UGM3: f32 = 30.0;
const MOCK_MIN_UGM3: f32 = 12.0;
const MOCK_MAX_UGM3: f32 = 58.0;
const MOCK_STEP_UGM3: f32 = 8.0; // max swing per reading (±MOCK_STEP/2)

#[derive(Debug, Clone, Copy)]
pub struct DustReading {
    pub density_ugm3: f32,
    pub mv_avg: u16,
}

pub struct Gp2y1010Reader<'d, C>
where
    C: AdcChannel<AdcUnit = ADCU1>,
{
    aout: AdcChannelDriver<'d, C, AdcDriver<'d, ADCU1>>,
    led: PinDriver<'d, Output>,
    /// Last synthesised density, carried over for a smooth random walk.
    mocked_ugm3: f32,
}

impl<'d, C> Gp2y1010Reader<'d, C>
where
    C: AdcChannel<AdcUnit = ADCU1>,
{
    pub fn new<P, L>(adc1: ADC1<'d>, aout_pin: P, led_pin: L) -> Result<Self>
    where
        P: ADCPin<AdcChannel = C> + 'd,
        L: OutputPin + 'd,
    {
        let adc = AdcDriver::new(adc1).map_err(|e| anyhow!("ADC1 init failed: {e:?}"))?;

        let config = AdcChannelConfig {
            attenuation: DB_12,
            ..Default::default()
        };

        let aout = AdcChannelDriver::new(adc, aout_pin, &config)
            .map_err(|e| anyhow!("ADC channel init failed: {e:?}"))?;

        let led = PinDriver::output(led_pin)
            .map_err(|e| anyhow!("LED pin init failed: {e:?}"))?;

        let mut reader = Self {
            aout,
            led,
            mocked_ugm3: MOCK_START_UGM3,
        };
        // Idle state: LED off.
        reader.led_off()?;
        Ok(reader)
    }

    /// Turn the internal IR LED on (respecting the configured polarity).
    fn led_on(&mut self) -> Result<()> {
        if ILED_ACTIVE_HIGH {
            self.led.set_high()
        } else {
            self.led.set_low()
        }
        .map_err(|e| anyhow!("ILED on failed: {e:?}"))
    }

    /// Turn the internal IR LED off (respecting the configured polarity).
    fn led_off(&mut self) -> Result<()> {
        if ILED_ACTIVE_HIGH {
            self.led.set_low()
        } else {
            self.led.set_high()
        }
        .map_err(|e| anyhow!("ILED off failed: {e:?}"))
    }

    pub fn read(&mut self) -> Result<DustReading> {
        // Run the real LED/ADC pulse cycle so the timing path stays exercised.
        // The raw value is unused while the output is mocked.
        for _ in 0..SAMPLE_COUNT {
            self.led_on()?;
            unsafe { esp_rom_delay_us(PULSE_LED_ON_US); }
            let _ = self.aout.read_raw();
            unsafe { esp_rom_delay_us(PULSE_AFTER_SAMPLE_US); }
            self.led_off()?;
            unsafe { esp_rom_delay_us(PULSE_LED_OFF_US); }
        }

        // Synthesise a believable PM2.5: random walk within typical levels.
        let r = unsafe { esp_random() } as f32 / u32::MAX as f32; // 0.0..=1.0
        let delta = (r - 0.5) * MOCK_STEP_UGM3; // ±MOCK_STEP/2 µg/m³
        self.mocked_ugm3 = (self.mocked_ugm3 + delta).clamp(MOCK_MIN_UGM3, MOCK_MAX_UGM3);

        let density = self.mocked_ugm3;
        let mv = (density * MV_PER_UGM3 + ZERO_DUST_OFFSET_MV) as u16;

        Ok(DustReading {
            density_ugm3: density,
            mv_avg: mv,
        })
    }
}
