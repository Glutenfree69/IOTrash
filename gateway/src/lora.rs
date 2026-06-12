//! LoRa packet source: receives `SensorPacket`s from sensor nodes over the
//! SX1276 radio (point-to-point, not LoRaWAN).
//!
//! `lora-phy` is async; we keep the gateway's blocking control flow by driving
//! each radio operation to completion with `esp_idf_svc::hal::task::block_on`.
//!
//! Radio config (must match the node exactly): 868.1 MHz, SF7, BW 125 kHz,
//! CR 4/5, private sync word (`enable_public_network = false`).

#![cfg(feature = "lora")]

use anyhow::{anyhow, bail, Result};
use embedded_hal::digital::OutputPin;
use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::digital::Wait;
use embedded_hal_async::spi::SpiDevice;
use esp_idf_svc::hal::task::block_on;

use lora_phy::mod_params::{
    Bandwidth, CodingRate, ModulationParams, PacketParams, SpreadingFactor,
};
use lora_phy::iv::GenericSx127xInterfaceVariant;
use lora_phy::mod_traits::RadioKind;
use lora_phy::sx127x::{self, Sx127x, Sx1276};
use lora_phy::{LoRa, RxMode};

use crate::packet::{PacketSource, SensorPacket, PACKET_SIZE};

const LORA_FREQUENCY_HZ: u32 = 868_100_000;
const PREAMBLE_LEN: u16 = 8;

struct LoRaSource<RK, DLY>
where
    RK: RadioKind,
    DLY: DelayNs,
{
    lora: LoRa<RK, DLY>,
    mdltn: ModulationParams,
    rx_params: PacketParams,
}

/// Build a LoRa-backed [`PacketSource`]. `spi` must be an async SPI device with
/// CS managed (e.g. esp-idf-hal `SpiDeviceDriver`), `reset` the SX1276 RST pin,
/// `irq` the DIO0 pin (TxDone/RxDone).
pub fn new_source<SPI, RST, IRQ>(spi: SPI, reset: RST, irq: IRQ) -> Result<impl PacketSource>
where
    SPI: SpiDevice<u8>,
    RST: OutputPin,
    IRQ: Wait,
{
    block_on(async move {
        let iv = GenericSx127xInterfaceVariant::new(reset, irq, None, None)
            .map_err(|e| anyhow!("interface variant init failed: {e:?}"))?;

        let config = sx127x::Config {
            chip: Sx1276,
            tcxo_used: false,
            tx_boost: true,
            rx_boost: true,
        };

        let mut lora = LoRa::new(Sx127x::new(spi, iv, config), false, embassy_time::Delay)
            .await
            .map_err(|e| anyhow!("LoRa init failed: {e:?}"))?;

        let mdltn = lora
            .create_modulation_params(
                SpreadingFactor::_7,
                Bandwidth::_125KHz,
                CodingRate::_4_5,
                LORA_FREQUENCY_HZ,
            )
            .map_err(|e| anyhow!("modulation params failed: {e:?}"))?;

        let rx_params = lora
            .create_rx_packet_params(PREAMBLE_LEN, false, 255, true, false, &mdltn)
            .map_err(|e| anyhow!("rx packet params failed: {e:?}"))?;

        log::info!("LoRa RX ready @ {} Hz (SF7/BW125/CR4-5)", LORA_FREQUENCY_HZ);
        anyhow::Ok(LoRaSource {
            lora,
            mdltn,
            rx_params,
        })
    })
}

impl<RK, DLY> PacketSource for LoRaSource<RK, DLY>
where
    RK: RadioKind,
    DLY: DelayNs,
{
    fn receive(&mut self) -> Result<SensorPacket> {
        let mut buf = [0u8; 255];
        let lora = &mut self.lora;
        let mdltn = &self.mdltn;
        let rx_params = &self.rx_params;

        let (len, status) = block_on(async {
            lora.prepare_for_rx(RxMode::Continuous, mdltn, rx_params)
                .await
                .map_err(|e| anyhow!("prepare_for_rx failed: {e:?}"))?;
            lora.rx(rx_params, &mut buf)
                .await
                .map_err(|e| anyhow!("rx failed: {e:?}"))
        })?;

        log::info!(
            "LoRa RX: {} bytes, RSSI={} dBm, SNR={}",
            len,
            status.rssi,
            status.snr
        );

        if len as usize != PACKET_SIZE {
            bail!("unexpected payload size {len} (want {PACKET_SIZE})");
        }
        let frame: [u8; PACKET_SIZE] = buf[..PACKET_SIZE].try_into().unwrap();
        SensorPacket::decode(&frame)
    }
}
