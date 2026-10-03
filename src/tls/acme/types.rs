use crate::tls::acme::dns::cloudflare::CloudflareProvider;
use crate::tls::acme::dns::route53::Route53Provider;

#[async_trait::async_trait]
pub trait DnsProvider: Send + Sync {
    async fn create_txt_record(&self, domain: &str, name: &str, value: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;

    async fn delete_txt_record(&self, record_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

// pub enum ProviderType {
//     Cloudflare,
//     Route53,
// }

pub fn get_provider() -> Box<dyn DnsProvider> {
    let provider_choice: Option<Box<dyn DnsProvider>> = match std::env::var("DNS_PROVIDER").as_deref() {
        Ok("cloudflare") => Some(Box::new(CloudflareProvider::new())),
        Ok("route53") => Some(Box::new(Route53Provider::new())),
        _ => None,
    };
    provider_choice.expect("No provider choice provided!")
}
