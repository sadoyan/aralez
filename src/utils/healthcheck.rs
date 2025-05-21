use crate::utils::structs::{UpstreamsDashMap, UpstreamsIdMap};
use crate::utils::tools::*;
use dashmap::DashMap;
use log::{error, info, warn};
use reqwest::{Client, Version};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tonic::transport::Endpoint;

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
                    let mut _scheme: (String, u16, bool, bool) = ("".to_string(), 0, false, false);
                    for path_entry in val.value().iter() {
                        // let inner = DashMap::new();
                        let path = path_entry.key();
                        let mut innervec= Vec::new();
                        for k in path_entry.value().0 .iter().enumerate() {
                            let (ip, port, _ssl, _version) = k.1;
                            let mut _link = String::new();
                            let tls = detect_tls(ip, port).await;
                            let mut is_h2 = false;

                            // if tls.1 == Some(Version::HTTP_11) {
                            //     println!("  V1: ==> {:?}", tls.1)
                            // }else if tls.1 == Some(Version::HTTP_2) {
                            //     is_h2 = true;
                            //     println!("  V2: ==> {:?}", tls.1)
                            // }

                            if tls.1 == Some(Version::HTTP_2) {
                                is_h2 = true;
                                // println!("  V2: ==> {} ==> {:?}", tls.0, tls.1)
                            }

                            match tls.0 {
                                true => _link = format!("https://{}:{}{}", ip, port, path),
                                false => _link = format!("http://{}:{}{}", ip, port, path),
                            }
                            // if _pref == "https://" {
                            //     _scheme = (ip.to_string(), *port, true);
                            // }else {
                            //     _scheme = (ip.to_string(), *port, false);
                            // }
                            _scheme = (ip.to_string(), *port, tls.0, is_h2);
                            // let link = format!("{}{}:{}{}", _pref, ip, port, path);
                            let resp = http_request(_link.as_str(), params.0, "").await;
                            match resp.0 {
                                true => {
                                    if resp.1 {
                                        _scheme = (ip.to_string(), *port, tls.0, true);
                                    }
                                    innervec.push(_scheme.clone());
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
                    info!("Gazan is up and ready to serve requests, the upstreams list is:");
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
