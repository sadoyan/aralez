// ------------------------ //
// This file is vibecoded !
// ------------------------ //

use crate::tls::acme::lookup::lookup_wait;
use crate::tls::acme::types::{DnsBackendPlugin, DnsProvider};
use aws_config::SdkConfig;
use aws_sdk_route53::Client;
use aws_sdk_route53::types::{Change, ChangeAction, ChangeBatch, ResourceRecord, ResourceRecordSet, RrType};
use log::info;
use tokio::runtime::Handle;

pub struct Route53Provider {
    pub client: Client,
    pub hosted_zone_id: String,
}

impl Route53Provider {
    pub fn new() -> Self {
        let hosted_zone_id = std::env::var("AWS_HOSTED_ZONE_ID").expect("Environment variable AWS_HOSTED_ZONE_ID is required");
        // Load AWS SDK config synchronously using Tokio runtime handle to fit factory construction
        let config: SdkConfig = if let Ok(handle) = Handle::try_current() {
            tokio::task::block_in_place(|| handle.block_on(aws_config::load_from_env()))
        } else {
            tokio::runtime::Runtime::new().unwrap().block_on(aws_config::load_from_env())
        };
        let client = Client::new(&config);
        Self { client, hosted_zone_id }
    }

    pub async fn get_authoritative_ns_ip(&self) -> Option<String> {
        // Query Route53 for the zone's details (includes assigned Name Servers)
        let response = self.client.get_hosted_zone().id(&self.hosted_zone_id).send().await.ok()?;

        // Pick the first name server assigned to this zone (e.g., "ns-1289.awsdns-29.org")
        let ns_domain = response.delegation_set()?.name_servers().first()?;

        // Resolve that nameserver's domain name to an IP address
        let ips = tokio::net::lookup_host(format!("{}:53", ns_domain)).await.ok()?;

        let ip = ips.into_iter().next()?.ip().to_string();
        info!("Targeting Route53 authoritative NS {} ({}) for polling", ns_domain, ip);
        Some(ip)
    }
}

#[async_trait::async_trait]
impl DnsProvider for Route53Provider {
    async fn create_txt_record(&self, _domain: &str, name: &str, value: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // AWS Route53 requires TXT strings to be wrapped in double quotes
        let formatted_value = format!("\"{}\"", value);

        let change = Change::builder()
            .action(ChangeAction::Upsert)
            .resource_record_set(
                ResourceRecordSet::builder()
                    .name(name)
                    .r#type(RrType::Txt)
                    .ttl(60)
                    .resource_records(ResourceRecord::builder().value(formatted_value).build()?)
                    .build()?,
            )
            .build()?;

        let change_batch = ChangeBatch::builder().changes(change).build()?;

        let response = self
            .client
            .change_resource_record_sets()
            .hosted_zone_id(&self.hosted_zone_id)
            .change_batch(change_batch)
            .send()
            .await?;

        let record_id = response.change_info().ok_or("Missing ChangeInfo in Route53 response")?.id().to_string();
        info!("Created Route53 TXT record name: {}, change_id: {}", name, record_id);
        let target_dns_ip = self.get_authoritative_ns_ip().await;
        lookup_wait(name, target_dns_ip.as_deref(), 60, value).await;
        Ok(record_id)
    }

    async fn delete_txt_record(&self, _record_id: &str, record_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Since Route53 requires record value context to delete, we look up or pass existing TXT record values.
        // For ACME cleanup, issuing a DELETE with wildcards/upsert strategy or direct deletion:
        let list_res = self
            .client
            .list_resource_record_sets()
            .hosted_zone_id(&self.hosted_zone_id)
            .start_record_name(record_name)
            .start_record_type(RrType::Txt)
            .max_items(1)
            .send()
            .await?;

        if let Some(record_set) = list_res.resource_record_sets().first() {
            if record_set.name().trim_end_matches('.') == record_name.trim_end_matches('.') {
                let change = Change::builder().action(ChangeAction::Delete).resource_record_set(record_set.clone()).build()?;

                let change_batch = ChangeBatch::builder().changes(change).build()?;

                self.client
                    .change_resource_record_sets()
                    .hosted_zone_id(&self.hosted_zone_id)
                    .change_batch(change_batch)
                    .send()
                    .await?;

                info!("Deleted Route53 TXT record name: {}", record_name);
                return Ok(());
            }
        }

        info!("Route53 TXT record name: {} already deleted or not found", record_name);
        Ok(())
    }
}

inventory::submit! {
    DnsBackendPlugin {
        name: "route53",
        factory: || Box::new(Route53Provider::new()),
    }
}
