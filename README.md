# Sensor Sensei

Air quality sensor with LoRa communication to a WiFi gateway that forwards data to [sensor.community](https://sensor.community).

## Architecture

```
[Node capteur]                    [Gateway]
TTGO LoRa32 #2                   TTGO LoRa32 #1
+ SDS011 (PM2.5/PM10, UART)      Receives LoRa packets
+ DHT22 (temp/humidity, GPIO)     Forwards via WiFi → sensor.community
Battery powered, deep sleep       Mains powered, WiFi connected
```

## Prerequisites

### Hardware

- 2x TTGO LoRa32 v2.1 (ESP32 + SX1276)
- 1x SDS011 (particulate matter sensor, UART)
- 1x DHT22 or BME280 (temperature + humidity)
- LiPo 3.7V battery (for the node)
- USB cable for flashing

### Software

1. **Rust** (stable) via [rustup](https://rustup.rs):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **espup** — installs the Xtensa Rust toolchain and ESP-IDF tools:
   ```bash
   cargo install espup
   espup install
   ```
   After install, source the environment file (add to your shell profile if you are a virgin):
   ```bash
   # On macOS/Linux
   . $HOME/export-esp.sh
   ```

3. **espflash** — flash and monitor tool:
   ```bash
   cargo install espflash         # standalone CLI — used for `espflash monitor` and direct flashing
   cargo install cargo-espflash  # cargo plugin wrapper — needed for `cargo espflash` commands for the cool dudes
   ```

4. **ldproxy** — linker proxy required by esp-idf:
   ```bash
   cargo install ldproxy
   ```

5. **System dependencies** (macOS) - seems like some bullshit here:
   ```bash
   # Via Homebrew
   brew install cmake ninja python3
   ```

   On Linux (Debian/Ubuntu):
   ```bash
   sudo apt install git curl gcc ninja-build cmake python3 python3-venv
   ```

## Getting started

```bash
git clone <repo-url>
cd IOTrash
```

### Build the gateway firmware

```bash
cd gateway
cargo build
```

The first build will download and compile ESP-IDF (~10-15 min). Subsequent builds are much faster.

### Flash and monitor

Connect the TTGO LoRa32 via USB, then:

```bash
cd gateway
cargo espflash flash --monitor
```

This builds, flashes the board, and opens a serial monitor. Press `Ctrl+c` to quit the monitor. You can do it with espflash command as well but cargo is for cool dudes

### Monitor only (no reflash)

```bash
espflash monitor
```

### Build with mock data (no LoRa hardware needed)

```bash
cargo build --features mock
```

## Project structure

```
gateway/    — Gateway firmware (receives LoRa, forwards to sensor.community)
node/       — Node firmware (reads sensors, sends via LoRa)
docs/       — Project documentation
```

See [PLAN.md](PLAN.md) for the full architecture, protocol spec, and development phases.

## Useful links

- [sensor.community](https://sensor.community) — Data platform
- [esp-rs book](https://docs.esp-rs.org/book/) — Rust on ESP32 guide
- [esp-idf-hal docs](https://docs.esp-rs.org/esp-idf-hal/) — HAL documentation
