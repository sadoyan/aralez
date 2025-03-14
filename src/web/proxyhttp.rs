use crate::utils::discovery::{APIUpstreamProvider, Discovery, FromFileProvider};
use crate::utils::tools::*;
use crate::utils::*;
use async_trait::async_trait;
use futures::channel::mpsc;
use futures::StreamExt;
use log::{info, warn};
use pingora::prelude::*;
use pingora_core::prelude::HttpPeer;
use pingora_core::server::ShutdownWatch;
use pingora_core::services::background::BackgroundService;
use pingora_http::{RequestHeader, ResponseHeader};
use pingora_proxy::{ProxyHttp, Session};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
// use tokio::time::Instant;

pub struct LB {
    pub upstreams: Arc<UpstreamMap>,
    pub umap_full: Arc<UpstreamMap>,
}

#[async_trait]
impl BackgroundService for LB {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        println!("Starting example background service");

        let (tx, mut rx) = mpsc::channel::<UpstreamMap>(0);
        let file_load = FromFileProvider {
            path: "etc/upstreams.conf".to_string(),
        };

        let api_load = APIUpstreamProvider;

        let tx_file = tx.clone();
        let tx_api = tx.clone();
        let _ = tokio::spawn(async move { api_load.run(tx_api).await });
        let _ = tokio::spawn(async move { file_load.run(tx_file).await });
        let up = self.upstreams.clone();
        let fu = self.umap_full.clone();
        let _ = tokio::spawn(async move { healthcheck::hc(up, fu).await });

        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    break;
                }
                val = rx.next() => {
                    match val {
                        Some(newmap) => {
                            match compare::dm(&self.umap_full, &newmap) {
                                false => {
                                    self.upstreams.clear();
                                    self.umap_full.clear();
                                    for (k,v) in newmap {
                                        println!("Host: {}", k);
                                        // <UpstreamMap
                                        for vv in v.0.clone() {
                                            println!("   ===> {:?}", vv);
                                        }
                                        self.upstreams.insert(k.clone(), (v.0.clone(), AtomicUsize::new(0))); // No need for extra vec!
                                        self.umap_full.insert(k, (v.0, AtomicUsize::new(0))); // Use `value.0` directly
                                    }
                                }
                                true => {
                                }
                            }
                        }
                        None => {}
                    }
                }
            }
        }
    }
}

#[async_trait]
pub trait GetHost {
    async fn get_host(&self, peer: &str) -> Option<(String, u16)>;
}
#[async_trait]
impl GetHost for LB {
    async fn get_host(&self, peer: &str) -> Option<(String, u16)> {
        let x = if let Some(entry) = self.upstreams.get(peer) {
            let (servers, index) = entry.value();
            if servers.is_empty() {
                return None;
            }
            let idx = index.fetch_add(1, Ordering::Relaxed) % servers.len();
            println!("{} {:?} => len: {}, idx: {}", peer, servers[idx], servers.len(), idx);
            Some(servers[idx].clone())
        } else {
            None
        };
        x
    }
}

#[async_trait]
impl ProxyHttp for LB {
    type CTX = ();
    fn new_ctx(&self) -> Self::CTX {}
    async fn upstream_peer(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<Box<HttpPeer>> {
        // let before = Instant::now();
        let host_name = session.req_header().headers.get("host");
        match host_name {
            Some(host) => {
                let h = host.to_str().unwrap().split(':').collect::<Vec<&str>>();
                let ddr = self.get_host(h[0]);
                match ddr.await {
                    Some((host, port)) => {
                        let peer = Box::new(HttpPeer::new((host, port), false, String::new()));
                        // info!("{:?}, Time => {:.2?}", session.request_summary(), before.elapsed());
                        Ok(peer)
                    }
                    None => {
                        warn!("Returning default list => {:?}", ("127.0.0.1", 3000));
                        let peer = Box::new(HttpPeer::new(("127.0.0.1", 3000), false, String::new()));
                        // info!("{:?}, Time => {:.2?}", session.request_summary(), before.elapsed());
                        Ok(peer)
                    }
                }
            }
            None => {
                warn!("Returning default list => {:?}", ("127.0.0.1", 3000));
                let peer = Box::new(HttpPeer::new(("127.0.0.1", 3000), false, String::new()));
                // info!("{:?}, Time => {:.2?}", session.request_summary(), before.elapsed());
                Ok(peer)
            }
        }
        /*
        let ddr = self.get_host(host_name.unwrap().to_str().unwrap());
        match ddr.await {
            Some((host, port)) => {
                let peer = Box::new(HttpPeer::new((host, port), false, String::new()));
                Ok(peer)
            }
            None => {
                println!("Returning default list => {:?}", ("127.0.0.1", 3000));
                let peer = Box::new(HttpPeer::new(("127.0.0.1", 3000), false, String::new()));
                Ok(peer)
            }
        }
        */
    }
    async fn request_filter(&self, _session: &mut Session, _ctx: &mut Self::CTX) -> pingora_core::Result<bool>
    where
        Self::CTX: Send + Sync,
    {
        // if !_session.req_header().uri.path().starts_with("/ping") {
        if _session.req_header().uri.path().starts_with("/denied") {
            let _ = _session.respond_error(403).await;
            info!("Forbidded: {:?}, {}", _session.client_addr(), _session.req_header().uri.path().to_string());
            return Ok(true);
        };
        Ok(false)
    }
    async fn upstream_request_filter(&self, _session: &mut Session, _upstream_request: &mut RequestHeader, _ctx: &mut Self::CTX) -> pingora_core::Result<()>
    where
        Self::CTX: Send + Sync,
    {
        let clientip = _session.client_addr();
        match clientip {
            Some(ip) => {
                let inet = ip.as_inet();
                match inet {
                    Some(addr) => {
                        _upstream_request
                            .insert_header("X-Forwarded-For", addr.to_string().split(':').collect::<Vec<&str>>()[0])
                            .unwrap();
                    }
                    None => warn!("Malformed Client IP: {:?}", inet),
                }
            }
            None => {
                warn!("Cannot detect client IP");
            }
        }
        Ok(())
    }
    async fn response_filter(&self, _session: &mut Session, _upstream_response: &mut ResponseHeader, _ctx: &mut Self::CTX) -> pingora_core::Result<()>
    where
        Self::CTX: Send + Sync,
    {
        _upstream_response.insert_header("X-Proxied-From", "Fooooooooooooooo").unwrap();
        Ok(())
    }
    // async fn logging(&self, session: &mut Session, _e: Option<&pingora::Error>, ctx: &mut Self::CTX) {
    //     let response_code = session.response_written().map_or(0, |resp| resp.status.as_u16());
    //     info!("{}, response code: {response_code}", self.request_summary(session, ctx));
    // }
}
