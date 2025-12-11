use crate::utils::kuberconsul::{match_path, ConsulService, KubeEndpoints};
use crate::utils::structs::{InnerMap, ServiceMapping};
use axum::http::{HeaderMap, HeaderValue};
use dashmap::DashMap;
use reqwest::Client;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

pub async fn for_consul(url: String, token: Option<String>, conf: &ServiceMapping) -> Option<DashMap<String, (Vec<Arc<InnerMap>>, AtomicUsize)>> {
    let client = Client::builder().timeout(Duration::from_secs(2)).danger_accept_invalid_certs(true).build().ok()?;
    let mut headers = HeaderMap::new();
    if let Some(token) = token {
        headers.insert("X-Consul-Token", HeaderValue::from_str(&token).unwrap());
    }
    let to = Duration::from_secs(1);
    let resp = client.get(url).timeout(to).send().await.ok()?;
    if !resp.status().is_success() {
        eprintln!("Consul API returned status: {}", resp.status());
        return None;
    }
    let mut inner_vec = Vec::new();
    let upstreams: DashMap<String, (Vec<Arc<InnerMap>>, AtomicUsize)> = DashMap::new();
    let endpoints: Vec<ConsulService> = resp.json().await.ok()?;
    for subsets in endpoints {
        // let addr = subsets.tagged_addresses.get("lan_ipv4").unwrap().address.clone();
        // let prt = subsets.tagged_addresses.get("lan_ipv4").unwrap().port.clone();
        let addr = subsets.tagged_addresses.get("lan_ipv4").unwrap().address.clone().parse().unwrap();
        let prt = subsets.tagged_addresses.get("lan_ipv4").unwrap().port.clone();
        let to_add = Arc::from(InnerMap {
            address: addr,
            port: prt,
            is_ssl: false,
            is_http2: false,
            to_https: conf.to_https.unwrap_or(false),
            rate_limit: conf.rate_limit,
            healthcheck: None,
        });
        inner_vec.push(to_add);
    }
    match_path(&conf, &upstreams, inner_vec.clone());
    Some(upstreams)
}

pub async fn for_kuber(url: &str, token: &str, conf: &ServiceMapping) -> Option<DashMap<String, (Vec<Arc<InnerMap>>, AtomicUsize)>> {
    let to = Duration::from_secs(10);
    let client = Client::builder().timeout(Duration::from_secs(10)).danger_accept_invalid_certs(true).build().ok()?;
    let resp = client.get(url).timeout(to).bearer_auth(token).send().await.ok()?;
    if !resp.status().is_success() {
        eprintln!("Kubernetes API returned status: {}", resp.status());
        return None;
    }
    let endpoints: KubeEndpoints = resp.json().await.ok()?;
    let upstreams: DashMap<String, (Vec<Arc<InnerMap>>, AtomicUsize)> = DashMap::new();
    if let Some(subsets) = endpoints.subsets {
        for subset in subsets {
            if let (Some(addresses), Some(ports)) = (subset.addresses, subset.ports) {
                let mut inner_vec = Vec::new();
                for addr in addresses {
                    for port in &ports {
                        let to_add = Arc::from(InnerMap {
                            address: addr.ip.parse().unwrap(),
                            port: port.port.clone(),
                            is_ssl: false,
                            is_http2: false,
                            to_https: conf.to_https.unwrap_or(false),
                            rate_limit: conf.rate_limit,
                            healthcheck: None,
                        });
                        inner_vec.push(to_add);
                    }
                }
                match_path(&conf, &upstreams, inner_vec.clone());
            }
        }
    }
    Some(upstreams)
}
