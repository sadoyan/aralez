// use crate::utils::compare;
// use crate::utils::discovery;
use crate::utils::*;
use async_trait::async_trait;
use dashmap::DashMap;
use log::{info, warn};
use pingora::prelude::*;
use pingora_core::prelude::HttpPeer;
use pingora_core::server::ShutdownWatch;
use pingora_core::services::background::BackgroundService;
use pingora_http::{RequestHeader, ResponseHeader};
use pingora_proxy::{ProxyHttp, Session};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::interval;

pub struct LB {
    pub upstreams: Arc<RwLock<DashMap<String, (Vec<(String, u16)>, AtomicUsize)>>>,
    pub umap_full: Arc<RwLock<DashMap<String, (Vec<(String, u16)>, AtomicUsize)>>>,
}
// pub struct BGService {
//     pub upstreams: Arc<RwLock<DashMap<String, (Vec<(String, u16)>, AtomicUsize)>>>,
//     pub umap_full: Arc<RwLock<DashMap<String, (Vec<(String, u16)>, AtomicUsize)>>>,
// }

#[async_trait]
impl BackgroundService for LB {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        tokio::spawn(healthcheck::hc(self.upstreams.clone(), self.umap_full.clone()));

        println!("Starting example background service");
        let mut period = interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    break;
                }
                _ = period.tick() => {
                    let umap_work = self.upstreams.write().await;
                    let umap_full = self.umap_full.write().await;
                    let newmap = discovery::discover();
                    if !compare::dashmaps(&umap_full, &newmap) {
                        println!("DashMaps are different. Syncing !!!!!");
                        for (k,v) in newmap {
                            let mut o= Vec::new();
                            println!("{} -> {:?}", k, v);
                            for k in v.0.clone() {
                                o.push(k);
                            }
                            umap_work.insert(k.clone(),v);
                            umap_full.insert(k,(o,AtomicUsize::new(0)));
                        }
                    }
                    drop(umap_full);
                    drop(umap_work);
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
        let map_read = self.upstreams.read().await;
        // let ful_read = self.umap_full.read().await;
        println!("DN ==> {:?}", map_read);
        // println!("FU ==> {:?}", ful_read);
        let x = if let Some(entry) = map_read.get(peer) {
            let (servers, index) = entry.value(); // No clone here

            if servers.is_empty() {
                return None;
            }
            let idx = index.fetch_add(1, Ordering::Relaxed) % servers.len();
            // println!("{} {:?} => len: {}, idx: {}", peer, servers[idx], servers.len(), idx);
            Some(servers[idx].clone())
        } else {
            None
        };
        drop(map_read);
        x
    }
}

#[async_trait]
impl ProxyHttp for LB {
    type CTX = ();
    fn new_ctx(&self) -> Self::CTX {}
    async fn upstream_peer(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<Box<HttpPeer>> {
        let host_name = session.req_header().headers.get("host");
        let ddr = self.get_host(host_name.unwrap().to_str().unwrap());
        match ddr.await {
            Some((host, port)) => {
                let peer = Box::new(HttpPeer::new((host, port), false, "".to_string()));
                Ok(peer)
            }
            None => {
                println!("Returning default list => {:?}", ("127.0.0.1", 8000));
                let peer = Box::new(HttpPeer::new(("127.0.0.1", 8000), false, "".to_string()));
                Ok(peer)
            }
        }
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
