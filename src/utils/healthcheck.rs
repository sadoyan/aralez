use crate::utils::structs::{InnerMap, UpstreamsDashMap, UpstreamsIdMap};
use crate::utils::tools::*;
use dashmap::DashMap;
use log::{error, warn};
use reqwest::{Client, Version};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tonic::transport::Endpoint;

pub async fn hc2(upslist: Arc<UpstreamsDashMap>, fullist: Arc<UpstreamsDashMap>, idlist: Arc<UpstreamsIdMap>, params: (&str, u64)) {
    let mut period = interval(Duration::from_secs(params.1));
    let client = Client::builder().timeout(Duration::from_secs(params.1)).danger_accept_invalid_certs(true).build().unwrap();
    loop {
        tokio::select! {
            _ = period.tick() => {
                populate_upstreams(&upslist, &fullist, &idlist, params, &client).await;
            }
        }
    }
}

pub async fn populate_upstreams(upslist: &Arc<UpstreamsDashMap>, fullist: &Arc<UpstreamsDashMap>, idlist: &Arc<UpstreamsIdMap>, params: (&str, u64), client: &Client) {
    let totest = build_upstreams(fullist, params.0, client).await;
    if !compare_dashmaps(&totest, upslist) {
        clone_dashmap_into(&totest, upslist);
        clone_idmap_into(&totest, idlist);
    }
}

pub async fn initiate_upstreams(fullist: UpstreamsDashMap) -> UpstreamsDashMap {
    let client = Client::builder().timeout(Duration::from_secs(2)).danger_accept_invalid_certs(true).build().unwrap();
    build_upstreams(&fullist, "HEAD", &client).await
}

async fn build_upstreams(fullist: &UpstreamsDashMap, method: &str, client: &Client) -> UpstreamsDashMap {
    let totest: UpstreamsDashMap = DashMap::new();
    let fclone = clone_dashmap(fullist);
    for val in fclone.iter() {
        let host = val.key();
        let inner = DashMap::new();

        for path_entry in val.value().iter() {
            let path = path_entry.key();
            let mut innervec = Vec::new();

            for (_, upstream) in path_entry.value().0.iter().enumerate() {
                let tls = detect_tls(upstream.address.as_str(), &upstream.port, &client).await;
                let is_h2 = matches!(tls.1, Some(Version::HTTP_2));

                let link = if tls.0 {
                    format!("https://{}:{}{}", upstream.address, upstream.port, path)
                } else {
                    format!("http://{}:{}{}", upstream.address, upstream.port, path)
                };

                let mut scheme = InnerMap {
                    address: upstream.address.clone(),
                    port: upstream.port,
                    is_ssl: tls.0,
                    is_http2: is_h2,
                    to_https: upstream.to_https,
                    rate_limit: upstream.rate_limit,
                    healthcheck: upstream.healthcheck,
                };

                if scheme.healthcheck.unwrap_or(true) {
                    let resp = http_request(&link, method, "", &client).await;
                    if resp.0 {
                        if resp.1 {
                            scheme.is_http2 = is_h2; // could be adjusted further
                        }
                        innervec.push(scheme);
                    } else {
                        warn!("Dead Upstream : {}", link);
                    }
                } else {
                    innervec.push(scheme);
                }

                // let resp = http_request(&link, method, "", &client).await;
                // if resp.0 {
                //     if resp.1 {
                //         scheme.is_http2 = is_h2; // could be adjusted further
                //     }
                //     innervec.push(scheme);
                // } else {
                //     warn!("Dead Upstream : {}", link);
                // }
            }
            inner.insert(path.clone(), (innervec, AtomicUsize::new(0)));
        }
        totest.insert(host.clone(), inner);
    }
    totest
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
