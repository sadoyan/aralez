use crate::utils::parceyaml::load_configuration;
use crate::utils::structs::{Configuration, ServiceMapping, UpstreamsDashMap};
use crate::utils::tools::{clone_dashmap_into, compare_dashmaps};
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

pub async fn start(fp: String, mut toreturn: Sender<Configuration>) {
    let config = load_configuration(fp.as_str(), "filepath");
    let headers = DashMap::new();

    match config {
        Some(config) => {
            if config.typecfg.to_string() != "consul" {
                info!("Not running Consul discovery, requested type is: {}", config.typecfg);
                return;
            }

            info!("Consul Discovery is enabled : {}", config.typecfg);
            let consul = config.consul.clone();
            let prev_upstreams = UpstreamsDashMap::new();
            match consul {
                Some(consul) => {
                    let servers = consul.servers.unwrap();
                    info!("Consul Servers => {:?}", servers);
                    let end = servers.len();

                    loop {
                        let num = rand::rng().random_range(1..end);
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
                                        typecfg: "".to_string(),
                                        extraparams: config.extraparams.clone(),
                                        globals: Default::default(),
                                    };

                                    clone_dashmap_into(&upstreams, &prev_upstreams);
                                    clone_dashmap_into(&upstreams, &tosend.upstreams);
                                    tosend.headers = headers.clone();
                                    tosend.globals = config.globals.clone();
                                    tosend.typecfg = config.typecfg.clone();
                                    tosend.consul = config.consul.clone();
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
        None => {}
    }
}

async fn consul_request(url: String, whitelist: Option<Vec<ServiceMapping>>, token: Option<String>) -> Option<UpstreamsDashMap> {
    let upstreams = UpstreamsDashMap::new();
    let ss = url.clone() + "/v1/catalog/service/";
    match whitelist {
        Some(whitelist) => {
            for k in whitelist.iter() {
                let pref: String = ss.clone() + &k.real;
                let list = get_by_http(pref.clone(), token.clone()).await;
                match list {
                    Some(list) => {
                        upstreams.insert(k.proxy.clone(), list);
                    }
                    None => {
                        warn!("Whitelist not found for {}", k.proxy);
                    }
                }
            }
        }
        None => {}
    }
    Some(upstreams)
}

async fn get_by_http(url: String, token: Option<String>) -> Option<DashMap<String, (Vec<(String, u16, bool)>, AtomicUsize)>> {
    let client = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    if let Some(token) = token {
        headers.insert("X-Consul-Token", HeaderValue::from_str(&token).unwrap());
    }
    let to = Duration::from_secs(1);
    let u = client.get(url).timeout(to).send();
    let mut values = Vec::new();
    let upstreams: DashMap<String, (Vec<(String, u16, bool)>, AtomicUsize)> = DashMap::new();
    match u.await {
        Ok(r) => {
            let jason = r.json::<Vec<Service>>().await;
            match jason {
                Ok(whitelist) => {
                    for service in whitelist {
                        let addr = service.tagged_addresses.get("lan_ipv4").unwrap().address.clone();
                        let prt = service.tagged_addresses.get("lan_ipv4").unwrap().port.clone();
                        let to_add = (addr, prt, false);
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
