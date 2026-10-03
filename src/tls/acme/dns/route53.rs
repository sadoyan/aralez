use crate::tls::acme::types::{DnsBackendPlugin, DnsProvider};

pub struct Route53Provider {
    pub hosted_zone_id: String,
}

impl Route53Provider {
    pub fn new() -> Self {
        Self {
            hosted_zone_id: std::env::var("AWS_HOSTED_ZONE_ID").unwrap_or_default(),
        }
    }
}

// Not working, just a placeholder.
#[async_trait::async_trait]
impl DnsProvider for Route53Provider {
    async fn create_txt_record(&self, _domain: &str, _name: &str, _value: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        println!(" ====> Route53 Created TXT Record for {}: {}: {}", _name, _domain, _value);
        Ok("route53_change_id".to_string())
    }
    async fn delete_txt_record(&self, _record_id: &str, _record_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!(" ====> Route53 Deleted TXT Record for {}: {}", _record_name, _record_id);
        Ok(())
    }
}

inventory::submit! {
    DnsBackendPlugin {
        name: "route53",
        factory: || Box::new(Route53Provider::new()),
    }
}
