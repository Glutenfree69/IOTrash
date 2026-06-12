# CLAUDE.md — Sensor Sensei

## Contexte

Projet "Sensor Sensei" : capteur qualité de l'air avec communication LoRa vers une gateway WiFi qui forward les données à sensor.community.

Lire README.md pour l'architecture, le protocole et les commandes.

## Stack

- Rust avec esp-idf-hal (std, pas no_std)
- Target : ESP32 (TTGO LoRa32 v2.1, Xtensa LX6)
- Toolchain : espup + espflash
- Pas d'IDE, tout en terminal

## Règles de code

- Code en anglais (commentaires, noms de variables, docs)
- Messages de commit en anglais
- Pas de unwrap() en prod — utiliser `?` ou `anyhow::Result` partout
- Logging via `log` crate (`info!`, `warn!`, `error!`)
- Préférer les abstractions avec des traits (PacketSource, SensorReader)
  pour pouvoir mock/test facilement
- Pas d'allocations inutiles — on est sur un microcontrôleur
- Garder les payloads LoRa compacts (< 20 bytes)
- Encoder les floats en entiers × facteur pour le payload
  (ex: 22.5°C → 2250 en i16)

## Structure workspace

```
gateway/    → firmware gateway (LoRa RX + WiFi + HTTP)
node/       → firmware node capteur (DHT11 + dust + LoRa TX)
protocol/   → crate partagé (SensorPacket, encode/decode, CRC) — testable sur l'hôte
```

Chaque firmware est un projet Rust indépendant avec son propre Cargo.toml.
Ignorer le dossier `miner/`.

## Commandes utiles

Ne jamais lancer les commandes cargo soi-même — Dylan build/flash lui-même.

```bash
# Node : build + flash + monitor
cd node && cargo espflash flash --monitor

# Gateway en LoRa réel
cd gateway && cargo espflash flash --features lora --monitor

# Gateway en mock (défaut, sans node)
cd gateway && cargo espflash flash --monitor

# Tests du protocole (sur l'hôte)
cd protocol && cargo test
```

## Points d'attention

- Le TTGO LoRa32 a des pins SPI fixes pour le SX1276 — ne pas les
  réassigner. Vérifier le pinout selon la version de la board.
- sensor.community API : X-Pin=1 pour SDS011, P1=PM10, P2=PM2.5
  (attention P1 n'est PAS PM2.5, c'est contre-intuitif)
- WiFi credentials en dur pour le dev, AP mode config pour la prod
- GPS coordinates hardcodées (autorisé par le sujet)
- Feature flag `mock` (défaut) / `lora` côté gateway — `lora` a priorité si les deux sont actives
- Le capteur de poussière GP2Y1010 est mort : son mock dans `node/src/dust.rs` est volontaire, NE PAS le retirer
