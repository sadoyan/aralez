use crate::utils::httpclient;
use crate::utils::parceyaml::build_headers;
use crate::utils::structs::{Configuration, InnerMap, ServiceMapping, UpstreamsDashMap};
use crate::utils::tools::{clone_dashmap_into, compare_dashmaps, print_upstreams};
use async_trait::async_trait;
use dashmap::DashMap;
use futures::channel::mpsc::Sender;
use futures::SinkExt;
use pingora::prelude::sleep;
use rand::Rng;
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

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
pub fn list_to_upstreams(lt: Option<DashMap<String, (Vec<InnerMap>, AtomicUsize)>>, upstreams: &UpstreamsDashMap, i: &ServiceMapping) {
    if let Some(list) = lt {
        match upstreams.get(&i.hostname.clone()) {
            Some(upstr) => {
                for (k, v) in list {
                    upstr.value().insert(k, v);
                }
            }
            None => {
                upstreams.insert(i.hostname.clone(), list);
            }
        };
    }
}

pub fn match_path(conf: &ServiceMapping, upstreams: &DashMap<String, (Vec<InnerMap>, AtomicUsize)>, values: Vec<InnerMap>) {
    match conf.path {
        Some(ref p) => {
            upstreams.insert(p.to_string(), (values, AtomicUsize::new(0)));
        }
        None => {
            upstreams.insert("/".to_string(), (values, AtomicUsize::new(0)));
        }
    }
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
    async fn fetch_upstreams(&self, config: Arc<Configuration>, mut toreturn: Sender<Configuration>) {
        let prev_upstreams = UpstreamsDashMap::new();

        if let Some(kuber) = config.kubernetes.clone() {
            let servers = kuber.servers.unwrap_or(vec![format!(
                "{}:{}",
                env::var("KUBERNETES_SERVICE_HOST").unwrap_or("0.0.0.0".to_string()),
                env::var("KUBERNETES_SERVICE_PORT_HTTPS").unwrap_or("0".to_string())
            )]);

            let end = servers.len().saturating_sub(1);
            let num = if end > 0 { rand::rng().random_range(0..end) } else { 0 };
            let server = servers.get(num).unwrap().to_string();
            let path = kuber.tokenpath.unwrap_or("/var/run/secrets/kubernetes.io/serviceaccount/token".to_string());
            let token = read_token(path.as_str()).await;
            // let mut oldcrt: HashMap<String, String> = HashMap::new();

            loop {
                // crate::utils::watchksecret::watch_secret("ar-tls", "staging", server.clone(), token.clone(), &mut oldcrt).await;
                let upstreams = UpstreamsDashMap::new();
                if let Some(kuber) = config.kubernetes.clone() {
                    if let Some(svc) = kuber.services {
                        for i in svc {
                            let header_list = DashMap::new();
                            let mut hl = Vec::new();
                            build_headers(&i.client_headers, config.as_ref(), &mut hl);
                            if !hl.is_empty() {
                                header_list.insert(i.path.clone().unwrap_or("/".to_string()), hl);
                                config.client_headers.insert(i.hostname.clone(), header_list);
                            }
                            let url = format!("https://{}/api/v1/namespaces/staging/endpoints/{}", server, i.hostname);
                            let list = httpclient::for_kuber(&*url, &*token, &i).await;
                            list_to_upstreams(list, &upstreams, &i);
                        }
                    }
                    if let Some(lt) = clone_compare(&upstreams, &prev_upstreams, &config).await {
                        toreturn.send(lt).await.unwrap();
                    }
                }
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

#[async_trait]
impl ServiceDiscovery for ConsulDiscovery {
    async fn fetch_upstreams(&self, config: Arc<Configuration>, mut toreturn: Sender<Configuration>) {
        let prev_upstreams = UpstreamsDashMap::new();
        loop {
            let upstreams = UpstreamsDashMap::new();

            if let Some(consul) = config.consul.clone() {
                let servers = consul.servers.unwrap_or(vec![format!(
                    "{}:{}",
                    env::var("CONSUL_SERVICE_HOST").unwrap_or("0.0.0.0".to_string()),
                    env::var("CONSUL_SERVICE_PORT").unwrap_or("0".to_string())
                )]);
                let end = servers.len().saturating_sub(1);
                let num = if end > 0 { rand::rng().random_range(0..end) } else { 0 };
                let consul_data = servers.get(num).unwrap().to_string();
                let ss = consul_data + "/v1/catalog/service/";

                if let Some(svc) = consul.services {
                    for i in svc {
                        let header_list = DashMap::new();
                        let mut hl = Vec::new();
                        build_headers(&i.client_headers, config.as_ref(), &mut hl);
                        if !hl.is_empty() {
                            header_list.insert(i.path.clone().unwrap_or("/".to_string()), hl);
                            config.client_headers.insert(i.hostname.clone(), header_list);
                        }

                        let pref = ss.clone() + &i.upstream;
                        let list = httpclient::for_consul(pref, consul.token.clone(), &i).await;
                        list_to_upstreams(list, &upstreams, &i);
                    }
                }
            }
            if let Some(lt) = clone_compare(&upstreams, &prev_upstreams, &config).await {
                toreturn.send(lt).await.unwrap();
            }
            sleep(Duration::from_secs(5)).await;
        }
    }
}
async fn clone_compare(upstreams: &UpstreamsDashMap, prev_upstreams: &UpstreamsDashMap, config: &Arc<Configuration>) -> Option<Configuration> {
    if !compare_dashmaps(&upstreams, &prev_upstreams) {
        let tosend: Configuration = Configuration {
            upstreams: Default::default(),
            client_headers: config.client_headers.clone(),
            server_headers: config.server_headers.clone(),
            consul: config.consul.clone(),
            kubernetes: config.kubernetes.clone(),
            typecfg: config.typecfg.clone(),
            extraparams: config.extraparams.clone(),
        };
        clone_dashmap_into(&upstreams, &prev_upstreams);
        clone_dashmap_into(&upstreams, &tosend.upstreams);
        print_upstreams(&tosend.upstreams);
        return Some(tosend);
    };
    None
}
