use crate::utils::hcclient::httpclient;
use crate::utils::lazylock::REVERSE_STORE;
use crate::utils::structs::{InnerMap, UpstreamsDashMap, UpstreamsIdMap};
use crate::utils::tools::*;
use dashmap::DashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::{interval, timeout};
use tokio_native_tls::native_tls::TlsConnector;

pub async fn hc2(upslist: Arc<UpstreamsDashMap>, fullist: Arc<UpstreamsDashMap>, idlist: Arc<UpstreamsIdMap>, params: (&str, u64)) {
    let mut period = interval(Duration::from_secs(params.1));
    loop {
        tokio::select! {
            _ = period.tick() => {
                let totest = build_upstreams(&fullist, params.0).await;
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
    build_upstreams(&fullist, "HEAD").await
}

async fn build_upstreams(fullist: &UpstreamsDashMap, method: &str) -> UpstreamsDashMap {
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
                    detect_tls(upstream.address.as_ref(), &upstream.port, host).await
                } else {
                    false
                };
                let mut scheme = InnerMap {
                    address: upstream.address.clone(),
                    port: upstream.port,
                    is_ssl: tls,
                    is_http2: false,
                    to_https: upstream.to_https,
                    rate_limit: upstream.rate_limit,
                    x4xx_limit: upstream.x4xx_limit,
                    healthcheck: upstream.healthcheck,
                    redirect_to: upstream.redirect_to.clone(),
                    authorization: upstream.authorization.clone(),
                };

                if scheme.healthcheck.unwrap_or(true) {
                    let link = if tls {
                        format!("https://{}:{}{}", upstream.address, upstream.port, path)
                    } else {
                        format!("http://{}:{}{}", upstream.address, upstream.port, path)
                    };
                    let resp = httpclient(method, tls, host, path, upstream.address.as_ref(), upstream.port, link).await;
                    if resp.0 {
                        if resp.1 {
                            scheme.is_http2 = resp.1;
                        }
                        innervec.push(Arc::from(scheme));
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

async fn detect_tls(ip: &str, port: &u16, sni: &str) -> bool {
    let addr = format!("{ip}:{port}");
    let timeout_duration = Duration::from_secs(2); // THINK !!!
    let stream = match timeout(timeout_duration, TcpStream::connect(&addr)).await {
        Ok(Ok(s)) => s,
        Ok(Err(_)) => return false,
        Err(_) => return false,
    };
    let connector = tokio_native_tls::TlsConnector::from(
        TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
            .build()
            .unwrap(),
    );
    connector.connect(sni, stream).await.is_ok()
}
