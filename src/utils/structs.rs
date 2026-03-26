use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

pub type UpstreamsDashMap = DashMap<Arc<str>, DashMap<Arc<str>, (Vec<Arc<InnerMap>>, AtomicUsize)>>;

pub type UpstreamsIdMap = DashMap<String, Arc<InnerMap>>;
pub type Headers = DashMap<Arc<str>, DashMap<Arc<str>, Vec<(Arc<str>, Arc<str>)>>>;

#[derive(Clone, Debug, Default)]
pub struct Extraparams {
    pub to_https: Option<bool>,
    pub sticky_sessions: bool,
    pub authentication: Option<Arc<InnerAuth>>,
    pub rate_limit: Option<isize>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct GlobalServiceMapping {
    pub upstream: String,
    pub hostname: String,
    pub path: Option<String>,
    pub to_https: Option<bool>,
    pub sticky_sessions: Option<bool>,
    pub rate_limit: Option<isize>,
    pub client_headers: Option<Vec<String>>,
    pub server_headers: Option<Vec<String>>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Kubernetes {
    pub servers: Option<Vec<String>>,
    pub services: Option<Vec<GlobalServiceMapping>>,
    pub tokenpath: Option<String>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Consul {
    pub servers: Option<Vec<String>>,
    pub services: Option<Vec<GlobalServiceMapping>>,
    pub token: Option<String>,
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub provider: String,
    pub to_https: Option<bool>,
    pub sticky_sessions: bool,
    #[serde(default)]
    pub upstreams: Option<HashMap<String, HostConfig>>,
    #[serde(default)]
    pub globals: Option<HashMap<String, Vec<String>>>,
    #[serde(default)]
    pub client_headers: Option<Vec<String>>,
    #[serde(default)]
    pub server_headers: Option<Vec<String>>,
    #[serde(default)]
    pub authorization: Option<Auth>,
    #[serde(default)]
    pub consul: Option<Consul>,
    #[serde(default)]
    pub kubernetes: Option<Kubernetes>,
    #[serde(default)]
    pub rate_limit: Option<isize>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HostConfig {
    pub paths: HashMap<String, PathConfig>,
    pub rate_limit: Option<isize>,
}
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct Auth {
    #[serde(rename = "type")]
    pub auth_type: String,
    #[serde(rename = "creds")]
    pub auth_cred: String,
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PathConfig {
    pub servers: Vec<String>,
    pub to_https: Option<bool>,
    pub sticky_sessions: Option<bool>,
    pub client_headers: Option<Vec<String>>,
    pub server_headers: Option<Vec<String>>,
    pub rate_limit: Option<isize>,
    pub healthcheck: Option<bool>,
    pub redirect_to: Option<String>,
    pub authorization: Option<Auth>,
}
#[derive(Debug, Default)]
pub struct Configuration {
    pub upstreams: UpstreamsDashMap,
    pub client_headers: Headers,
    pub server_headers: Headers,
    pub consul: Option<Consul>,
    pub kubernetes: Option<Kubernetes>,
    pub typecfg: String,
    pub extraparams: Extraparams,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    pub hc_interval: u16,
    pub hc_method: String,
    pub upstreams_conf: String,
    pub log_level: String,
    pub master_key: String,
    pub config_address: String,
    pub proxy_address_http: String,
    pub config_api_enabled: bool,
    pub config_tls_address: Option<String>,
    pub config_tls_certificate: Option<String>,
    pub config_tls_key_file: Option<String>,
    pub proxy_address_tls: Option<String>,
    pub proxy_port_tls: Option<String>,
    pub proxy_port: Option<String>,
    pub local_server: Option<(String, u16)>,
    pub proxy_certificates: Option<String>,
    pub proxy_tls_grade: Option<String>,
    pub file_server_address: Option<String>,
    pub file_server_folder: Option<String>,
    pub runuser: Option<String>,
    pub rungroup: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InnerAuth {
    pub auth_type: Arc<str>,
    pub auth_cred: Arc<str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InnerMap {
    pub address: Arc<str>,
    pub port: u16,
    pub is_ssl: bool,
    pub is_http2: bool,
    pub to_https: bool,
    pub rate_limit: Option<isize>,
    pub healthcheck: Option<bool>,
    pub redirect_to: Option<Arc<str>>,
    pub authorization: Option<Arc<InnerAuth>>,
}

#[allow(dead_code)]
impl InnerMap {
    pub fn new() -> Self {
        Self {
            address: Arc::from("127.0.0.1"),
            port: Default::default(),
            is_ssl: Default::default(),
            is_http2: Default::default(),
            to_https: Default::default(),
            rate_limit: Default::default(),
            healthcheck: Default::default(),
            redirect_to: Default::default(),
            authorization: Default::default(),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct InnerMapForJson {
    pub address: String,
    pub port: u16,
    pub is_ssl: bool,
    pub is_http2: bool,
    pub to_https: bool,
    pub rate_limit: Option<isize>,
    pub healthcheck: Option<bool>,
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct UpstreamSnapshotForJson {
    pub backends: Vec<InnerMapForJson>,
    pub requests: usize,
}
