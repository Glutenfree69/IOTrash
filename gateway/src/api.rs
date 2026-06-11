use anyhow::Result;
use std::time::Duration;

use embedded_svc::http::client::Client as HttpClient;
use embedded_svc::io::Write;
use esp_idf_svc::http::client::{Configuration, EspHttpConnection};

use crate::packet::SensorPacket;

const DEFAULT_API_URL: &str = "https://api.sensor.community/v1/push-sensor-data/";

// RELIABILITY: without an explicit timeout, a connection that never gets a
// response (dead AP, unreachable test server, ...) blocks this call forever
// and freezes the whole main loop. 10s is generous for a small JSON POST but
// still bounded.
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

pub struct SensorApi {
    chip_id: String,
    base_url: String,
}

impl SensorApi {
    pub fn new(chip_id: String) -> Self {
        let base_url = option_env!("API_URL")
            .unwrap_or(DEFAULT_API_URL)
            .to_string();
        log::info!("SensorApi endpoint: {}", base_url);
        Self { chip_id, base_url }
    }

    pub fn post_sensor_data(&self, packet: &SensorPacket) -> Result<()> {
        // POST 1: dust data (X-Pin: 1, SDS011)
        let dust_body = format!(
            r#"{{"software_version":"sensor-sensei-0.1","sensordatavalues":[{{"value_type":"P1","value":"{:.1}"}},{{"value_type":"P2","value":"{:.1}"}}]}}"#,
            packet.pm10(),
            packet.pm25(),
        );
        self.post_with_pin("1", &dust_body)?;

        // POST 2: climate data (X-Pin: 11, DHT22)
        let climate_body = format!(
            r#"{{"software_version":"sensor-sensei-0.1","sensordatavalues":[{{"value_type":"temperature","value":"{:.1}"}},{{"value_type":"humidity","value":"{:.1}"}}]}}"#,
            packet.temperature(),
            packet.humidity(),
        );
        self.post_with_pin("11", &climate_body)?;

        Ok(())
    }

    fn post_with_pin(&self, pin: &str, body: &str) -> Result<()> {
        let use_https = self.base_url.starts_with("https");
        // SECURITY: warn loudly if sensor data is sent unencrypted. This is
        // expected for the local-test override (`API_URL=http://...`,
        // documented in gateway/.env.example), but should never happen with
        // the default sensor.community endpoint, which is HTTPS-only.
        if !use_https {
            log::warn!(
                "Sending sensor data over plaintext HTTP to {} — \
                 only use this for local testing, never in production",
                self.base_url
            );
        }
        let config = Configuration {
            crt_bundle_attach: if use_https {
                Some(esp_idf_svc::sys::esp_crt_bundle_attach)
            } else {
                None
            },
            timeout: Some(HTTP_TIMEOUT),
            ..Default::default()
        };

        let mut client = HttpClient::wrap(EspHttpConnection::new(&config)?);
        let payload = body.as_bytes();
        let content_length = payload.len().to_string();

        let headers = [
            ("Content-Type", "application/json"),
            ("X-Pin", pin),
            ("X-Sensor", &self.chip_id),
            ("Content-Length", &content_length),
        ];

        let mut request = client.post(&self.base_url, &headers)?;
        request.write_all(payload)?;
        request.flush()?;
        let response = request.submit()?;
        let status = response.status();

        if status == 201 {
            log::info!("POST X-Pin:{} -> {} Created", pin, status);
        } else {
            log::warn!("POST X-Pin:{} -> {} (expected 201)", pin, status);
        }

        Ok(())
    }
}
