use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;

pub type UpstreamsDashMap = DashMap<String, DashMap<String, (Vec<InnerMap>, AtomicUsize)>>;

pub type UpstreamsIdMap = DashMap<String, InnerMap>;
pub type Headers = DashMap<String, DashMap<String, Vec<(String, String)>>>;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ServiceMapping {
    pub upstream: String,
    pub hostname: String,
    pub path: Option<String>,
    pub to_https: Option<bool>,
    pub rate_limit: Option<isize>,
    pub client_headers: Option<Vec<String>>,
    pub server_headers: Option<Vec<String>>,
}

// pub type Services = DashMap<String, Vec<(String, Option<String>)>>;

#[derive(Clone, Debug, Default)]
pub struct Extraparams {
    pub sticky_sessions: bool,
    pub to_https: Option<bool>,
    pub authentication: DashMap<String, Vec<String>>,
    pub rate_limit: Option<isize>,
}
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Kubernetes {
    pub servers: Option<Vec<String>>,
    pub services: Option<Vec<ServiceMapping>>,
    pub tokenpath: Option<String>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Consul {
    pub servers: Option<Vec<String>>,
    pub services: Option<Vec<ServiceMapping>>,
    pub token: Option<String>,
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub provider: String,
    pub sticky_sessions: bool,
    pub to_https: Option<bool>,
    #[serde(default)]
    pub upstreams: Option<HashMap<String, HostConfig>>,
    #[serde(default)]
    pub globals: Option<HashMap<String, Vec<String>>>,
    #[serde(default)]
    pub client_headers: Option<Vec<String>>,
    #[serde(default)]
    pub server_headers: Option<Vec<String>>,
    #[serde(default)]
    pub authorization: Option<HashMap<String, String>>,
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

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PathConfig {
    pub servers: Vec<String>,
    pub to_https: Option<bool>,
    pub client_headers: Option<Vec<String>>,
    pub server_headers: Option<Vec<String>>,
    pub rate_limit: Option<isize>,
    pub healthcheck: Option<bool>,
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
    pub proxy_port_tls: Option<u16>,
    pub local_server: Option<(String, u16)>,
    pub proxy_certificates: Option<String>,
    pub proxy_tls_grade: Option<String>,
    pub file_server_address: Option<String>,
    pub file_server_folder: Option<String>,
    pub runuser: Option<String>,
    pub rungroup: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InnerMap {
    pub address: String,
    pub port: u16,
    pub is_ssl: bool,
    pub is_http2: bool,
    pub to_https: bool,
    pub rate_limit: Option<isize>,
    pub healthcheck: Option<bool>,
}

#[allow(dead_code)]
impl InnerMap {
    pub fn new() -> Self {
        Self {
            address: Default::default(),
            port: Default::default(),
            is_ssl: Default::default(),
            is_http2: Default::default(),
            to_https: Default::default(),
            rate_limit: Default::default(),
            healthcheck: Default::default(),
        }
    }
}
