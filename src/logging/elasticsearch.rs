use crate::logging::core::StructuredSystemLog;
use crate::logging::types::{LogBackendPlugin, WriteLog};
use crate::utils::hcclient::httpclient;
use async_trait::async_trait;
use bytes::Bytes;
use std::env;
use std::sync::LazyLock;

static ELASTIC: LazyLock<ElasticSearchConfig> = LazyLock::new(ElasticSearchConfig::load_from_env);

#[derive(Debug)]
struct ElasticHost {
    address: &'static str,
    port: u16,
    tls: bool,
    url: &'static str,
}

struct ElasticSearchConfig {
    pub hosts: &'static [ElasticHost],
    pub user: &'static str,
    pub password: &'static str,
}

impl ElasticSearchConfig {
    pub fn load_from_env() -> Self {
        let raw_hosts = env::var("LOG_ELASTIC_HOSTS").unwrap_or_else(|_| "http://127.0.0.1:9200".to_string());

        let mut hosts_struct = Vec::new();

        for entry in raw_hosts.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }

            let (scheme, rest) = if let Some(stripped) = entry.strip_prefix("https://") {
                ("https", stripped)
            } else if let Some(stripped) = entry.strip_prefix("http://") {
                ("http", stripped)
            } else {
                ("http", entry)
            };

            let (address, port) = match rest.split_once(':') {
                Some((host_part, port_part)) => {
                    let port = port_part.parse::<u16>().unwrap_or(9200);
                    (host_part, port)
                }
                None => (rest, 9200),
            };

            let tls = scheme == "https";
            let full_url = format!("{}://{}:{}", scheme, address, port);

            hosts_struct.push(ElasticHost {
                address: Box::leak(address.to_string().into_boxed_str()),
                port,
                tls,
                url: Box::leak(full_url.into_boxed_str()), // Clean static URL
            });
        }

        let user = env::var("LOG_ELASTIC_USER").unwrap_or_else(|_| "elastic".to_string());
        let password = env::var("LOG_ELASTIC_PASSWORD").unwrap_or_else(|_| "elastic".to_string());

        Self {
            hosts: Box::leak(hosts_struct.into_boxed_slice()),
            user: Box::leak(user.into_boxed_str()),
            password: Box::leak(password.into_boxed_str()),
        }
    }
}

struct ElasticSearch;

#[async_trait]
impl WriteLog for ElasticSearch {
    async fn writelog(&self, msg: &StructuredSystemLog) {
        let payload = match serde_json::to_vec(&msg) {
            Ok(vec) => Bytes::from(vec),
            Err(e) => {
                log::warn!("Failed to serialize structured system log to JSON bytes: {}", e);
                return;
            }
        };

        if let Some(target) = ELASTIC.hosts.first() {
            let _ = httpclient("POST", target.tls, target.address, "/i", target.address, target.port, target.url, payload).await;
        }
    }
}

inventory::submit! {
    LogBackendPlugin {
        name: "elasticsearch",
        factory: || Box::new(ElasticSearch),
    }
}
