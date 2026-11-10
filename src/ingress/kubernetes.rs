use crate::utils::httpclient;
use crate::utils::parceyaml::build_headers;
use crate::utils::types::{Configuration, GlobalServiceMapping, Headers, UpstreamsDashMap};
use async_trait::async_trait;
use log::error;
use rand::RngExt;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;

use crate::ingress::kuberconsul;
use crate::ingress::kuberconsul::{ServiceDiscovery, list_to_upstreams};
use crate::ingress::kubewatcher::{AralezRouteUpdate, start_ingress_watcher_with_config};
use crate::utils::tools::{Die, compare_dashmaps, print_upstreams};
use rustls::crypto::CryptoProvider;
use rustls::crypto::ring::default_provider;
use tokio::sync::mpsc::Sender;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubeEndpointSliceList {
    #[serde(default)]
    pub items: Vec<KubeEndpointSlice>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubeEndpointSlice {
    pub ports: Option<Vec<KubePort>>,
    #[serde(default)]
    pub endpoints: Vec<KubeEndpoint>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubeEndpoint {
    #[serde(default)]
    pub addresses: Vec<String>,
    pub conditions: Option<KubeConditions>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubeConditions {
    pub ready: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubePort {
    pub port: Option<u16>,
}

pub struct KubernetesDiscovery;

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
            let server = servers.get(num).die("Getting k8s api server").to_string();
            let path = kuber.tokenpath.unwrap_or_else(|| "/var/run/secrets/kubernetes.io/serviceaccount/token".to_string());
            let token = kuberconsul::read_token(path.as_str()).await;

            if CryptoProvider::get_default().is_none() {
                default_provider().install_default().die("Failed to install rustls crypto provider");
            }

            let (tx, mut rx) = tokio::sync::mpsc::channel::<AralezRouteUpdate>(100);
            let srv = server.clone();
            let tok = token.clone();
            let _watcher_handle = tokio::spawn(async move {
                if let Err(e) = start_ingress_watcher_with_config(srv.as_str(), tok.as_str(), tx, "aralez").await {
                    error!("Fatal watcher error: {:?}", e);
                }
            });
            let mut upstreams = HashMap::new();
            for (host_key, inner_map) in &config.upstreams {
                let mut cloned_inner = HashMap::new();
                for (path_key, (servers, atomic_val)) in inner_map {
                    let current_val = atomic_val.load(std::sync::atomic::Ordering::Relaxed);
                    cloned_inner.insert(path_key.clone(), (servers.clone(), std::sync::atomic::AtomicUsize::new(current_val)));
                }
                upstreams.insert(host_key.clone(), cloned_inner);
            }
            let mut local_client_headers = config.client_headers.clone();
            let mut local_server_headers = config.server_headers.clone();

            while let Some(update) = rx.recv().await {
                let host_key: Arc<str> = Arc::from(update.host.as_str());

                if update.is_deleted {
                    upstreams.remove(&host_key);
                    local_client_headers.remove(&host_key);
                    local_server_headers.remove(&host_key);
                } else {
                    let service = GlobalServiceMapping {
                        upstream: update.service_name.clone(),
                        hostname: update.host.clone(),
                        path: Some(update.path.clone()),
                        to_https: None,
                        redirect_to: None,
                        authorization: None,
                        rate_limit: update.rate_limit,
                        x4xx_limit: update.x4xx_limit,
                        client_headers: None,
                        server_headers: None,
                    };

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

                    let mut client_headers_list = Vec::new();
                    build_headers(&chl, config.as_ref(), &mut client_headers_list);
                    if !client_headers_list.is_empty() {
                        let path_key = service.path.as_deref().unwrap_or("/");
                        local_client_headers
                            .entry(host_key.clone())
                            .or_insert_with(HashMap::new)
                            .insert(Arc::from(path_key), client_headers_list);
                    }

                    let mut server_headers_list = Vec::new();
                    build_headers(&shl, config.as_ref(), &mut server_headers_list);
                    if !server_headers_list.is_empty() {
                        let path_key = service.path.as_deref().unwrap_or("/");
                        local_server_headers
                            .entry(host_key.clone())
                            .or_insert_with(HashMap::new)
                            .insert(Arc::from(path_key), server_headers_list);
                    }

                    let url = format!(
                        "https://{}/apis/discovery.k8s.io/v1/namespaces/{}/endpointslices?labelSelector=kubernetes.io%2Fservice-name%3D{}",
                        server, update.namespace, update.service_name
                    );

                    let list = httpclient::for_kuber(&url, &token, &service).await;

                    if list.is_none() {
                        error!("Endpoint URL: {} returned empty/invalid response", url);
                    }

                    list_to_upstreams(list, &mut upstreams, &service);
                }

                // 2. Pass current scratch state into clone_compare to check differences and produce new snapshot
                if let Some(lt) = clone_compare_with_headers(&upstreams, &local_client_headers, &local_server_headers, &config).await {
                    let _ = toreturn.send(lt).await;
                }
            }
        }
    }
}

pub async fn clone_compare_with_headers(
    new_upstreams: &UpstreamsDashMap,
    new_client_headers: &Headers,
    new_server_headers: &Headers,
    config: &Configuration,
) -> Option<Configuration> {
    if !compare_dashmaps(&new_upstreams, &config.upstreams) || new_client_headers != &config.client_headers || new_server_headers != &config.server_headers {
        let mut cloned_upstreams = HashMap::new();
        for (host_key, inner_map) in new_upstreams {
            let mut cloned_inner = HashMap::new();
            for (path_key, (servers, atomic_val)) in inner_map {
                let current_val = atomic_val.load(std::sync::atomic::Ordering::Relaxed);
                cloned_inner.insert(path_key.clone(), (servers.clone(), std::sync::atomic::AtomicUsize::new(current_val)));
            }
            cloned_upstreams.insert(host_key.clone(), cloned_inner);
        }

        let tosend = Configuration {
            upstreams: cloned_upstreams,
            client_headers: new_client_headers.clone(),
            server_headers: new_server_headers.clone(),
            consul: config.consul.clone(),
            kubernetes: config.kubernetes.clone(),
            typecfg: config.typecfg.clone(),
            extraparams: config.extraparams.clone(),
        };

        print_upstreams(&tosend.upstreams, &tosend.extraparams);
        return Some(tosend);
    }
    None
}
