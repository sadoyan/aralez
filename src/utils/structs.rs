use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;

pub type InnerMap = (String, u16, bool, bool, bool);
pub type UpstreamsDashMap = DashMap<String, DashMap<String, (Vec<InnerMap>, AtomicUsize)>>;
pub type UpstreamsIdMap = DashMap<String, InnerMap>;
pub type Headers = DashMap<String, DashMap<String, Vec<(String, String)>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceMapping {
    pub proxy: String,
    pub real: String,
}

#[derive(Clone, Debug)]
pub struct Extraparams {
    pub sticky_sessions: bool,
    pub to_ssl: Option<bool>,
    pub authentication: DashMap<String, Vec<String>>,
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
    pub sticky_sessions: bool,
    pub to_ssl: Option<bool>,
    pub upstreams: Option<HashMap<String, HostConfig>>,
    pub globals: Option<HashMap<String, Vec<String>>>,
    pub headers: Option<Vec<String>>,
    pub authorization: Option<HashMap<String, String>>,
    pub consul: Option<Consul>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HostConfig {
    pub paths: HashMap<String, PathConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PathConfig {
    pub servers: Vec<String>,
    pub to_https: Option<bool>,
    pub headers: Option<Vec<String>>,
}
#[derive(Debug)]
pub struct Configuration {
    pub upstreams: UpstreamsDashMap,
    pub headers: Headers,
    pub consul: Option<Consul>,
    pub typecfg: String,
    pub extraparams: Extraparams,
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
    pub proxy_port_tls: Option<u16>,
    pub tls_certificate: Option<String>,
    pub tls_key_file: Option<String>,
    pub local_server: Option<(String, u16)>,
}
