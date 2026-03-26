# Sensor Sensei — PLAN.md

## Objectif

Projet EPITECH : capteur qualité de l'air compatible sensor.community, utilisant LoRa pour la communication longue distance vers une gateway WiFi dédiée.

## Architecture

```
[Node capteur]                    [Gateway]
TTGO LoRa32 #2                   TTGO LoRa32 #1 (celui qu'on a)
+ SDS011 (PM2.5/PM10, UART)      Reçoit LoRa
+ DHT22 (temp/humidité, GPIO)    Forward WiFi → sensor.community
Sur batterie, deep sleep          Sur secteur, WiFi connecté
Envoie payload LoRa toutes        HTTP POST vers API
les 2-5 minutes                   sensor.community
```

## Hardware

- 2x TTGO LoRa32 v2.1 (ESP32 + SX1276 LoRa intégré)
- 1x SDS011 (capteur particules fines PM2.5/PM10, interface UART)
- 1x DHT22 ou BME280 (température + humidité)
- Batterie LiPo 3.7V pour le node autonome
- Jumper wires + breadboard

## Stack technique

- Langage : Rust (esp-idf-hal, std)
- Toolchain : espup (Xtensa), espflash
- Framework : esp-idf-hal + esp-idf-svc (std, avec ESP-IDF sous le capot)
- Build : cargo + espflash, tout en terminal (pas d'IDE)
- Dev sur Mac Mini M2, nix-darwin

## Workspace structure

```
sensor-sensei/
├── PLAN.md
├── CLAUDE.md
├── gateway/                  # Firmware gateway (phase 1)
│   ├── Cargo.toml
│   ├── sdkconfig.defaults
│   └── src/
│       ├── main.rs           # Entry point, loop receive → forward
│       ├── lora.rs           # LoRa receive (SX1276 via SPI interne TTGO)
│       ├── wifi.rs           # WiFi connect + reconnect
│       ├── api.rs            # HTTP POST vers sensor.community
│       ├── packet.rs         # Struct SensorPacket + encode/decode
│       └── mock.rs           # MockSource pour simuler des packets LoRa
├── node/                     # Firmware node capteur (phase 2)
│   ├── Cargo.toml
│   ├── sdkconfig.defaults
│   └── src/
│       ├── main.rs           # Entry point, loop read → send → deep sleep
│       ├── lora.rs           # LoRa send
│       ├── sensors.rs        # Lecture SDS011 (UART) + DHT22 (GPIO)
│       ├── packet.rs         # Même struct SensorPacket (shared)
│       └── power.rs          # Deep sleep management
└── docs/
    ├── architecture.md       # Choix d'implémentation vs firmware actuel
    ├── crafting.md           # Guide montage hardware
    ├── firmware.md           # Doc firmware (build, flash, config)
    └── user.md              # Guide utilisateur
```

## Protocole LoRa (point-à-point, PAS LoRaWAN)

### Format du payload (compact, 14 bytes)

```
Byte 0     : message type (0x01 = sensor data)
Byte 1     : node_id (0-255)
Byte 2-3   : PM2.5 (u16 big-endian, en µg/m³ × 10)
Byte 4-5   : PM10 (u16 big-endian, en µg/m³ × 10)
Byte 6-7   : temperature (i16 big-endian, en °C × 100)
Byte 8-9   : humidity (u16 big-endian, en % × 100)
Byte 10-11 : battery voltage (u16 big-endian, en mV)
Byte 12-13 : CRC16 checksum
```

### Config radio LoRa

- Fréquence : 868.1 MHz (EU)
- Spreading Factor : SF7 (bon compromis portée/débit)
- Bandwidth : 125 kHz
- Coding Rate : 4/5

## API sensor.community

Endpoint : `POST https://api.sensor.community/v1/push-sensor-data/`

Headers requis :
```
X-Pin: 1              (pour SDS011)
X-Sensor: esp32-<chipid>
Content-Type: application/json
```

Body :
```json
{
  "software_version": "sensor-sensei-0.1",
  "sensordatavalues": [
    {"value_type": "P1", "value": "18.5"},
    {"value_type": "P2", "value": "42.3"},
    {"value_type": "temperature", "value": "22.5"},
    {"value_type": "humidity", "value": "55.0"}
  ]
}
```

P1 = PM10, P2 = PM2.5.

## Phases de développement

### Phase 1 — Gateway avec MockSource (maintenant, 1 seul TTGO)

**Objectif** : gateway fonctionnelle qui POST vers sensor.community avec des données simulées.

- [ ] 1.1 — Setup projet Rust ESP32 (cargo generate, sdkconfig, build test)
- [ ] 1.2 — WiFi connect (esp-idf-svc::wifi, SSID/password en config)
- [ ] 1.3 — Définir SensorPacket struct + encode/decode binaire
- [ ] 1.4 — Implémenter MockSource (génère des faux packets réalistes)
- [ ] 1.5 — Implémenter PacketSource trait (abstraction mock/lora)
- [ ] 1.6 — HTTP POST vers sensor.community API (esp-idf-svc::http)
- [ ] 1.7 — Vérifier les données sur https://maps.sensor.community
- [ ] 1.8 — WiFi AP mode pour configuration (SSID, coordonnées GPS)
- [ ] 1.9 — Tests + logging

### Phase 2 — Firmware node capteur (quand 2ème TTGO reçu)

- [ ] 2.1 — LoRa send sur TTGO LoRa32 (SX1276 via SPI interne)
- [ ] 2.2 — Lecture SDS011 via UART
- [ ] 2.3 — Lecture DHT22 via GPIO
- [ ] 2.4 — Encode SensorPacket → payload LoRa
- [ ] 2.5 — Deep sleep entre les envois (économie batterie)
- [ ] 2.6 — Gestion batterie (lecture voltage ADC, inclure dans payload)

### Phase 3 — Intégration LoRa gateway (remplacer MockSource)

- [ ] 3.1 — Implémenter LoRaSource (réception réelle)
- [ ] 3.2 — Gestion multi-nodes (routing par node_id)
- [ ] 3.3 — Test end-to-end node → gateway → sensor.community
- [ ] 3.4 — Range test (vérifier portée LoRa)

### Phase 4 — Deliverables & polish

- [ ] 4.1 — Listing features firmware sensor.community actuel
- [ ] 4.2 — Documentation (architecture, crafting, firmware, user)
- [ ] 4.3 — Visualisation données (Grafana ou dashboard web custom)
- [ ] 4.4 — Optimisation consommation énergie node
- [ ] 4.5 — WiFi AP config portal sur la gateway

## Notes

- Le TTGO LoRa32 a le SX1276 câblé en interne sur des pins SPI fixes
  (vérifier la version de la board pour le pinout exact)
- sensor.community demande un enregistrement du capteur sur leur site
  avec les coordonnées GPS — on peut les hardcoder (autorisé par le sujet)
- Le firmware actuel sensor.community est en C++ (Arduino framework),
  on réécrit from scratch en Rust — documenter les différences
