use crate::utils::structs::{InnerMap, UpstreamsDashMap, UpstreamsIdMap};
use crate::utils::tools::*;
use dashmap::DashMap;
use log::{error, info, warn};
use reqwest::{Client, Version};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tonic::transport::Endpoint;

#[allow(unused_assignments)]
pub async fn hc2(upslist: Arc<UpstreamsDashMap>, fullist: Arc<UpstreamsDashMap>, idlist: Arc<UpstreamsIdMap>, params: (&str, u64)) {
    let mut period = interval(Duration::from_secs(params.1));
    let mut first_run = 0;
    loop {
        tokio::select! {
            _ = period.tick() => {
                let totest : UpstreamsDashMap = DashMap::new();
                let fclone : UpstreamsDashMap = clone_dashmap(&fullist);
                for val in fclone.iter() {
                    let host = val.key();
                    let inner = DashMap::new();
                    let mut scheme = InnerMap::new();
                    for path_entry in val.value().iter() {
                        let path = path_entry.key();
                        let mut innervec= Vec::new();
                        for k in path_entry.value().0 .iter().enumerate() {
                            let mut _link = String::new();
                            let tls = detect_tls(k.1.address.as_str(), &k.1.port).await;
                            let mut is_h2 = false;
                            if tls.1 == Some(Version::HTTP_2) {
                                is_h2 = true;
                            }
                            match tls.0 {
                                true => _link = format!("https://{}:{}{}", k.1.address, k.1.port, path),
                                false => _link = format!("http://{}:{}{}", k.1.address, k.1.port, path),
                            }
                            scheme = InnerMap {
                                address: k.1.address.clone(),
                                port: k.1.port,
                                is_ssl: tls.0,
                                is_http2: is_h2,
                                to_https: k.1.to_https,
                            };
                            let resp = http_request(_link.as_str(), params.0, "").await;
                            match resp.0 {
                                true => {
                                    if resp.1 {
                                      scheme = InnerMap {
                                        address: k.1.address.clone(),
                                        port: k.1.port,
                                        is_ssl: tls.0,
                                        is_http2: is_h2,
                                        to_https: k.1.to_https,
                                        };
                                    }
                                    innervec.push(scheme);
                                }
                                false => {
                                    warn!("Dead Upstream : {}", _link);
                                }
                            }
                        }
                        inner.insert(path.clone().to_owned(), (innervec, AtomicUsize::new(0)));
                    }
                    totest.insert(host.clone(), inner);
                }

                if first_run == 1 {
                    info!("Performing initial hatchecks and upstreams ssl detection");
                    clone_idmap_into(&totest, &idlist);
                    info!("Aralez is up and ready to serve requests, the upstreams list is:");
                    print_upstreams(&totest)
                }

                first_run+=1;

                if ! compare_dashmaps(&totest, &upslist){
                    clone_dashmap_into(&totest, &upslist);
                    clone_idmap_into(&totest, &idlist);
                }

            }
        }
    }
}

#[allow(dead_code)]
async fn http_request(url: &str, method: &str, payload: &str) -> (bool, bool) {
    let client = Client::builder().danger_accept_invalid_certs(true).build().unwrap();
    let timeout = Duration::from_secs(1);
    if !["POST", "GET", "HEAD"].contains(&method) {
        error!("Method {} not supported. Only GET|POST|HEAD are supported ", method);
        return (false, false);
    }
    async fn send_request(client: &Client, method: &str, url: &str, payload: &str, timeout: Duration) -> Option<reqwest::Response> {
        match method {
            "POST" => client.post(url).body(payload.to_owned()).timeout(timeout).send().await.ok(),
            "GET" => client.get(url).timeout(timeout).send().await.ok(),
            "HEAD" => client.head(url).timeout(timeout).send().await.ok(),
            _ => None,
        }
    }

    match send_request(&client, method, url, payload, timeout).await {
        Some(response) => {
            let status = response.status().as_u16();
            ((99..499).contains(&status), false)
        }
        None => {
            // let fallback_url = url.replace("https", "http");
            // ping_grpc(&fallback_url).await
            (ping_grpc(&url).await, true)
        }
    }
}

pub async fn ping_grpc(addr: &str) -> bool {
    let endpoint_result = Endpoint::from_shared(addr.to_owned());

    if let Ok(endpoint) = endpoint_result {
        let endpoint = endpoint.timeout(Duration::from_secs(2));

        match tokio::time::timeout(Duration::from_secs(3), endpoint.connect()).await {
            Ok(Ok(_channel)) => {
                // println!("{:?} ==> {:?} ==> {}", endpoint, _channel, addr);
                true
            }
            _ => false,
        }
    } else {
        false
    }
}

async fn detect_tls(ip: &str, port: &u16) -> (bool, Option<Version>) {
    let url = format!("https://{}:{}", ip, port);
    // let url = format!("{}:{}", ip, port);
    let client = Client::builder().timeout(Duration::from_secs(2)).danger_accept_invalid_certs(true).build().unwrap();
    match client.get(&url).send().await {
        Ok(response) => (true, Some(response.version())),
        Err(e) => {
            if e.is_builder() || e.is_connect() || e.to_string().contains("tls") {
                (false, None)
            } else {
                (false, None)
            }
        }
    }
}
