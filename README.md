# Sensor Sensei

Air quality sensor network in Rust on ESP32. A battery-powered **node** reads
temperature, humidity and dust density, and transmits them over **LoRa**
(point-to-point, not LoRaWAN) to a **gateway** that forwards the data over
WiFi to the [sensor.community](https://sensor.community) API.

```
[Node — TTGO LoRa32]                    [Gateway — TTGO LoRa32]
 DHT11 (temp/humidity, GPIO 13)          SX1276 LoRa RX (internal SPI)
 GP2Y1010 (dust, ADC)          ──LoRa──▶ WiFi ──HTTP POST──▶ sensor.community
 SX1276 LoRa TX (internal SPI)  868 MHz
```

## Hardware

- 2× TTGO LoRa32 v2.1 (ESP32 + SX1276) — **antennas connected on both, always**
- 1× DHT11 (temperature + humidity)
- 1× Sharp GP2Y1010AU0F (dust) — *currently dead; firmware mocks its readings*

### Wiring

Schematics: [DHT11](docs/wiring.png) · [GP2Y1010](docs/wiring_dust.svg)

| Signal              | TTGO pin |
|---------------------|----------|
| DHT11 data          | GPIO 13  |
| GP2Y1010 AOUT       | GPIO 35  |
| GP2Y1010 ILED       | GPIO 25  |

The SX1276 radio is wired internally (SPI2: SCK=5, MISO=19, MOSI=27, CS=18,
RST=23, DIO0=26) — nothing to wire, do not reassign those pins.

## Toolchain setup

```bash
cargo install espup espflash cargo-espflash ldproxy
espup install
. $HOME/export-esp.sh   # add to your shell profile
```

macOS also needs `brew install cmake ninja python3`.

## Build, flash, monitor

Each firmware is an independent Cargo project; run commands from its folder.
The first build compiles ESP-IDF (~10–15 min).

```bash
# Node: sensors + LoRa TX
cd node && cargo espflash flash --monitor

# Gateway: LoRa RX + WiFi forwarding
cd gateway && cargo espflash flash --features lora --monitor

# Gateway with simulated packets (no node needed, default feature)
cd gateway && cargo espflash flash --monitor

# Check compilation only / monitor without reflashing
cargo build
espflash monitor
```

Gateway WiFi credentials and API endpoint are compile-time settings in
`gateway/.cargo/config.toml` (`WIFI_SSID`, `WIFI_PASS`, `API_URL`).

## How it works

1. Every 5 s the node reads the DHT11 and the dust sensor, encodes a
   `SensorPacket` and transmits it (868.1 MHz, SF7, BW 125 kHz, CR 4/5, 14 dBm).
2. The gateway receives, validates the CRC and decodes the packet.
3. It forwards the data as two HTTP POSTs in sensor.community format:
   `X-Pin: 1` carrying `P1`/`P2` (PM10/PM2.5) and `X-Pin: 11` carrying
   `temperature`/`humidity`. Note: `P1` is PM10, **not** PM2.5.

### Wire protocol

The `protocol/` crate is shared by both firmwares so the format can never
drift, and tests on the host (`cd protocol && cargo test`). Payload is
14 bytes, big-endian, floats encoded as scaled integers:

| Bytes | Field        | Encoding        |
|-------|--------------|-----------------|
| 0     | msg type     | `0x01` = data   |
| 1     | node id      | u8              |
| 2–3   | PM2.5        | µg/m³ × 10      |
| 4–5   | PM10         | µg/m³ × 10      |
| 6–7   | temperature  | °C × 100 (i16)  |
| 8–9   | humidity     | % × 100         |
| 10–11 | battery      | mV              |
| 12–13 | CRC-16-CCITT | over bytes 0–11 |

## Project structure

```
node/       Node firmware: DHT11 + GP2Y1010 readers, LoRa TX
gateway/    Gateway firmware: LoRa RX (feature `lora`) or mock, WiFi, HTTP client
protocol/   Shared wire format (SensorPacket encode/decode + CRC), host-testable
```

## Known limitations

- The GP2Y1010 reports a single dust density, so PM2.5 and PM10 carry the
  same value (an SDS011 would provide the real split).
- Battery voltage is not measured yet (`battery_mv` = 0).
- GPS coordinates are hardcoded and WiFi credentials are compile-time
  (allowed by the assignment).
