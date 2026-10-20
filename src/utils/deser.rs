use crate::utils::hcclient::{HttpClientRequest, httpclient};
use crate::utils::lazylock::{GLOBAL_PARAMS, MASTER_KEY, WORKERS};
use crate::utils::tools::Die;
use crate::utils::types::{Consul, Extraparams, Headers, Kubernetes, UpstreamsDashMap};
use bytes::Bytes;
use log::{error, info, warn};
use noyalib::to_string;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::Deref;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UpstreamConfigDto {
    pub paths: HashMap<String, PathConfigDto>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PathConfigDto {
    pub is_http2: bool,
    pub healthcheck: bool,
    pub is_ssl: bool,
    pub to_https: bool,
    pub rate_limit: isize,
    pub x4xx_limit: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization: Option<HashMap<String, String>>,
    pub servers: Vec<String>,
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub client_headers: Option<Vec<String>>,
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub server_headers: Option<Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct SendConfigDto {
    pub upstreams: HashMap<String, UpstreamConfigDto>,
    pub client_headers: Headers,
    pub server_headers: Headers,
    pub consul: Option<Consul>,
    pub kubernetes: Option<Kubernetes>,
    pub typecfg: String,
    pub extraparams: Extraparams,
}

pub fn serialize_config(data: &UpstreamsDashMap) -> String {
    let config = GLOBAL_PARAMS.read().die("Failed to read config");
    let mut snapshot: HashMap<String, UpstreamConfigDto> = HashMap::new();

    for outer_entry in data.iter() {
        let domain = outer_entry.key().to_string();
        let mut paths: HashMap<String, PathConfigDto> = HashMap::new();

        for inner_entry in outer_entry.value().iter() {
            let path = inner_entry.key().to_string();
            let (vec_inner, _atomic_val) = inner_entry.value();

            if let Some(first) = vec_inner.first() {
                let servers = vec_inner.iter().map(|item| format!("{}:{}", item.address, item.port)).collect();
                let mut auth = None;
                if let Some(ath) = first.authorization.clone() {
                    let mut a = HashMap::new();
                    a.insert("type".to_string(), ath.auth_type.clone().to_string());
                    a.insert("data".to_string(), ath.auth_cred.clone().to_string());
                    auth = Some(a);
                }
                paths.insert(
                    path,
                    PathConfigDto {
                        is_http2: first.is_http2,
                        healthcheck: first.healthcheck,
                        is_ssl: first.is_ssl,
                        to_https: first.to_https,
                        rate_limit: first.rate_limit,
                        x4xx_limit: first.x4xx_limit,
                        redirect_to: first.redirect_to.clone().as_ref().map(|s| s.to_string()),
                        authorization: auth,
                        servers,
                    },
                );
            }
        }

        snapshot.insert(domain, UpstreamConfigDto { paths });
    }
    let payload = SendConfigDto {
        upstreams: snapshot,
        client_headers: config.client_headers.clone(),
        server_headers: config.server_headers.clone(),
        consul: config.consul.clone(),
        kubernetes: config.kubernetes.clone(),
        typecfg: config.typecfg.clone(),
        extraparams: config.extraparams.clone(),
    };
    to_yaml(&payload)
}

#[derive(Serialize, Debug)]
struct YamlAuth {
    #[serde(rename = "type")]
    auth_type: String,
    data: String,
}

#[derive(Serialize)]
struct YamlPath {
    servers: Vec<String>,
    #[serde(skip_serializing_if = "is_false")]
    is_http2: bool,
    #[serde(skip_serializing_if = "is_false")]
    to_https: bool,
    #[serde(skip_serializing_if = "is_false")]
    is_ssl: bool,
    #[serde(skip_serializing_if = "is_true")] // healthcheck true is default
    healthcheck: bool,
    #[serde(skip_serializing_if = "is_zero_isize")]
    rate_limit: isize,
    #[serde(skip_serializing_if = "is_zero_u32")]
    x4xx_limit: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    redirect_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    authorization: Option<YamlAuth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_headers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_headers: Option<Vec<String>>,
}

#[derive(Serialize)]
struct YamlUpstream {
    paths: HashMap<String, YamlPath>,
}

#[derive(Serialize)]
struct YamlConfig {
    provider: String,
    sticky_sessions: u64,
    to_https: bool,
    rate_limit: isize,
    x4xx_limit: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    server_headers: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    client_headers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    authorization: Option<YamlAuth>,
    upstreams: HashMap<String, YamlUpstream>,
}

fn is_false(v: &bool) -> bool {
    !v
}
fn is_true(v: &bool) -> bool {
    *v
}
fn is_zero_isize(v: &isize) -> bool {
    *v == 0
}
fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

pub fn to_yaml(config: &SendConfigDto) -> String {
    let server_headers: Vec<String> = config
        .server_headers
        .get("GLOBAL_SERVER_HEADERS")
        .map(|paths| {
            paths
                .value()
                .iter()
                .flat_map(|inner| inner.value().iter().map(|(k, v)| format!("{}:{}", k, v)).collect::<Vec<_>>())
                .collect()
        })
        .unwrap_or_default();

    let client_headers: Vec<String> = config
        .client_headers
        .get("GLOBAL_CLIENT_HEADERS")
        .map(|paths| {
            paths
                .value()
                .iter()
                .flat_map(|inner| inner.value().iter().map(|(k, v)| format!("{}:{}", k, v)).collect::<Vec<_>>())
                .collect()
        })
        .unwrap_or_default();

    let authorization = config.extraparams.authentication.as_ref().map(|auth| YamlAuth {
        auth_type: auth.auth_type.to_string(),
        data: auth.auth_cred.to_string(),
    });

    let upstreams: HashMap<String, YamlUpstream> = config
        .upstreams
        .iter()
        .map(|(domain, upstream)| {
            let paths = upstream
                .paths
                .iter()
                .map(|(path, pcfg)| {
                    let ch: Option<Vec<String>> = {
                        if let Some(domain_ref) = config.client_headers.get(domain.as_str()) {
                            if let Some(path_ref) = domain_ref.get(path.as_str()) {
                                let collected: Vec<String> = path_ref.value().iter().map(|(k, v)| format!("{}:{}", k, v)).collect();
                                if !collected.is_empty() { Some(collected) } else { None }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };

                    let sh: Option<Vec<String>> = {
                        if let Some(domain_ref) = config.server_headers.get(domain.as_str()) {
                            if let Some(path_ref) = domain_ref.get(path.as_str()) {
                                let collected: Vec<String> = path_ref.value().iter().map(|(k, v)| format!("{}:{}", k, v)).collect();
                                if !collected.is_empty() { Some(collected) } else { None }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };

                    let auth = pcfg.authorization.as_ref().map(|a| YamlAuth {
                        auth_type: a.get("type").cloned().unwrap_or_default(),
                        data: a.get("data").cloned().unwrap_or_default(),
                    });

                    (
                        path.clone(),
                        YamlPath {
                            servers: pcfg.servers.clone(),
                            is_http2: pcfg.is_http2,
                            is_ssl: pcfg.is_ssl,
                            healthcheck: pcfg.healthcheck,
                            rate_limit: pcfg.rate_limit,
                            x4xx_limit: pcfg.x4xx_limit,
                            to_https: pcfg.to_https,
                            redirect_to: pcfg.redirect_to.clone(),
                            authorization: auth,
                            client_headers: ch,
                            server_headers: sh,
                        },
                    )
                })
                .collect();
            (domain.clone(), YamlUpstream { paths })
        })
        .collect();

    let yaml_config = YamlConfig {
        provider: config.typecfg.clone(),
        sticky_sessions: config.extraparams.sticky_sessions,
        to_https: config.extraparams.to_https,
        rate_limit: config.extraparams.rate_limit,
        x4xx_limit: config.extraparams.x4xx_limit,
        server_headers,
        client_headers,
        authorization,
        upstreams,
    };

    to_string(&yaml_config).unwrap_or_default()
}

pub async fn send_to_worker(data: String) {
    let masterkey = MASTER_KEY.get().die("Master key not set");
    if let Some(workers) = WORKERS.get() {
        for worker in workers {
            let prefix = if worker.tls { "https://" } else { "http://" };
            let worker_url = format!("{}{}:{}", prefix, worker.address, worker.port);
            let status = httpclient(HttpClientRequest {
                method: "POST",
                tls: worker.tls,
                host: worker.address.as_ref(),
                path: "/workerconf",
                address: worker.address.as_ref(),
                port: worker.port,
                url: worker_url.as_str(),
                payload: Bytes::from(data.clone()),
                headers: Some(vec![("API-KEY", masterkey.clone().die("Master key must be set in the cluster mode").deref())]),
            })
            .await;
            if (200..500).contains(&status.2) {
                info!("Successfully updated Worker {}, status {}", worker_url, status.2);
            } else {
                error!("Cannot connect to worker node, status {}", status.2);
            }
        }
    } else {
        warn!("Workers list has not been initialized yet.");
    }
}
