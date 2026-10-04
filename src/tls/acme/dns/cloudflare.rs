use crate::tls::acme::lookup::lookup_wait;
use crate::tls::acme::types::{DnsBackendPlugin, DnsProvider};
use log::info;
use reqwest::Client;
use serde_json::json;

pub struct CloudflareProvider {
    pub client: Client,
    pub api_token: String,
    pub zone_id: String,
}

impl CloudflareProvider {
    pub fn new() -> Self {
        let api_token = std::env::var("CLOUDFLARE_API_TOKEN").expect("Environment variable CLOUDFLARE_API_TOKEN is required");
        let zone_id = std::env::var("CLOUDFLARE_ZONE_ID").expect("Environment variable CLOUDFLARE_ZONE_ID is required");

        Self {
            client: Client::new(),
            api_token,
            zone_id,
        }
    }
}

#[async_trait::async_trait]
impl DnsProvider for CloudflareProvider {
    async fn create_txt_record(&self, _domain: &str, name: &str, value: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("https://api.cloudflare.com/client/v4/zones/{}/dns_records", self.zone_id);

        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.api_token)
            .json(&json!({
                "type": "TXT",
                "name": name,
                "content": value,
                "ttl": 120,
            }))
            .send()
            .await?;

        let res_json: serde_json::Value = response.json().await?;
        if !res_json["success"].as_bool().unwrap_or(false) {
            return Err(format!("Cloudflare API error: {:?}", res_json["errors"]).into());
        }

        let record_id = res_json["result"]["id"].as_str().ok_or("missing id")?.to_string();
        info!("Created TXT record name: {}, id: {}", name, record_id);
        lookup_wait(name, Some("1.1.1.1"), 60, value).await;
        Ok(record_id)
    }

    async fn delete_txt_record(&self, record_id: &str, record_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("https://api.cloudflare.com/client/v4/zones/{}/dns_records/{}", self.zone_id, record_id);

        let response = self.client.delete(&url).bearer_auth(&self.api_token).send().await?;

        let res_json: serde_json::Value = response.json().await?;
        if !res_json["success"].as_bool().unwrap_or(false) {
            return Err(format!("Cloudflare API error deleting record: {:?}", res_json["errors"]).into());
        }
        info!("Deleted TXT record name: {}, id: {}", record_name, record_id);
        Ok(())
    }
}

inventory::submit! {
    DnsBackendPlugin {
        name: "cloudflare",
        factory: || Box::new(CloudflareProvider::new()),
    }
}
