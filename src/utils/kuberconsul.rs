use crate::utils::httpclient;
use crate::utils::kubewatcher::{start_ingress_watcher_with_config, AralezRouteUpdate};
use crate::utils::parceyaml::build_headers;
use crate::utils::structs::{Configuration, GlobalServiceMapping, InnerMap, UpstreamsDashMap};
use crate::utils::tools::{clone_dashmap_into, compare_dashmaps, print_upstreams};
use async_trait::async_trait;
use dashmap::DashMap;
use log::error;
use pingora::prelude::sleep;
use rand::RngExt;
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

use rustls::crypto::ring::default_provider;
use rustls::crypto::CryptoProvider;
use tokio::sync::mpsc::Sender;

#[derive(Debug, serde::Deserialize)]
pub struct KubeEndpoints {
    pub subsets: Option<Vec<KubeSubset>>,
}

#[derive(Debug, serde::Deserialize)]
pub struct KubeSubset {
    pub addresses: Option<Vec<KubeAddress>>,
    pub ports: Option<Vec<KubePort>>,
}

#[derive(Debug, serde::Deserialize)]
pub struct KubeAddress {
    pub ip: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct KubePort {
    pub port: u16,
}

#[derive(Debug, Deserialize)]
pub struct ConsulService {
    #[serde(rename = "ServiceTaggedAddresses")]
    pub tagged_addresses: HashMap<String, ConsulTaggedAddress>,
}

#[derive(Debug, Deserialize)]
pub struct ConsulTaggedAddress {
    #[serde(rename = "Address")]
    pub address: String,
    #[serde(rename = "Port")]
    pub port: u16,
}

#[allow(clippy::type_complexity)]
pub fn list_to_upstreams(lt: Option<DashMap<Arc<str>, (Vec<Arc<InnerMap>>, AtomicUsize)>>, upstreams: &UpstreamsDashMap, i: &GlobalServiceMapping) {
    if let Some(list) = lt {
        let key: Arc<str> = Arc::from(i.hostname.as_str());
        match upstreams.get(&key) {
            Some(upstr) => {
                for (k, v) in list {
                    upstr.value().insert(k, v);
                }
            }
            None => {
                upstreams.insert(key, list);
            }
        };
    }
}

pub fn match_path(conf: &GlobalServiceMapping, upstreams: &DashMap<Arc<str>, (Vec<Arc<InnerMap>>, AtomicUsize)>, values: Vec<Arc<InnerMap>>) {
    let path = conf.path.as_deref().unwrap_or("/");
    upstreams.insert(Arc::from(path), (values, AtomicUsize::new(0)));
}

async fn read_token(path: &str) -> String {
    let mut file = File::open(path).await.unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).await.unwrap();
    contents.trim().to_string()
}

#[async_trait]
pub trait ServiceDiscovery {
    async fn fetch_upstreams(&self, config: Arc<Configuration>, toreturn: Sender<Configuration>);
}

pub struct KubernetesDiscovery;
pub struct ConsulDiscovery;

