# Sécurité — Sensor Sensei

Ce document résume les mesures de sécurité appliquées au firmware
`gateway/` (et applicables à `node/` quand il sera développé), et pourquoi.

## 1. Secrets WiFi non commités

- `.gitignore` exclut déjà `.env*`, `*.pem`, `*.key`.
- `gateway/.env.example` documente les variables attendues
  (`WIFI_SSID`, `WIFI_PASS`, `API_URL`) sans valeurs réelles.
  Copier ce fichier en `.env` (ignoré par git) pour le dev local.

**Pourquoi** : éviter de committer accidentellement le mot de passe WiFi
dans l'historique git (qui reste récupérable même après suppression).

## 2. Identifiants WiFi en dur dans le binaire

`src/wifi.rs` utilise `env!("WIFI_SSID")`/`env!("WIFI_PASS")` — ces
valeurs sont injectées au build et finissent en clair dans le binaire
flashé sur l'ESP32.

**Risque** : un attaquant avec accès physique au device peut dumper la
flash et récupérer le mot de passe WiFi.

**Mesures** :
- Commentaire de sécurité ajouté dans `src/wifi.rs` expliquant le risque
  et recommandant un réseau WiFi dédié/invité pour les devices déployés.
- Option `CONFIG_SECURE_FLASH_ENC_ENABLED` documentée (commentée) dans
  `gateway/sdkconfig.defaults` pour le chiffrement de la flash en
  production.
- À terme : passer au mode AP de provisioning prévu dans `PLAN.md`
  (config WiFi au runtime via NVS, pas au build).

## 3. Pas de fuite de credentials dans les logs

Vérifié que `src/wifi.rs` ne logue jamais `SSID`/`PASSWORD` (seulement
l'IP obtenue), avec un commentaire explicite pour éviter toute régression
future — le port série est souvent laissé ouvert et accessible
physiquement.

## 4. HTTPS obligatoire vers sensor.community

`src/api.rs` attache le bundle de certificats ESP-IDF (`crt_bundle_attach`)
dès que l'URL commence par `https`, ce qui est le cas par défaut
(`https://api.sensor.community/...`). Un `log::warn!` a été ajouté quand
une requête part en HTTP non chiffré, pour que tout usage hors test local
(`API_URL=http://...` dans `.env.example`) soit visible dans les logs.

**Pourquoi** : éviter qu'une URL HTTP traîne en "prod" sans qu'on s'en
rende compte (données capteur envoyées en clair, sans authentification du
serveur).

## 5. Robustesse réseau (disponibilité)

- **Timeout HTTP** : `gateway/src/api.rs` fixe `HTTP_TIMEOUT = 10s` sur
  chaque requête vers sensor.community. Sans ça, une connexion qui ne
  répond jamais (AP mort, serveur de test injoignable) bloquait la boucle
  principale indéfiniment.
- **Reconnexion WiFi** : `gateway/src/wifi.rs` expose `ensure_connected()`,
  appelée à chaque itération de la boucle principale (`main.rs`). Si la
  WiFi est tombée, elle relance `connect()`/`wait_netif_up()` au lieu de
  laisser le device hors ligne définitivement après une coupure.

**Pourquoi** : un device de terrain qui perd son WiFi ou tombe sur un
endpoint qui ne répond pas ne doit pas rester bloqué/figé jusqu'au prochain
reboot manuel.
