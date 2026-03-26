# Gateway Firmware — Code Walkthrough

## Overview

The gateway firmware runs on a TTGO LoRa32 v2.1 (ESP32 + SX1276). It receives sensor
packets (currently mocked, later via LoRa), and forwards them to the sensor.community
API over WiFi.

```
MockSource ──> main loop ──> SensorApi ──> HTTP POST ──> sensor.community
  (or LoRa)       │              │
                   │              └── 2 requests: dust (X-Pin:1) + climate (X-Pin:11)
                   └── logs every packet to serial monitor
```

## File by file

### `main.rs` — Entry point

The ESP32 boot sequence calls our `fn main()`. Here's what happens:

1. **`link_patches()`** — Required boilerplate. Patches the Rust runtime to work
   correctly with ESP-IDF's FreeRTOS. Without it, some libc functions won't link.

2. **`EspLogger::initialize_default()`** — Bridges the Rust `log` crate to ESP-IDF's
   logging system. After this call, `log::info!("...")` outputs to the serial monitor
   with the familiar `I (timestamp) tag: message` format. The tag is automatically
   derived from the Rust module path (e.g., `gateway::wifi`).

3. **`Peripherals::take()`** — Singleton pattern. Returns ownership of ALL hardware
   peripherals (GPIO, SPI, UART, modem, etc.) exactly once. Calling it again would
   return an error. This is Rust's ownership model enforcing that two pieces of code
   can't fight over the same hardware pin.

4. **`EspSystemEventLoop::take()`** — ESP-IDF's event bus. WiFi state changes
   (connected, disconnected, got IP) are dispatched through this. `BlockingWifi` uses
   it internally to know when `wait_netif_up()` should unblock.

5. **`EspDefaultNvsPartition::take()`** — Non-Volatile Storage. ESP-IDF's WiFi driver
   needs it to cache calibration data and connection info. It maps to the `nvs` partition
   in our `partitions.csv`.

6. **WiFi connect** — Hands over the modem peripheral to `wifi::connect()`.
   The modem is *moved* (not borrowed), so no other code can use it. This is a
   compile-time guarantee that nothing else can mess with the radio while WiFi is active.

7. **Chip ID** — Reads the WiFi STA MAC address (6 bytes, factory-burned into the ESP32)
   and takes the last 3 bytes as a hex string. This matches the sensor.community convention
   (`esp32-XXYYZZ`). It uniquely identifies this device to the API.

8. **Main loop** — Infinite loop: `receive()` blocks until a packet is ready, then
   `post_sensor_data()` sends it. Errors are logged but don't crash — the loop continues.
   The `wifi` variable must stay in scope (not dropped), otherwise the WiFi connection
   would be torn down.

### `packet.rs` — Data format + abstraction

**`SensorPacket`** — The core data structure. Fields use raw integer encoding
(no floats) to match the 14-byte LoRa wire format:

```
Field         Type   Encoding             Example
─────────────────────────────────────────────────────
msg_type      u8     0x01 = sensor data   0x01
node_id       u8     0-255                1
pm25_raw      u16    µg/m³ × 10           152 = 15.2 µg/m³
pm10_raw      u16    µg/m³ × 10           228 = 22.8 µg/m³
temp_raw      i16    °C × 100             2250 = 22.50°C
humidity_raw  u16    % × 100              5500 = 55.00%
battery_mv    u16    millivolts           4200 = 4.2V
```

All multi-byte fields are big-endian. The last 2 bytes are a CRC16-CCITT checksum
over the first 12 bytes.

Why integers instead of floats? Two reasons:
- LoRa payloads must be compact (< 20 bytes). An f32 is 4 bytes vs 2 bytes for a u16.
- Integer encoding is lossless for the precision we need. No float rounding issues.

The convenience methods (`pm25()`, `temperature()`, etc.) convert back to f32 for
display and API formatting.

**`encode()` / `decode()`** — Serialization for the LoRa wire format. Not used yet
(MockSource creates packets directly), but will be essential in Phase 3 when real
LoRa bytes arrive.

**`PacketSource` trait** — The abstraction that makes mock/lora swappable:

```rust
pub trait PacketSource {
    fn receive(&mut self) -> Result<SensorPacket>;
}
```

`MockSource` implements it now. `LoRaSource` will implement it in Phase 3. The main
loop doesn't know or care which one it's talking to — it just calls `source.receive()`.
This is the Strategy pattern, enforced at compile time via Rust traits.

**`crc16_ccitt()`** — CRC16 with polynomial 0x1021 and init 0xFFFF (CCITT/XMODEM
variant). Processes one bit at a time — no lookup table needed for 12 bytes. The
CRC protects against LoRa transmission errors.

