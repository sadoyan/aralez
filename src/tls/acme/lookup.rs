use hickory_resolver::TokioResolver;
use hickory_resolver::config::{NameServerConfig, ResolverConfig, ResolverOpts};
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use hickory_resolver::proto::rr::RData;
use log::info;
use std::net::IpAddr;
use std::str::FromStr;
use std::time::Duration;

pub async fn lookup_wait(record_name: &str, dns_server_ip: &str, max_wait_secs: u64) -> bool {
    let ip = IpAddr::from_str(dns_server_ip).unwrap_or_else(|_| IpAddr::from([1, 1, 1, 1]));
    let name_server = NameServerConfig::udp(ip);
    let config = ResolverConfig::from_name_servers(vec![name_server]);
    let mut opts = ResolverOpts::default();
    opts.cache_size = 0;

    let resolver = TokioResolver::builder_with_config(config, TokioRuntimeProvider::default())
        .with_options(opts)
        .build()
        .expect("failed to build DNS resolver");

    let start = std::time::Instant::now();
    let poll_interval = Duration::from_secs(3);
    while start.elapsed().as_secs() < max_wait_secs {
        match resolver.txt_lookup(record_name).await {
            Ok(txt_lookup) => {
                for record in txt_lookup.answers() {
                    if let RData::TXT(txt) = &record.data {
                        for data in &txt.txt_data {
                            if let Ok(txt_str) = std::str::from_utf8(data) {
                                info!("DNS record propagated successfully for {} : {}", record_name, txt_str);
                                return true;
                            }
                        }
                    }
                }
            }
            Err(_) => {
                info!("Waiting for TXT record {}", record_name);
            }
        }

        tokio::time::sleep(poll_interval).await;
    }

    false
}
