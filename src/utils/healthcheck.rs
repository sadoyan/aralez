use crate::utils::grpc::ping_grpc;
use crate::utils::lazylock::REVERSE_STORE;
use crate::utils::structs::{InnerMap, UpstreamsDashMap, UpstreamsIdMap};
use crate::utils::tools::*;
use dashmap::DashMap;
use log::{error, warn};
use reqwest::{Client, Version};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::interval;
use tokio_native_tls::native_tls::TlsConnector;

pub async fn hc2(upslist: Arc<UpstreamsDashMap>, fullist: Arc<UpstreamsDashMap>, idlist: Arc<UpstreamsIdMap>, params: (&str, u64)) {
    let mut period = interval(Duration::from_secs(params.1));
    let client = Client::builder().timeout(Duration::from_secs(params.1)).danger_accept_invalid_certs(true).build().unwrap();
    loop {
        tokio::select! {
            _ = period.tick() => {
                // populate_upstreams(&upslist, &fullist, &idlist, params, &client).await;
                let totest = build_upstreams(&fullist, params.0, &client).await;
                if !compare_dashmaps(&totest, &upslist) {
                    clone_dashmap_into(&totest, &upslist);
                    clone_idmap_into(&totest, &idlist);
                    REVERSE_STORE.clear();
                }
            }
        }
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

            for upstream in path_entry.value().0.iter() {
                let tls = if upstream.healthcheck.unwrap_or(true) {
                    detect_tls(upstream.address.as_ref(), &upstream.port, client).await
                } else {
                    (false, None)
                };
                let mut scheme = InnerMap {
                    address: upstream.address.clone(),
                    port: upstream.port,
                    is_ssl: tls.0,
                    is_http2: matches!(tls.1, Some(Version::HTTP_2)),
                    to_https: upstream.to_https,
                    rate_limit: upstream.rate_limit,
                    x4xx_limit: upstream.x4xx_limit,
                    healthcheck: upstream.healthcheck,
                    redirect_to: upstream.redirect_to.clone(),
                    authorization: upstream.authorization.clone(),
                };

                if scheme.healthcheck.unwrap_or(true) {
                    let link = if tls.0 {
                        format!("https://{}:{}{}", upstream.address, upstream.port, path)
                    } else {
                        format!("http://{}:{}{}", upstream.address, upstream.port, path)
                    };

                    let resp = http_request(&link, method, "", client).await;

                    if resp.0 {
                        if resp.1 {
                            scheme.is_http2 = resp.1;
                        }
                        innervec.push(Arc::from(scheme));
                    } else {
                        warn!("Dead Upstream : {}", link);
                    }
                } else {
                    innervec.push(Arc::from(scheme));
                }
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

    match send_request(client, method, url, payload).await {
        Some(response) => {
            let status = response.status().as_u16();
            ((99..499).contains(&status), false)
        }
        None => (ping_grpc(url).await, true),
    }
}

async fn detect_tls(ip: &str, port: &u16, client: &Client) -> (bool, Option<Version>) {
    let addr = format!("{}:{}", ip, port);
    let Ok(stream) = TcpStream::connect(&addr).await else {
        return (false, Some(Version::HTTP_11));
    };

    let connector = tokio_native_tls::TlsConnector::from(
        TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
            .build()
            .unwrap(),
    );

    let tls = connector.connect(ip, stream).await.is_ok();
    if tls {
        let vers = detect_version(ip, *port, true, client).await;
        return (tls, vers);
    }
    (tls, Some(Version::HTTP_11))
}

async fn detect_version(ip: &str, port: u16, is_tls: bool, client: &Client) -> Option<Version> {
    let scheme = if is_tls { "https" } else { "http" };
    let url = format!("{}://{}:{}", scheme, ip, port);
    match client.get(&url).send().await.ok().map(|r| r.version()) {
        Some(version) => Some(version),
        None => Some(Version::HTTP_11),
    }
}