### `mock.rs` — Fake sensor data

Gated behind `#[cfg(feature = "mock")]` — this entire file is excluded from the
binary when building without the `mock` feature.

`MockSource` generates sensor values that oscillate over time using a triangle wave:

```
cycle:  0  1  2  ... 29 30 31 ... 59  0  1 ...
wave:   0  1  2  ... 29 30 29 ... 1   0  1 ...
```

This produces values that slowly drift up and down within realistic ranges,
without needing a random number generator or floating point math.

The **10-second sleep** between packets matches the sensor.community expected
cadence (~2.5 minutes). The API silently drops data sent faster than every 60 seconds.

### `wifi.rs` — WiFi connection

**`env!("WIFI_SSID")` / `env!("WIFI_PASS")`** — Compile-time environment variable
injection. If you forget to set them, the build fails immediately with a clear error.
The values are baked into the binary — no runtime config file needed.

The connection follows ESP-IDF's state machine:

```
EspWifi::new()          → allocates WiFi driver resources
BlockingWifi::wrap()    → wraps it with blocking event-loop integration
set_configuration()     → tells the driver: "connect to this SSID with this password"
start()                 → powers on the radio, loads PHY calibration
connect()               → triggers auth + association (WPA2 handshake)
wait_netif_up()         → blocks until DHCP assigns an IP address
```

`BlockingWifi` is a convenience wrapper. Without it, you'd have to manually subscribe
to system events and write your own state machine to know when the IP is ready.

The function takes `impl WifiModemPeripheral + 'd` instead of a concrete `Modem` type.
This is a trait bound that means "anything that acts as a WiFi modem". It solves a
Rust lifetime issue: the modem peripheral has a lifetime tied to `Peripherals::take()`,
and the trait bound lets the compiler track it correctly.

### `api.rs` — HTTP POST to sensor.community

**Two separate POST requests** per packet, because sensor.community expects different
sensor types on different "pins":

| POST | X-Pin | Content | Sensor type |
|------|-------|---------|-------------|
| 1st  | 1     | P1 (PM10), P2 (PM2.5) | SDS011 (dust) |
| 2nd  | 11    | temperature, humidity | DHT22 (climate) |

Note: P1 = PM10 and P2 = PM2.5. This is counter-intuitive (P1 is NOT PM2.5).

**`option_env!("API_URL")`** — Like `env!()` but returns `Option<&str>` instead of
failing at compile time. If `API_URL` is set, use it. Otherwise, fall back to the
real sensor.community endpoint.

**HTTPS handling** — When the URL starts with `https`, we attach the ESP-IDF certificate
bundle (`crt_bundle_attach`). This bundle contains ~130 root CA certificates embedded in
the firmware (that's why the binary is ~1.3 MB). For plain HTTP (local testing), we skip
it.

**No connection reuse** — Each POST creates a fresh `EspHttpConnection`. At 2 requests
every 2.5 minutes, connection pooling would add complexity for zero benefit.

## Build configuration

### `Cargo.toml` features

```toml
[features]
default = ["mock"]    # cargo build → includes MockSource
mock = []             # cargo build --no-default-features → excludes it
```

In main.rs, `#[cfg(feature = "mock")]` conditionally compiles the mock module and
MockSource instantiation. When LoRaSource is added in Phase 3, it will use
`#[cfg(not(feature = "mock"))]`.

### `partitions.csv`

Custom flash partition layout giving ~2 MB to the app (default is 1 MB, too small
for the TLS certificate bundle). Used by espflash via `espflash.toml`.

### `sdkconfig.defaults`

ESP-IDF build configuration. Our additions enable the TLS certificate bundle for HTTPS.
Stack sizes are increased from the C defaults because Rust uses more stack space.

## Environment variables

| Variable | Required | Purpose |
|----------|----------|---------|
| `WIFI_SSID` | Yes (build) | WiFi network name, baked into binary |
| `WIFI_PASS` | Yes (build) | WiFi password, baked into binary |
| `API_URL` | No (build) | Override API endpoint (default: sensor.community) |

## Test with a local server

```bash
# Terminal 1: local HTTP server that accepts POST and prints the body
python3 -c "
from http.server import HTTPServer, BaseHTTPRequestHandler
class H(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers['Content-Length'])
        print(self.headers)
        print(self.rfile.read(length).decode())
        self.send_response(201)
        self.end_headers()
HTTPServer(('', 8080), H).serve_forever()
"

# Terminal 2: build + flash
WIFI_SSID="MyWiFi" WIFI_PASS="MyPass" API_URL="http://<PC_IP>:8080" cargo espflash flash --monitor
```
