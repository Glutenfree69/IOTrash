use anyhow::{bail, Result};

pub const MSG_TYPE_SENSOR_DATA: u8 = 0x01;
pub const PACKET_SIZE: usize = 14;

pub trait PacketSource {
    fn receive(&mut self) -> Result<SensorPacket>;
}

#[derive(Debug, Clone)]
pub struct SensorPacket {
    pub msg_type: u8,
    pub node_id: u8,
    pub pm25_raw: u16,
    pub pm10_raw: u16,
    pub temp_raw: i16,
    pub humidity_raw: u16,
    pub battery_mv: u16,
}

impl SensorPacket {
    pub fn pm25(&self) -> f32 {
        self.pm25_raw as f32 / 10.0
    }

    pub fn pm10(&self) -> f32 {
        self.pm10_raw as f32 / 10.0
    }

    pub fn temperature(&self) -> f32 {
        self.temp_raw as f32 / 100.0
    }

    pub fn humidity(&self) -> f32 {
        self.humidity_raw as f32 / 100.0
    }

    pub fn encode(&self) -> [u8; PACKET_SIZE] {
        let mut buf = [0u8; PACKET_SIZE];
        buf[0] = self.msg_type;
        buf[1] = self.node_id;
        buf[2..4].copy_from_slice(&self.pm25_raw.to_be_bytes());
        buf[4..6].copy_from_slice(&self.pm10_raw.to_be_bytes());
        buf[6..8].copy_from_slice(&self.temp_raw.to_be_bytes());
        buf[8..10].copy_from_slice(&self.humidity_raw.to_be_bytes());
        buf[10..12].copy_from_slice(&self.battery_mv.to_be_bytes());
        let crc = crc16_ccitt(&buf[..12]);
        buf[12..14].copy_from_slice(&crc.to_be_bytes());
        buf
    }

    pub fn decode(buf: &[u8; PACKET_SIZE]) -> Result<Self> {
        let expected_crc = u16::from_be_bytes([buf[12], buf[13]]);
        let actual_crc = crc16_ccitt(&buf[..12]);
        if expected_crc != actual_crc {
            bail!(
                "CRC mismatch: expected 0x{:04X}, got 0x{:04X}",
                expected_crc,
                actual_crc
            );
        }

        let msg_type = buf[0];
        if msg_type != MSG_TYPE_SENSOR_DATA {
            bail!("Unknown message type: 0x{:02X}", msg_type);
        }

        Ok(Self {
            msg_type,
            node_id: buf[1],
            pm25_raw: u16::from_be_bytes([buf[2], buf[3]]),
            pm10_raw: u16::from_be_bytes([buf[4], buf[5]]),
            temp_raw: i16::from_be_bytes([buf[6], buf[7]]),
            humidity_raw: u16::from_be_bytes([buf[8], buf[9]]),
            battery_mv: u16::from_be_bytes([buf[10], buf[11]]),
        })
    }
}

fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}
