//! LoRa packet sink: transmits `SensorPacket`s to the gateway over the SX1276
//! radio (point-to-point, not LoRaWAN).
//!
//! `lora-phy` is async; we keep the node's blocking control flow by driving
//! each radio operation to completion with `esp_idf_svc::hal::task::block_on`.
//!
//! Radio config (must match the gateway exactly): 868.1 MHz, SF7, BW 125 kHz,
//! CR 4/5, private sync word (`enable_public_network = false`).

use anyhow::{anyhow, Result};
use embedded_hal::digital::OutputPin;
use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::digital::Wait;
use embedded_hal_async::spi::SpiDevice;
use esp_idf_svc::hal::task::block_on;

use lora_phy::iv::GenericSx127xInterfaceVariant;
use lora_phy::mod_params::{
    Bandwidth, CodingRate, ModulationParams, PacketParams, SpreadingFactor,
};
use lora_phy::mod_traits::RadioKind;
use lora_phy::sx127x::{self, Sx127x, Sx1276};
use lora_phy::LoRa;

use protocol::SensorPacket;

const LORA_FREQUENCY_HZ: u32 = 868_100_000;
const PREAMBLE_LEN: u16 = 8;
/// EU 868 MHz band ERP limit is 14 dBm.
const TX_POWER_DBM: i32 = 14;

/// A sink for outgoing sensor packets (the LoRa radio).
pub trait PacketSink {
    fn send(&mut self, packet: &SensorPacket) -> Result<()>;
}

struct LoRaSink<RK, DLY>
where
    RK: RadioKind,
    DLY: DelayNs,
{
    lora: LoRa<RK, DLY>,
    mdltn: ModulationParams,
    tx_params: PacketParams,
}

/// Build a LoRa-backed [`PacketSink`]. `spi` must be an async SPI device with
/// CS managed (e.g. esp-idf-hal `SpiDeviceDriver`), `reset` the SX1276 RST pin,
/// `irq` the DIO0 pin (TxDone/RxDone).
pub fn new_sink<SPI, RST, IRQ>(spi: SPI, reset: RST, irq: IRQ) -> Result<impl PacketSink>
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
            rx_boost: false,
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

        let tx_params = lora
            .create_tx_packet_params(PREAMBLE_LEN, false, true, false, &mdltn)
            .map_err(|e| anyhow!("tx packet params failed: {e:?}"))?;

        log::info!("LoRa TX ready @ {} Hz (SF7/BW125/CR4-5)", LORA_FREQUENCY_HZ);
        anyhow::Ok(LoRaSink {
            lora,
            mdltn,
            tx_params,
        })
    })
}

impl<RK, DLY> PacketSink for LoRaSink<RK, DLY>
where
    RK: RadioKind,
    DLY: DelayNs,
{
    fn send(&mut self, packet: &SensorPacket) -> Result<()> {
        let buf = packet.encode();
        let lora = &mut self.lora;
        let mdltn = &self.mdltn;
        let tx_params = &mut self.tx_params;

        block_on(async {
            lora.prepare_for_tx(mdltn, tx_params, TX_POWER_DBM, &buf)
                .await
                .map_err(|e| anyhow!("prepare_for_tx failed: {e:?}"))?;
            lora.tx().await.map_err(|e| anyhow!("tx failed: {e:?}"))
        })?;

        log::info!("LoRa TX: {} bytes sent (node={})", buf.len(), packet.node_id);
        Ok(())
    }
}
