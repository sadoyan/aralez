use crate::utils::structs::{Configuration, InnerMap, ServiceMapping, UpstreamsDashMap};
use crate::utils::tools::{clone_dashmap_into, compare_dashmaps, print_upstreams};
use dashmap::DashMap;
use futures::channel::mpsc::Sender;
use futures::SinkExt;
use log::{info, warn};
use pingora::prelude::sleep;
use rand::Rng;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Deserialize)]
struct Service {
    #[serde(rename = "ServiceTaggedAddresses")]
    tagged_addresses: HashMap<String, TaggedAddress>,
}

#[derive(Debug, Deserialize)]
struct TaggedAddress {
    #[serde(rename = "Address")]
    address: String,
    #[serde(rename = "Port")]
    port: u16,
}

pub async fn start(mut toreturn: Sender<Configuration>, config: Arc<Configuration>) {
    let headers = DashMap::new();
    info!("Consul Discovery is enabled : {}", config.typecfg);
    let consul = config.consul.clone();
    let prev_upstreams = UpstreamsDashMap::new();
    match consul {
        Some(consul) => {
            let servers = consul.servers.unwrap();
            info!("Consul Servers => {:?}", servers);
            let end = servers.len() - 1;

            loop {
                let mut num = 0;
                if end > 0 {
                    num = rand::rng().random_range(0..end);
                }
                headers.clear();
                for (k, v) in config.headers.clone() {
                    headers.insert(k.to_string(), v);
                }
                let consul_data = servers.get(num).unwrap().to_string();
                let upstreams = consul_request(consul_data, consul.services.clone(), consul.token.clone());
                match upstreams.await {
                    Some(upstreams) => {
                        if !compare_dashmaps(&upstreams, &prev_upstreams) {
                            let mut tosend: Configuration = Configuration {
                                upstreams: Default::default(),
                                headers: Default::default(),
                                consul: None,
                                kubernetes: None,
                                typecfg: "".to_string(),
                                extraparams: config.extraparams.clone(),
                            };

                            clone_dashmap_into(&upstreams, &prev_upstreams);
                            clone_dashmap_into(&upstreams, &tosend.upstreams);
                            tosend.headers = headers.clone();
                            tosend.extraparams.authentication = config.extraparams.authentication.clone();
                            tosend.typecfg = config.typecfg.clone();
                            tosend.consul = config.consul.clone();
                            print_upstreams(&tosend.upstreams);
                            toreturn.send(tosend).await.unwrap();
                        }
                    }
                    None => {}
                }
                sleep(Duration::from_secs(5)).await;
            }
        }
        None => {}
    }
}

async fn consul_request(url: String, whitelist: Option<Vec<ServiceMapping>>, token: Option<String>) -> Option<UpstreamsDashMap> {
    let upstreams = UpstreamsDashMap::new();
    let ss = url.clone() + "/v1/catalog/service/";
    match whitelist {
        Some(whitelist) => {
            for k in whitelist.iter() {
                let pref: String = ss.clone() + &k.hostname;
                let list = get_by_http(pref.clone(), token.clone()).await;
                match list {
                    Some(list) => {
                        upstreams.insert(k.upstream.clone(), list);
                    }
                    None => {
                        warn!("Whitelist not found for {}", k.upstream);
                    }
                }
            }
        }
        None => {}
    }
    Some(upstreams)
}

async fn get_by_http(url: String, token: Option<String>) -> Option<DashMap<String, (Vec<InnerMap>, AtomicUsize)>> {
    let client = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    if let Some(token) = token {
        headers.insert("X-Consul-Token", HeaderValue::from_str(&token).unwrap());
    }
    let to = Duration::from_secs(1);
    let u = client.get(url).timeout(to).send();
    let mut values = Vec::new();
    let upstreams: DashMap<String, (Vec<InnerMap>, AtomicUsize)> = DashMap::new();
    match u.await {
        Ok(r) => {
            let jason = r.json::<Vec<Service>>().await;
            match jason {
                Ok(whitelist) => {
                    for service in whitelist {
                        let addr = service.tagged_addresses.get("lan_ipv4").unwrap().address.clone();
                        let prt = service.tagged_addresses.get("lan_ipv4").unwrap().port.clone();
                        let to_add = InnerMap {
                            address: addr,
                            port: prt,
                            is_ssl: false,
                            is_http2: false,
                            to_https: false,
                            rate_limit: None,
                            healthcheck: None,
                        };
                        values.push(to_add);
                    }
                }
                Err(_) => return None,
            }
        }
        Err(_) => return None,
    }
    upstreams.insert("/".to_string(), (values, AtomicUsize::new(0)));
    Some(upstreams)
}
