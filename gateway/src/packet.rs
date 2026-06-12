//! Gateway-side packet handling.
//!
//! The wire format (`SensorPacket`, encode/decode, CRC) lives in the shared
//! `protocol` crate so the node and gateway can never drift. This module only
//! re-exports those types and adds the gateway-only `PacketSource` abstraction
//! (mock vs real LoRa reception).

use anyhow::Result;

// MSG_TYPE_SENSOR_DATA is used by the mock source; allow it to be unused under
// other feature combinations (e.g. `--features lora` without `mock`).
#[allow(unused_imports)]
pub use protocol::{SensorPacket, MSG_TYPE_SENSOR_DATA, PACKET_SIZE};

/// A source of incoming sensor packets (mock generator or LoRa receiver).
pub trait PacketSource {
    fn receive(&mut self) -> Result<SensorPacket>;
}
