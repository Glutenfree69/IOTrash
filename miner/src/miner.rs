use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use esp_idf_svc::http::client::{Configuration as HttpConfig, EspHttpConnection};
use sha1::{Digest, Sha1};

const POOL_URL: &str = "https://server.duinocoin.com/getPool";
const MINER_BANNER: &str = "SensorSensei Miner";
const RECONNECT_DELAY: Duration = Duration::from_secs(5);

pub struct DucoMiner {
    username: &'static str,
    mining_key: &'static str,
    device_id: String,
}

impl DucoMiner {
    pub fn new(chip_id: &str) -> Self {
        Self {
            username: env!("DUCO_USERNAME"),
            mining_key: env!("DUCO_MINING_KEY"),
            device_id: chip_id.to_string(),
        }
    }

    pub fn run(&self) -> Result<()> {
        let mut accepted: u32 = 0;
        let mut rejected: u32 = 0;

        loop {
            if let Err(e) = self.mine_session(&mut accepted, &mut rejected) {
                log::error!("Mining session error: {e}");
                log::info!("Reconnecting in {}s...", RECONNECT_DELAY.as_secs());
                std::thread::sleep(RECONNECT_DELAY);
            }
        }
    }

    fn mine_session(&self, accepted: &mut u32, rejected: &mut u32) -> Result<()> {
        let (ip, port) = fetch_pool()?;
        log::info!("Pool node: {}:{}", ip, port);

        let stream = TcpStream::connect(format!("{}:{}", ip, port))
            .context("Failed to connect to mining node")?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;

        let mut stream = BufReader::new(stream);

        // Read server version
        let version = read_line(&mut stream)?;
        log::info!("Connected to Duino-Coin server v{}", version.trim());

        loop {
            // Request job
            let job_request = format!("JOB,{},ESP32,{}\n", self.username, self.mining_key);
            stream.get_mut().write_all(job_request.as_bytes())?;
            stream.get_mut().flush()?;

            // Read job response
            let job_line = read_line(&mut stream)?;
            let parts: Vec<&str> = job_line.trim().split(',').collect();
            if parts.len() < 3 {
                return Err(anyhow!("Invalid job response: {}", job_line));
            }

            let last_hash = parts[0];
            let expected_hash_hex = parts[1];
            let difficulty: u32 = parts[2]
                .parse()
                .context("Failed to parse difficulty")?;

            let expected_hash = hex_to_bytes(expected_hash_hex)
                .context("Failed to decode expected hash")?;

            let max_nonce = difficulty * 100 + 1;
            log::info!("Job received: difficulty={}, range=0..{}", difficulty, max_nonce);

            // Brute-force DUCO-S1
            let start = Instant::now();
            let mut found = false;

            for nonce in 0..max_nonce {
                let mut hasher = Sha1::new();
                hasher.update(last_hash.as_bytes());
                hasher.update(nonce.to_string().as_bytes());
                let result = hasher.finalize();

                if result[..] == expected_hash[..] {
                    let elapsed = start.elapsed().as_secs_f64();
                    let hashrate = if elapsed > 0.0 {
                        nonce as f64 / elapsed
                    } else {
                        0.0
                    };

                    // Submit result
                    let submit = format!(
                        "{},{:.0},{},{},DUCOID{}\n",
                        nonce, hashrate, MINER_BANNER, self.device_id, self.device_id
                    );
                    stream.get_mut().write_all(submit.as_bytes())?;
                    stream.get_mut().flush()?;

                    // Read response
                    let response = read_line(&mut stream)?;
                    let response = response.trim();

                    match response {
                        "GOOD" | "BLOCK" => {
                            *accepted += 1;
                            log::info!(
                                "Share accepted! nonce={} hashrate={:.0} H/s [acc={} rej={}]",
                                nonce, hashrate, accepted, rejected
                            );
                        }
                        _ => {
                            *rejected += 1;
                            log::warn!(
                                "Share rejected: {} [acc={} rej={}]",
                                response, accepted, rejected
                            );
                        }
                    }

                    found = true;
                    break;
                }
            }

            if !found {
                log::warn!("No nonce found in range 0..{}", max_nonce);
            }
        }
    }
}

fn fetch_pool() -> Result<(String, u16)> {
    log::info!("Fetching mining pool...");

    let config = HttpConfig {
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        ..Default::default()
    };
    let mut conn = EspHttpConnection::new(&config)?;

    use embedded_svc::http::client::Client;
    let mut client = Client::wrap(&mut conn);

    let request = client.get(POOL_URL)?;
    let mut response = request.submit()?;

    let status = response.status();
    if status != 200 {
        return Err(anyhow!("getPool returned HTTP {}", status));
    }

    let mut body = [0u8; 256];
    let mut total = 0;
    loop {
        let n = response.read(&mut body[total..])?;
        if n == 0 {
            break;
        }
        total += n;
        if total >= body.len() {
            break;
        }
    }

    let json = std::str::from_utf8(&body[..total])
        .context("getPool response is not valid UTF-8")?;

    // Simple JSON parsing — extract "ip" and "port" without a JSON library
    let ip = extract_json_string(json, "ip")
        .context("Missing 'ip' in getPool response")?;
    let port_str = extract_json_string(json, "port")
        .context("Missing 'port' in getPool response")?;
    let port: u16 = port_str.parse().context("Invalid port number")?;

    Ok((ip, port))
}

fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\"", key);
    let idx = json.find(&pattern)?;
    let rest = &json[idx + pattern.len()..];
    // Skip past colon and whitespace
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(':')?;
    let rest = rest.trim_start();

    if let Some(rest) = rest.strip_prefix('"') {
        // String value
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    } else {
        // Numeric value
        let end = rest.find(|c: char| c == ',' || c == '}' || c.is_whitespace())
            .unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
}

fn hex_to_bytes(hex: &str) -> Result<[u8; 20]> {
    if hex.len() != 40 {
        return Err(anyhow!("Expected 40 hex chars, got {}", hex.len()));
    }
    let mut bytes = [0u8; 20];
    for i in 0..20 {
        bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .context("Invalid hex character")?;
    }
    Ok(bytes)
}

fn read_line(reader: &mut BufReader<TcpStream>) -> Result<String> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.is_empty() {
        return Err(anyhow!("Server closed connection"));
    }
    Ok(line)
}
