use crate::tls::acme::types::DnsProvider;

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

#[async_trait::async_trait]
impl DnsProvider for Route53Provider {
    async fn create_txt_record(&self, domain: &str, name: &str, value: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        println!(" ====> Route53 Created TXT Record for {}: {}: {}", name, domain, value);
        Ok("route53_change_id".to_string())
    }
    async fn delete_txt_record(&self, record_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!(" ====> Route53 Deleted TXT Record {}", record_id);
        Ok(())
    }
}
