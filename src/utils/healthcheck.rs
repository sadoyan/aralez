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
    let client = Client::builder().timeout(Duration::from_secs(2)).danger_accept_invalid_certs(true).build().unwrap();
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
                            let tls = detect_tls(k.1.address.as_str(), &k.1.port, &client).await;
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
                                rate_limit: k.1.rate_limit,
                            };
                            let resp = http_request(_link.as_str(), params.0, "", &client).await;
                            match resp.0 {
                                true => {
                                    if resp.1 {
                                      scheme = InnerMap {
                                        address: k.1.address.clone(),
                                        port: k.1.port,
                                        is_ssl: tls.0,
                                        is_http2: is_h2,
                                        to_https: k.1.to_https,
                                        rate_limit: k.1.rate_limit,
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

async fn http_request(url: &str, method: &str, payload: &str, client: &Client) -> (bool, bool) {
    if !["POST", "GET", "HEAD"].contains(&method) {
        error!("Method {} not supported. Only GET|POST|HEAD are supported ", method);
        return (false, false);
    }
    async fn send_request(client: &Client, method: &str, url: &str, payload: &str) -> Option<reqwest::Response> {
        match method {
            "POST" => client.post(url).body(payload.to_owned()).send().await.ok(),
            "GET" => client.get(url).send().await.ok(),
            "HEAD" => client.head(url).send().await.ok(),
            _ => None,
        }
    }

    match send_request(&client, method, url, payload).await {
        Some(response) => {
            let status = response.status().as_u16();
            ((99..499).contains(&status), false)
        }
        None => (ping_grpc(&url).await, true),
    }
}

pub async fn ping_grpc(addr: &str) -> bool {
    let endpoint_result = Endpoint::from_shared(addr.to_owned());

    if let Ok(endpoint) = endpoint_result {
        let endpoint = endpoint.timeout(Duration::from_secs(2));

        match tokio::time::timeout(Duration::from_secs(3), endpoint.connect()).await {
            Ok(Ok(_channel)) => true,
            _ => false,
        }
    } else {
        false
    }
}

async fn detect_tls(ip: &str, port: &u16, client: &Client) -> (bool, Option<Version>) {
    let https_url = format!("https://{}:{}", ip, port);
    match client.get(&https_url).send().await {
        Ok(response) => {
            // println!("{} => {:?} (HTTPS)", https_url, response.version());
            return (true, Some(response.version()));
        }
        _ => {}
    }
    let http_url = format!("http://{}:{}", ip, port);
    match client.get(&http_url).send().await {
        Ok(response) => {
            // println!("{} => {:?} (HTTP)", http_url, response.version());
            (false, Some(response.version()))
        }
        Err(_) => {
            if ping_grpc(&http_url).await {
                (false, Some(Version::HTTP_2))
            } else {
                (false, None)
            }
        }
    }
}
