use crate::utils::tools::{Headers, UpstreamsDashMap};
use futures::channel::mpsc::Sender;
use std::collections::HashMap;
use std::time::Duration;

use crate::utils::parceyaml::load_configuration;
use dashmap::DashMap;
use futures::SinkExt;
use hickory_client::client::{Client, ClientHandle};
use hickory_client::proto::rr::{DNSClass, Name, RecordType};
use hickory_client::proto::runtime::TokioRuntimeProvider;
use hickory_client::proto::tcp::TcpClientStream;
use log::info;
use pingora::prelude::sleep;
use rand::Rng;
use serde::Deserialize;
use std::str::FromStr;
use std::sync::atomic::AtomicUsize;

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

pub async fn start(fp: String, mut toreturn: Sender<(UpstreamsDashMap, Headers)>) {
    let config = load_configuration(fp.as_str(), "filepath");
    let headers = DashMap::new();
    // println!("{:?}", config);
    match config {
        Some(config) => {
            let conf: Vec<&str> = config.2.split_whitespace().collect();
            let y = conf.get(0).unwrap();
            if y.to_string() != "consul" {
                info!("Not running Consul discovery, requested type is: {}", config.2);
                return;
            }
            info!("Consul Discovery is enabled : {}", config.2);
            let end = conf.len();
            loop {
                let num = rand::thread_rng().gen_range(1..end);
                sleep(Duration::from_secs(5)).await;
                headers.clear();
                for (k, v) in config.1.clone() {
                    headers.insert(k.to_string(), v);
                }
                let consul = "http://".to_string() + conf.get(num).unwrap();
                let upstreams = http_request(consul, "GET");
                match upstreams.await {
                    Some(upstreams) => {
                        toreturn.send((upstreams, headers.clone())).await.unwrap();
                    }
                    None => {}
                }
            }
        }
        None => {}
    }
}

async fn http_request(url: String, method: &str) -> Option<UpstreamsDashMap> {
    let client = reqwest::Client::new();
    let to = Duration::from_secs(1);
    let upstreams = UpstreamsDashMap::new();
    let excludes = vec!["consul", "nomad", "nomad-client"];
    match method {
        "GET" => {
            let ss = url.clone() + "/v1/catalog/service";
            let response = client.get(ss.clone() + "s").timeout(to).send().await;
            match response {
                Ok(r) => {
                    let json = r.json::<HashMap<String, Vec<String>>>().await;
                    match json {
                        Ok(_j) => {
                            for (k, _v) in _j {
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
        _ => None,
    }
}

async fn get_by_http(url: String) -> Option<DashMap<String, (Vec<(String, u16, bool)>, AtomicUsize)>> {
    let client = reqwest::Client::new();
    let to = Duration::from_secs(1);
    let u = client.get(url.clone()).timeout(to).send();
    let mut values = Vec::new();
    let upstreams: DashMap<String, (Vec<(String, u16, bool)>, AtomicUsize)> = DashMap::new();
    match u.await {
        Ok(r) => {
            let jason = r.json::<Vec<Service>>().await;
            match jason {
                Ok(services) => {
                    for service in services {
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

#[allow(dead_code)]
async fn get_by_dns() {
    let (stream, sender) = TcpClientStream::new(([192, 168, 22, 1], 53).into(), None, None, TokioRuntimeProvider::new());
    let client = Client::new(stream, sender, None);
    let (mut client, bg) = client.await.expect("connection failed");
    tokio::spawn(bg);
    let query = client.query(Name::from_str("_frontend-dev-frontend-srv._tcp.service.consul.").unwrap(), DNSClass::IN, RecordType::SRV);
    // let query = client.query(Name::from_str("matyan.org.").unwrap(), DNSClass::IN, RecordType::A);
    let response = query.await.unwrap();

    for t in response.answers().iter() {
        for y in t.data().as_srv().iter() {
            println!("     DNS ==> {:?} : {:?}", y.target().to_utf8(), y.port());
        }
    }
}
