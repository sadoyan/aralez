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
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub struct LB {
    pub ump_upst: Arc<UpstreamsDashMap>,
    pub ump_full: Arc<UpstreamsDashMap>,
}

#[async_trait]
impl BackgroundService for LB {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        println!("Starting example background service");
        let (tux, mut rux) = mpsc::channel::<UpstreamsDashMap>(0);
        let file_load2 = FromFileProvider {
            path: "etc/upstreams.yaml".to_string(),
        };

        let api_load = APIUpstreamProvider;

        let tux_file = tux.clone();
        let tux_api = tux.clone();
        let _ = tokio::spawn(async move { file_load2.start(tux_file).await });
        let _ = tokio::spawn(async move { api_load.start(tux_api).await });
        let uu = self.ump_upst.clone();
        let ff = self.ump_full.clone();
        let _ = tokio::spawn(async move { healthcheck::hc2(uu, ff).await });

        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    break;
                }
                val = rux.next() => {
                    match val {
                        Some(ss) => {
                            let foo = compare_dashmaps(&*self.ump_full, &ss);
                            if !foo {
                                clone_dashmap_into(&ss, &self.ump_full);
                                clone_dashmap_into(&ss, &self.ump_upst);
                                print_upstreams(&self.ump_full);
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
    async fn get_host(&self, peer: &str, path: &str, upgrade: bool) -> Option<(String, u16, bool, String)>;
}
#[async_trait]
impl GetHost for LB {
    async fn get_host(&self, peer: &str, path: &str, upgrade: bool) -> Option<(String, u16, bool, String)> {
        let mut _proto = "";
        if upgrade {
            _proto = "wsoc";
        } else {
            _proto = "http"
        }
        let host_entry = self.ump_upst.get(peer).unwrap();
        let x = if let Some(entry) = host_entry.get(path) {
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
                let header_host = host.to_str().unwrap().split(':').collect::<Vec<&str>>();

                let ddr = self.get_host(header_host[0], session.req_header().uri.path(), session.is_upgrade_req());
                match ddr.await {
                    Some((host, port, ssl, _proto)) => {
                        let peer = Box::new(HttpPeer::new((host, port), ssl, String::new()));
                        // info!("{:?}, Time => {:.2?}", session.request_summary(), before.elapsed());
                        Ok(peer)
                    }
                    None => {
                        warn!("Returning default list => {:?}, {:?}", host_name, session.req_header().uri);
                        let peer = Box::new(HttpPeer::new(("127.0.0.1", 3000), false, String::new()));
                        // info!("{:?}, Time => {:.2?}", session.request_summary(), before.elapsed());
                        Ok(peer)
                    }
                }
            }
            None => {
                warn!("Returning default list => {:?}, {:?}", host_name, session.req_header().uri);
                let peer = Box::new(HttpPeer::new(("127.0.0.1", 3000), false, String::new()));
                // info!("{:?}, Time => {:.2?}", session.request_summary(), before.elapsed());
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
