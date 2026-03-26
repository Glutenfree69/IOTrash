#![cfg(feature = "mock")]

use anyhow::Result;
use std::thread;
use std::time::Duration;

use crate::packet::{PacketSource, SensorPacket, MSG_TYPE_SENSOR_DATA};

const INTERVAL_SECS: u64 = 10;

pub struct MockSource {
    node_id: u8,
    cycle: u32,
}

impl MockSource {
    pub fn new(node_id: u8) -> Self {
        Self { node_id, cycle: 0 }
    }
}

impl PacketSource for MockSource {
    fn receive(&mut self) -> Result<SensorPacket> {
        if self.cycle > 0 {
            log::info!("MockSource: sleeping {}s until next packet...", INTERVAL_SECS);
            thread::sleep(Duration::from_secs(INTERVAL_SECS));
        }

        // Oscillate values using simple triangle wave based on cycle
        // cycle % 60 gives a 0-59 range, mapped to oscillation
        let phase = (self.cycle % 60) as i32;
        let wave = if phase < 30 { phase } else { 60 - phase }; // 0 → 30 → 0

        // PM2.5: 12.0 - 25.0 ug/m3 → raw 120 - 250
        let pm25_raw = 120 + (wave as u16 * 130 / 30); // 120..250

        // PM10: ~1.5x PM2.5 + offset
        let pm10_raw = pm25_raw * 3 / 2 + 20;

        // Temperature: 20.0 - 25.0 C → raw 2000 - 2500
        let temp_raw = 2000 + (wave as i16 * 500 / 30);

        // Humidity: 45.0 - 65.0% → raw 4500 - 6500
        let humidity_raw = 4500 + (wave as u16 * 2000 / 30);

        // Battery: slow decrease from 4200 to 3600 mV over ~600 cycles
        let battery_mv = 4200u16.saturating_sub(self.cycle as u16);

        let packet = SensorPacket {
            msg_type: MSG_TYPE_SENSOR_DATA,
            node_id: self.node_id,
            pm25_raw,
            pm10_raw,
            temp_raw,
            humidity_raw,
            battery_mv,
        };

        log::info!(
            "MockSource: node={} PM2.5={:.1} PM10={:.1} T={:.1}C H={:.1}% bat={}mV",
            packet.node_id,
            packet.pm25(),
            packet.pm10(),
            packet.temperature(),
            packet.humidity(),
            packet.battery_mv,
        );

        self.cycle += 1;

        Ok(packet)
    }
}
