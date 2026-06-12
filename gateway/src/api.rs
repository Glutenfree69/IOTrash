use anyhow::Result;
use embedded_svc::http::client::Client as HttpClient;
use embedded_svc::io::Write;
use esp_idf_svc::http::client::{Configuration, EspHttpConnection};

use crate::packet::SensorPacket;

const DEFAULT_API_URL: &str = "https://api.sensor.community/v1/push-sensor-data/";

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

        // POST 2: climate data (X-Pin: 7, DHT22). Careful: pin 11 is the
        // BME280/BMP280 slot in the sensor.community convention — pushing a
        // DHT22-configured device on pin 11 gets a 400.
        let climate_body = format!(
            r#"{{"software_version":"sensor-sensei-0.1","sensordatavalues":[{{"value_type":"temperature","value":"{:.1}"}},{{"value_type":"humidity","value":"{:.1}"}}]}}"#,
            packet.temperature(),
            packet.humidity(),
        );
        self.post_with_pin("7", &climate_body)?;

        Ok(())
    }

    fn post_with_pin(&self, pin: &str, body: &str) -> Result<()> {
        let use_https = self.base_url.starts_with("https");
        let config = Configuration {
            crt_bundle_attach: if use_https {
                Some(esp_idf_svc::sys::esp_crt_bundle_attach)
            } else {
                None
            },
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
