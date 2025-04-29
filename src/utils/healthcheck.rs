use crate::utils::tools::*;
use dashmap::DashMap;
use log::{error, info, warn};
use reqwest::Client;
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
                    for path_entry in val.value().iter() {
                        // let inner = DashMap::new();
                        let path = path_entry.key();
                        let mut innervec= Vec::new();
                        for k in path_entry.value().0.iter().enumerate() {
                            let (ip, port, ssl) = k.1;
                            let mut _pref = "";
                            match ssl {
                                true => _pref = "https://",
                                false => _pref = "http://",
                            }
                            let link = format!("{}{}:{}{}", _pref, ip, port, path);
                            let resp = http_request(link.as_str(), params.0, "").await;
                            match resp {
                                true => {
                                    innervec.push(k.1.clone());
                                }
                                false => {
                                    warn!("Dead Upstream : {}", link);
                                }
                            }
                        }
                        inner.insert(path.clone().to_owned(), (innervec, AtomicUsize::new(0)));
                    }
                    totest.insert(host.clone(), inner);
                }

                if first_run == 1 {
                    info!("Synchronising inner hashmaps");
                    clone_idmap_into(&totest, &idlist);
                }

                first_run+=1;

                if ! compare_dashmaps(&totest, &upslist){
                    clone_dashmap_into(&totest, &upslist);
                    clone_idmap_into(&totest, &idlist);
                }
                // print!("{:?}", idlist);
            }
        }
    }
}

#[allow(dead_code)]
async fn http_request(url: &str, method: &str, payload: &str) -> bool {
    let client = Client::builder().danger_accept_invalid_certs(true).build().unwrap();
    let timeout = Duration::from_secs(1);
    if !["POST", "GET", "HEAD"].contains(&method) {
        error!("Method {} not supported. Only GET|POST|HEAD are supported ", method);
        return false;
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
            (99..499).contains(&status)
        }
        None => {
            let fallback_url = url.replace("https", "http");
            ping_grpc(&fallback_url).await
        }
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
