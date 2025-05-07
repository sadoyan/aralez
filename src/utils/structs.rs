use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;

pub type UpstreamsDashMap = DashMap<String, DashMap<String, (Vec<(String, u16, bool)>, AtomicUsize)>>;
pub type Headers = DashMap<String, DashMap<String, Vec<(String, String)>>>;
pub type UpstreamsIdMap = DashMap<String, (String, u16, bool)>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceMapping {
    pub proxy: String,
    pub real: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Extraparams {
    pub stickysessions: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Consul {
    pub servers: Option<Vec<String>>,
    pub services: Option<Vec<ServiceMapping>>,
    pub token: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub provider: String,
    pub stickysessions: bool,
    pub upstreams: Option<HashMap<String, HostConfig>>,
    pub globals: Option<HashMap<String, Vec<String>>>,
    pub consul: Option<Consul>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HostConfig {
    pub paths: HashMap<String, PathConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PathConfig {
    pub ssl: bool,
    pub servers: Vec<String>,
    pub headers: Option<Vec<String>>,
}
pub struct Configuration {
    pub upstreams: UpstreamsDashMap,
    pub headers: Headers,
    pub consul: Option<Consul>,
    pub typecfg: String,
    pub extraparams: Extraparams,
    pub globals: Option<DashMap<String, Vec<String>>>,
}

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub hc_interval: u16,
    pub hc_method: String,
    pub upstreams_conf: String,
    pub log_level: String,
    pub config_address: String,
    pub proxy_address_http: String,
    pub master_key: String,
    pub proxy_address_tls: Option<String>,
    pub tls_certificate: Option<String>,
    pub tls_key_file: Option<String>,
}
