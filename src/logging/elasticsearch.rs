use crate::logging::core::StructuredSystemLog;
use crate::logging::types::{LogBackendPlugin, WriteLog};
use async_trait::async_trait;
use elasticsearch::auth::Credentials;
use elasticsearch::http::transport::Transport;
use elasticsearch::{Elasticsearch, IndexParts};
use std::env;
use std::sync::LazyLock;
use std::time::Duration;
use tokio::sync::OnceCell;

static ELASTIC: LazyLock<ElasticSearchConfig> = LazyLock::new(ElasticSearchConfig::load_from_env);
static CLIENT: OnceCell<Elasticsearch> = OnceCell::const_new();

#[derive(Debug)]
struct ElasticHost {
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

            let full_url = format!("{}://{}:{}", scheme, address, port);

            hosts_struct.push(ElasticHost {
                url: Box::leak(full_url.into_boxed_str()),
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

async fn make_es_pool() -> Elasticsearch {
    let valid_urls: Vec<&str> = ELASTIC.hosts.iter().map(|target| target.url).collect();
    if valid_urls.is_empty() {
        log::error!("No valid Elasticsearch host URLs found in configuration");
        return Elasticsearch::default();
    }
    let transport = match Transport::sniffing_node_list(valid_urls, Duration::from_secs(180)) {
        Ok(t) => t,
        Err(e) => {
            log::error!("Failed to create sniffing transport: {}", e);
            return Elasticsearch::default();
        }
    };
    let credentials = Credentials::Basic(ELASTIC.user.to_string(), ELASTIC.password.to_string());
    transport.set_auth(credentials);
    let client = Elasticsearch::new(transport);
    if let Ok(res) = client.ping().send().await {
        if res.status_code().is_success() {
            log::info!("Elasticsearch sniffing pool initialized successfully");
            return client;
        }
    }
    log::warn!("Elasticsearch pool initialized, but clustser ping returned non-200");
    client
}
struct ElasticSearch;
/*
#[derive(Debug, Serialize)]
pub struct StructuredSystemLog {
    pub target: &'static str,
    pub level: Level,
    pub message: String,
}
*/
#[async_trait]
impl WriteLog for ElasticSearch {
    async fn writelog(&self, msg: &StructuredSystemLog) {
        let index_name = "logs";
        let response = CLIENT.get_or_init(make_es_pool).await.index(IndexParts::Index(index_name)).body(msg).send().await;
        println!("{:?}", response);
    }
}

inventory::submit! {
    LogBackendPlugin {
        name: "elasticsearch",
        factory: || Box::new(ElasticSearch),
    }
}
