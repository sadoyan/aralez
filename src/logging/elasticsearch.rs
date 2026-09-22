use crate::logging::core::StructuredSystemLog;
use crate::logging::types::WriteLog;
use std::env;
use std::sync::LazyLock;

pub struct ElasticSearchConfig {
    pub hosts: &'static str,
    pub user: &'static str,
    pub password: &'static str,
}

impl ElasticSearchConfig {
    pub fn load_from_env() -> Self {
        let host = env::var("LOG_ELASTIC_HOSTS").unwrap_or_else(|_| "127.0.0.1".to_string());
        let user = env::var("LOG_ELASTIC_USER").unwrap_or_else(|_| "elastic".to_string());
        let password = env::var("LOG_ELASTIC_PASSWORD").unwrap_or_else(|_| "elastic".to_string());

        Self {
            hosts: Box::leak(host.into_boxed_str()),
            user: Box::leak(user.into_boxed_str()),
            password: Box::leak(password.into_boxed_str()),
        }
    }
}
pub static ELASTIC: LazyLock<ElasticSearchConfig> = LazyLock::new(ElasticSearchConfig::load_from_env);
pub struct ElasticSearch;

impl WriteLog for ElasticSearch {
    fn writelog(&self, msg: &StructuredSystemLog) {
        if let Ok(jsonmsg) = serde_json::to_string(&msg) {
            println!("Connecting to {} - {}:{} => {}", ELASTIC.hosts, ELASTIC.user, ELASTIC.password, jsonmsg);
        }
    }
}
