use crate::utils::kuberconsul::*;
use crate::utils::parceyaml::build_headers;
use crate::utils::structs::{Configuration, InnerMap, ServiceMapping, UpstreamsDashMap};
use crate::utils::tools::{clone_dashmap_into, compare_dashmaps, print_upstreams};
use dashmap::DashMap;
use futures::channel::mpsc::Sender;
use futures::SinkExt;
use pingora::prelude::sleep;
use rand::Rng;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client;
use std::env;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

pub async fn start(mut toreturn: Sender<Configuration>, config: Arc<Configuration>) {
    let prev_upstreams = UpstreamsDashMap::new();
    loop {
        let upstreams = UpstreamsDashMap::new();
        if let Some(consul) = config.consul.clone() {
            let servers = consul.servers.unwrap_or(vec![format!(
                "{}:{}",
                env::var("CONSUL_SERVICE_HOST").unwrap_or("0.0.0.0".to_string()),
                env::var("CONSUL_SERVICE_PORT").unwrap_or("0".to_string())
            )]);
            let end = servers.len() - 1;
            let mut num = 0;
            if end > 0 {
                num = rand::rng().random_range(0..end);
            }
            let consul_data = servers.get(num).unwrap().to_string();
            let ss = consul_data + "/v1/catalog/service/";
            if let Some(svc) = consul.services {
                for i in svc {
                    let header_list = DashMap::new();
                    let mut hl = Vec::new();
                    build_headers(&i.headers, config.as_ref(), &mut hl);
                    if hl.len() > 0 {
                        header_list.insert(i.path.clone().unwrap_or("/".to_string()), hl);
                        config.headers.insert(i.hostname.clone(), header_list);
                    }
                    let pref: String = ss.clone() + &i.upstream;
                    let list = get_by_http(pref, consul.token.clone(), &i).await;
                    list_to_upstreams(list, &upstreams, &i);
                }
            }
            if !compare_dashmaps(&upstreams, &prev_upstreams) {
                let tosend: Configuration = Configuration {
                    upstreams: Default::default(),
                    headers: config.headers.clone(),
                    consul: config.consul.clone(),
                    kubernetes: config.kubernetes.clone(),
                    typecfg: config.typecfg.clone(),
                    extraparams: config.extraparams.clone(),
                };
                clone_dashmap_into(&upstreams, &prev_upstreams);
                clone_dashmap_into(&upstreams, &tosend.upstreams);
                print_upstreams(&tosend.upstreams);
                toreturn.send(tosend).await.unwrap();
            }
        }
        sleep(Duration::from_secs(5)).await;
    }
}

async fn get_by_http(url: String, token: Option<String>, conf: &ServiceMapping) -> Option<DashMap<String, (Vec<InnerMap>, AtomicUsize)>> {
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
    let upstreams: DashMap<String, (Vec<InnerMap>, AtomicUsize)> = DashMap::new();
    let endpoints: Vec<ConsulService> = resp.json().await.ok()?;
    for subsets in endpoints {
        let addr = subsets.tagged_addresses.get("lan_ipv4").unwrap().address.clone();
        let prt = subsets.tagged_addresses.get("lan_ipv4").unwrap().port.clone();
        let to_add = InnerMap {
            address: addr,
            port: prt,
            is_ssl: false,
            is_http2: false,
            to_https: conf.to_https.unwrap_or(false),
            rate_limit: conf.rate_limit,
            healthcheck: None,
        };
        inner_vec.push(to_add);
    }
    match_path(&conf, &upstreams, inner_vec.clone());
    Some(upstreams)
}
