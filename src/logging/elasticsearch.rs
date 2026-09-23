use crate::logging::core::StructuredSystemLog;
use crate::logging::types::{LogBackendPlugin, WriteLog};
use crate::utils::hcclient::httpclient;
use async_trait::async_trait;
use bytes::Bytes;
use std::env;
use std::sync::LazyLock;

pub struct ElasticSearchConfig {
    pub hosts: &'static [&'static str],
    pub user: &'static str,
    pub password: &'static str,
}

impl ElasticSearchConfig {
    pub fn load_from_env() -> Self {
        let hosts: Vec<&'static str> = env::var("LOG_ELASTIC_HOSTS")
            .unwrap_or_else(|_| "127.0.0.1".to_string())
            .split(',')
            .map(|s| -> &'static str { Box::leak(s.trim().to_string().into_boxed_str()) })
            .collect();

        let user = env::var("LOG_ELASTIC_USER").unwrap_or_else(|_| "elastic".to_string());
        let password = env::var("LOG_ELASTIC_PASSWORD").unwrap_or_else(|_| "elastic".to_string());

        Self {
            hosts: Box::leak(hosts.into_boxed_slice()),
            user: Box::leak(user.into_boxed_str()),
            password: Box::leak(password.into_boxed_str()),
        }
    }
}
pub static ELASTIC: LazyLock<ElasticSearchConfig> = LazyLock::new(ElasticSearchConfig::load_from_env);
pub struct ElasticSearch;

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
        println!("{} => {} => {}:{:?}", ELASTIC.hosts[0], ELASTIC.hosts[1], ELASTIC.user, ELASTIC.password);
        let _ = httpclient("POST", false, "127.0.0.1", "/i", "localhost", 8000, "http://127.0.0.1:8000/d".to_string(), payload).await;
    }
}

inventory::submit! {
    LogBackendPlugin {
        name: "elasticsearch",
        factory: || Box::new(ElasticSearch),
    }
}
