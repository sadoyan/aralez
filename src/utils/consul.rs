use crate::utils::parceyaml::{load_configuration, Configuration, ServiceMapping};
use crate::utils::tools::{clone_dashmap_into, compare_dashmaps, UpstreamsDashMap};
use dashmap::DashMap;
use futures::channel::mpsc::Sender;
use futures::SinkExt;
// use hickory_client::client::{Client, ClientHandle};
// use hickory_client::proto::rr::{DNSClass, Name, RecordType};
// use hickory_client::proto::runtime::TokioRuntimeProvider;
// use hickory_client::proto::tcp::TcpClientStream;
use log::{info, warn};
use pingora::prelude::sleep;
use rand::Rng;
// use std::str::FromStr;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

#[derive(Debug, Deserialize)]
struct Service {
    // #[serde(rename = "ServiceName")]
    // service_name: String,
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

/*
async fn http_request(url: String, whitelist: Option<Vec<String>>) -> Option<UpstreamsDashMap> {
    let client = reqwest::Client::new();
    let to = Duration::from_secs(1);
    let upstreams = UpstreamsDashMap::new();
    let excludes = vec!["consul", "nomad", "nomad-client"];
    let ss = url.clone() + "/v1/catalog/service";
    let response = client.get(ss.clone() + "s").timeout(to).send().await;
    match response {
        Ok(r) => {
            let json = r.json::<HashMap<String, Vec<String>>>().await;
            match json {
                Ok(_j) => {
                    for (k, _v) in _j {
                        match whitelist.clone() {
                            Some(whitelist) => {
                                if whitelist.iter().any(|i| *i == k) {
                                    let mut pref: String = ss.clone() + "/";
                                    pref.push_str(&k);
                                    let list = get_by_http(pref).await;
                                    match list {
                                        Some(list) => {
                                            upstreams.insert(k.to_string(), list);
                                        }
                                        None => {}
                                    }
                                }
                            }
                            None => {
                                if !excludes.iter().any(|&i| i == k) {
                                    let mut pref: String = ss.clone() + "/";
                                    pref.push_str(&k);
                                    let list = get_by_http(pref).await;
                                    match list {
                                        Some(list) => {
                                            upstreams.insert(k.to_string(), list);
                                        }
                                        None => {}
                                    }
                                }
                            }
                        }
                    }
                    // print_upstreams(&upstreams);
                    Some(upstreams)
                }
                Err(_) => None,
            }
        }
        Err(e) => {
            println!("Error: {:?}", e);
            None
        }
    }
}
*/
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

// #[allow(dead_code)]
// async fn get_by_dns() {
//     let (stream, sender) = TcpClientStream::new(([192, 168, 22, 1], 53).into(), None, None, TokioRuntimeProvider::new());
//     let client = Client::new(stream, sender, None);
//     let (mut client, bg) = client.await.expect("connection failed");
//     tokio::spawn(bg);
//     let query = client.query(Name::from_str("_frontend-dev-frontend-srv._tcp.service.consul.").unwrap(), DNSClass::IN, RecordType::SRV);
//     // let query = client.query(Name::from_str("matyan.org.").unwrap(), DNSClass::IN, RecordType::A);
//     let response = query.await.unwrap();
//
//     for t in response.answers().iter() {
//         for y in t.data().as_srv().iter() {
//             println!("     DNS ==> {:?} : {:?}", y.target().to_utf8(), y.port());
//         }
//     }
// }
