use crate::utils::compare;
// use crate::utils::tools::*;
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
    pub upstreams_map: Arc<RwLock<DashMap<String, (Vec<(String, u16)>, AtomicUsize)>>>,
}
pub struct BGService {
    pub upstreams_map: Arc<RwLock<DashMap<String, (Vec<(String, u16)>, AtomicUsize)>>>,
}

#[async_trait]
impl BackgroundService for BGService {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        println!("Starting example background service");
        let mut period = interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    break;
                }
                _ = period.tick() => {
                    let map_write = self.upstreams_map.write().await;
                    let newmap = discover();
                    if compare::dashmaps(&map_write, &newmap) {
                        println!("DashMaps are equal. Chilling out.");
                    } else {
                        println!("DashMaps are different. Syncing !!!!!");
                        for (k,v) in newmap {
                            println!("{} -> {:?}", k, v);
                            map_write.insert(k,v);
                        }
                    }
                    drop(map_write); // Important: Release the lock
                }
            }
        }
    }
}

fn discover() -> DashMap<String, (Vec<(String, u16)>, AtomicUsize)> {
    let upstreams_map: DashMap<String, (Vec<(String, u16)>, AtomicUsize)> = DashMap::new();
    let mut toreturn = vec![];
    toreturn.push(("192.168.1.1".to_string(), 8000.to_owned()));
    toreturn.push(("192.168.1.10".to_string(), 8000.to_owned()));
    toreturn.push(("127.0.0.1".to_string(), 8000.to_owned()));
    toreturn.push(("127.0.0.2".to_string(), 8000.to_owned()));
    toreturn.push(("127.0.0.3".to_string(), 8000.to_owned()));
    toreturn.push(("127.0.0.4".to_string(), 8000.to_owned()));
    toreturn.push(("127.0.0.5".to_string(), 8000.to_owned()));
    toreturn.push(("127.0.0.6".to_string(), 8000.to_owned()));
    upstreams_map.insert("myip.netangels.net".to_string(), (toreturn, AtomicUsize::new(0)));
    let mut toreturn = vec![];
    toreturn.push(("192.168.1.1".to_string(), 8000.to_owned()));
    toreturn.push(("192.168.1.10".to_string(), 8000.to_owned()));
    upstreams_map.insert("polo.netangels.net".to_string(), (toreturn, AtomicUsize::new(0)));
    let mut toreturn = vec![];
    toreturn.push(("192.168.1.20".to_string(), 8000.to_owned()));
    upstreams_map.insert("glop.netangels.net".to_string(), (toreturn, AtomicUsize::new(0)));
    upstreams_map
}

#[async_trait]
pub trait GetHost {
    async fn get_host(&self, peer: &str) -> Option<(String, u16)>;
}
#[async_trait]
impl GetHost for LB {
    async fn get_host(&self, peer: &str) -> Option<(String, u16)> {
        let map_read = self.upstreams_map.read().await;
        let x = if let Some(entry) = map_read.get(peer) {
            let (servers, index) = entry.value(); // No clone here

            if servers.is_empty() {
                return None;
            }
            let idx = index.fetch_add(1, Ordering::Relaxed) % servers.len();
            println!("{} {:?} => len: {}, idx: {}", peer, servers[idx], servers.len(), idx);
            Some(servers[idx].clone()) // Clone the server address
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
