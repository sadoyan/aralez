use crate::tls::acme::lookup::lookup_wait;
use crate::tls::acme::types::{DnsBackendPlugin, DnsProvider};
use crate::utils::tools::Die;
use async_trait::async_trait;
use log::info;
use serde_json::json;
use std::env;

pub struct DigitalOceanProvider {
    token: String,
    client: reqwest::Client,
}

impl DigitalOceanProvider {
    pub fn new() -> Self {
        let token = env::var("DIGITALOCEAN_TOKEN").die("Environment variable DIGITALOCEAN_TOKEN is required");

        Self {
            token,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl DnsProvider for DigitalOceanProvider {
    async fn create_txt_record(&self, domain: &str, name: &str, value: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let relative_name = name.strip_suffix(&format!(".{}", domain)).unwrap_or(name);
        let url = format!("https://api.digitalocean.com/v2/domains/{}/records", domain);
        let res = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&json!({
                "type": "TXT",
                "name": relative_name,
                "data": value,
                "ttl": 30
            }))
            .send()
            .await?;

        if !res.status().is_success() {
            let err_text = res.text().await?;
            return Err(format!("DigitalOcean API error: {}", err_text).into());
        }

        let json: serde_json::Value = res.json().await?;
        let record_id = json["domain_record"]["id"].as_i64().ok_or("Missing record ID in DigitalOcean response")?.to_string();
        info!("Created TXT record name: {}, id: {}", name, record_id);
        lookup_wait(name, Some("1.1.1.1"), 60, value).await;
        Ok(record_id)
    }

    async fn delete_txt_record(&self, record_id: &str, domain: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("https://api.digitalocean.com/v2/domains/{}/records/{}", domain, record_id);
        let res = self.client.delete(&url).bearer_auth(&self.token).send().await?;
        if !res.status().is_success() {
            let err_text = res.text().await?;
            return Err(format!("Failed to delete DigitalOcean record: {}", err_text).into());
        }
        Ok(())
    }
}

inventory::submit! {
    DnsBackendPlugin {
        name: "digitalocean",
        factory: || Box::new(DigitalOceanProvider::new()),
    }
}