#[async_trait]
impl ServiceDiscovery for KubernetesDiscovery {
    async fn fetch_upstreams(&self, config: Arc<Configuration>, toreturn: Sender<Configuration>) {
        if let Some(kuber) = config.kubernetes.clone() {
            let servers = kuber.servers.unwrap_or_else(|| {
                vec![format!(
                    "{}:{}",
                    env::var("KUBERNETES_SERVICE_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
                    env::var("KUBERNETES_SERVICE_PORT_HTTPS").unwrap_or_else(|_| "0".to_string())
                )]
            });

            let end = servers.len().saturating_sub(1);
            let num = if end > 0 { rand::rng().random_range(0..end) } else { 0 };
            let server = servers.get(num).unwrap().to_string();
            let path = kuber.tokenpath.unwrap_or_else(|| "/var/run/secrets/kubernetes.io/serviceaccount/token".to_string());
            let namespace = get_current_namespace().unwrap_or_else(|| "default".to_string());
            let token = read_token(path.as_str()).await;

            if CryptoProvider::get_default().is_none() {
                default_provider().install_default().expect("Failed to install rustls crypto provider");
            }

            let (tx, mut rx) = tokio::sync::mpsc::channel::<AralezRouteUpdate>(100);
            let srv = server.clone();
            let tok = token.clone();
            tokio::spawn(async move {
                if let Err(e) = start_ingress_watcher_with_config(srv.as_str(), tok.as_str(), tx, "aralez").await {
                    error!("Fatal watcher error: {:?}", e);
                }
            });

            let upstreams = UpstreamsDashMap::new();

            while let Some(update) = rx.recv().await {
                let host_key: Arc<str> = Arc::from(update.host.as_str());

                if update.is_deleted {
                    upstreams.remove(&host_key);
                    config.client_headers.remove(&host_key);
                } else {
                    let service = GlobalServiceMapping {
                        upstream: update.service_name.clone(),
                        hostname: update.host.clone(),
                        path: Some(update.path.clone()),
                        to_https: None,
                        sticky_sessions: update.sticky_sessions,
                        rate_limit: update.rate_limit,
                        x4xx_limit: update.x4xx_limit,
                        client_headers: None,
                        server_headers: None,
                    };

                    let cheader_list: DashMap<Arc<str>, Vec<(String, Arc<str>)>> = DashMap::new();
                    let sheader_list: DashMap<Arc<str>, Vec<(String, Arc<str>)>> = DashMap::new();

                    let mut client_headers_list = Vec::new();
                    let mut server_headers_list = Vec::new();

                    let mut chl: Option<Vec<String>> = None;
                    let mut shl: Option<Vec<String>> = None;

                    if let Some(hdr) = service.client_headers.as_ref() {
                        chl.get_or_insert_with(Vec::new).extend(hdr.iter().cloned());
                    }
                    if let Some(hdr) = service.server_headers.as_ref() {
                        shl.get_or_insert_with(Vec::new).extend(hdr.iter().cloned());
                    }

                    if let Some(ch) = update.client_headers {
                        chl.get_or_insert_with(Vec::new).extend(ch);
                    }
                    if let Some(hdr) = update.server_headers.as_ref() {
                        shl.get_or_insert_with(Vec::new).extend(hdr.iter().cloned());
                    }

                    build_headers(&chl, config.as_ref(), &mut client_headers_list);
                    if !client_headers_list.is_empty() {
                        let path_key = service.path.as_deref().unwrap_or("/");
                        cheader_list.insert(Arc::from(path_key), client_headers_list);
                        config.client_headers.insert(host_key.clone(), cheader_list);
                    }

                    build_headers(&shl, config.as_ref(), &mut server_headers_list);
                    if !server_headers_list.is_empty() {
                        let path_key = service.path.as_deref().unwrap_or("/");
                        sheader_list.insert(Arc::from(path_key), server_headers_list);
                        config.server_headers.insert(host_key.clone(), sheader_list);
                    }
                    let url = format!("https://{}/api/v1/namespaces/{}/endpoints/{}", server, namespace, update.service_name);
                    let list = httpclient::for_kuber(&url, &token, &service).await;

                    if list.is_none() {
                        error!("Endpoint URL: {} returned empty/invalid response", url);
                    }

                    list_to_upstreams(list, &upstreams, &service);
                }
                if let Some(lt) = clone_compare(&upstreams, &config).await {
                    let _ = toreturn.send(lt).await;
                }
            }
        }
    }
}

fn get_current_namespace() -> Option<String> {
    let ns_path = "/var/run/secrets/kubernetes.io/serviceaccount/namespace";
    if Path::new(ns_path).exists() {
        if let Ok(contents) = fs::read_to_string(ns_path) {
            return Some(contents.trim().to_string());
        }
    }
    env::var("POD_NAMESPACE").ok()
}

#[async_trait]
impl ServiceDiscovery for ConsulDiscovery {
    async fn fetch_upstreams(&self, config: Arc<Configuration>, toreturn: Sender<Configuration>) {
        loop {
            let upstreams = UpstreamsDashMap::new();

            if let Some(consul) = config.consul.clone() {
                let servers = consul.servers.unwrap_or_else(|| {
                    vec![format!(
                        "{}:{}",
                        env::var("CONSUL_SERVICE_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
                        env::var("CONSUL_SERVICE_PORT").unwrap_or_else(|_| "0".to_string())
                    )]
                });
                let end = servers.len().saturating_sub(1);
                let num = if end > 0 { rand::rng().random_range(0..end) } else { 0 };
                let consul_data = servers.get(num).unwrap().to_string();
                let ss = format!("{}/v1/catalog/service/", consul_data);

                if let Some(svc) = consul.services {
                    for i in &svc {
                        let header_list = DashMap::new();
                        let mut hl = Vec::new();
                        build_headers(&i.client_headers, config.as_ref(), &mut hl);
                        if !hl.is_empty() {
                            let path_key = i.path.as_deref().unwrap_or("/");
                            header_list.insert(Arc::from(path_key), hl);
                            config.client_headers.insert(Arc::from(i.hostname.clone()), header_list);
                        }

                        let pref = format!("{}{}", ss, i.upstream);
                        let list = httpclient::for_consul(pref.as_str(), consul.token.clone(), i).await;
                        list_to_upstreams(list, &upstreams, i);
                    }
                }
            }

            if let Some(lt) = clone_compare(&upstreams, &config).await {
                let _ = toreturn.send(lt).await;
            }

            sleep(Duration::from_secs(5)).await;
        }
    }
}

async fn clone_compare(upstreams: &UpstreamsDashMap, config: &Arc<Configuration>) -> Option<Configuration> {
    if !compare_dashmaps(upstreams, &config.upstreams) {
        let tosend = Configuration {
            upstreams: Default::default(),
            client_headers: config.client_headers.clone(),
            server_headers: config.server_headers.clone(),
            consul: config.consul.clone(),
            kubernetes: config.kubernetes.clone(),
            typecfg: config.typecfg.clone(),
            extraparams: config.extraparams.clone(),
        };
        clone_dashmap_into(upstreams, &config.upstreams);
        clone_dashmap_into(upstreams, &tosend.upstreams);

        print_upstreams(&tosend.upstreams, &tosend.extraparams);
        return Some(tosend);
    }
    None
}
