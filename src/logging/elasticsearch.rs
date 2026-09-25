use crate::logging::core::StructuredSystemLog;
use crate::logging::types::{LogBackendPlugin, WriteLog};
use crate::utils::tools::get_hostname;
use async_trait::async_trait;
use bytes::{BufMut, BytesMut};
use elasticsearch::auth::Credentials;
use elasticsearch::http::transport::Transport;
use elasticsearch::{BulkParts, Elasticsearch};
use humantime::format_rfc3339;
use serde_json::json;
use std::env;
use std::sync::LazyLock;
use std::time::Duration;
use std::time::SystemTime;
use tokio::sync::mpsc;
use tokio::sync::OnceCell;

static CLIENT: OnceCell<Elasticsearch> = OnceCell::const_new();
#[derive(Debug)]
struct ElasticHost {
    url: &'static str,
}

struct ElasticSearchConfig {
    pub hosts: &'static [ElasticHost],
    pub user: &'static str,
    pub password: &'static str,
    pub hostname: &'static str,
    pub index_name: &'static str,
    pub flush_duration: u64,
    pub batch_len: usize,
    pub buffer_size: usize,
}

static ELASTIC: LazyLock<ElasticSearchConfig> = LazyLock::new(ElasticSearchConfig::load_from_env);
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
        let index_name = env::var("LOG_ELASTIC_INDEX").unwrap_or_else(|_| "aralez-logs".to_string());
        let s_flush_duration = env::var("LOG_ELASTIC_FLUSH_DURATION").unwrap_or_else(|_| "2".to_string());
        let flush_duration = s_flush_duration.parse::<u64>().unwrap_or(2);
        let s_batch_len = env::var("LOG_ELASTIC_BATCH_LEN").unwrap_or_else(|_| "200".to_string());
        let batch_len = s_batch_len.parse::<usize>().unwrap_or(200);
        let buffer_size: usize = 100 * 512;

        Self {
            hosts: Box::leak(hosts_struct.into_boxed_slice()),
            user: Box::leak(user.into_boxed_str()),
            password: Box::leak(password.into_boxed_str()),
            flush_duration,
            batch_len,
            buffer_size,
            hostname: Box::leak(get_hostname().into_boxed_str()),
            index_name: Box::leak(index_name.into_boxed_str()),
        }
    }
}

static LOG_CHANNEL: LazyLock<mpsc::UnboundedSender<StructuredSystemLog>> = LazyLock::new(|| {
    let (tx, mut rx) = mpsc::unbounded_channel::<StructuredSystemLog>();
    tokio::spawn(async move {
        let mut batch = Vec::with_capacity(ELASTIC.batch_len);
        let mut timer = tokio::time::interval(Duration::from_secs(ELASTIC.flush_duration));
        let mut buffer = BytesMut::with_capacity(ELASTIC.buffer_size);
        loop {
            tokio::select! {
                Some(msg) = rx.recv() => {
                    batch.push(msg);
                    if batch.len() >= ELASTIC.batch_len {
                        flush_to_es(&mut batch, &mut buffer, ELASTIC.hostname).await;
                    }
                }
                _ = timer.tick() => {
                    if !batch.is_empty() {
                        flush_to_es(&mut batch, &mut buffer, ELASTIC.hostname).await;
                    }
                }
            }
        }
    });

    tx
});

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

#[async_trait]
impl WriteLog for ElasticSearch {
    async fn writelog(&self, msg: &StructuredSystemLog) {
        let _ = LOG_CHANNEL.send(msg.clone());
    }
}

async fn flush_to_es(batch: &mut Vec<StructuredSystemLog>, buffer: &mut BytesMut, hostname: &str) {
    buffer.clear();

    for msg in batch.drain(..) {
        let index_header = format!("{{\"index\":{{\"_index\":\"{}\"}}}}\n", ELASTIC.index_name).into_bytes();
        buffer.extend_from_slice(&index_header);
        // buffer.extend_from_slice(b"{\"index\":{\"_index\":\"logs\"}}\n");
        let logstash_payload = json!({
            "@timestamp": format_rfc3339(SystemTime::now()).to_string(),
            "@version": "1",
            "log": {
                "level": msg.level,
                "logger": msg.target,
            },
            "message": msg.message,
            "host": {
                "hostname": hostname,
            }
        });

        if serde_json::to_writer((buffer).writer(), &logstash_payload).is_ok() {
            buffer.extend_from_slice(b"\n");
        }
    }
    if buffer.is_empty() {
        return;
    }
    let payload = buffer.split().freeze();
    let client = CLIENT.get_or_init(make_es_pool).await;

    let _ = client.bulk(BulkParts::None).body(vec![payload]).send().await;
}

inventory::submit! {
    LogBackendPlugin {
        name: "elasticsearch",
        factory: || Box::new(ElasticSearch),
    }
}
