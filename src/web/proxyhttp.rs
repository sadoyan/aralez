use crate::utils::discovery::{APIUpstreamProvider, Discovery, FromFileProvider};
use crate::utils::tools::*;
use crate::utils::*;
use async_trait::async_trait;
use dashmap::DashMap;
use futures::channel::mpsc;
use futures::StreamExt;
use log::{debug, error, info, warn};
use pingora::prelude::*;
use pingora_core::prelude::HttpPeer;
use pingora_core::server::ShutdownWatch;
use pingora_core::services::background::BackgroundService;
use pingora_http::{RequestHeader, ResponseHeader};
use pingora_proxy::{ProxyHttp, Session};
use std::ops::Deref;
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub struct LB {
    pub ump_upst: Arc<UpstreamsDashMap>,
    pub ump_full: Arc<UpstreamsDashMap>,
    pub config: Arc<DashMap<String, String>>,
    pub local: Arc<(String, u16)>,
}

#[async_trait]
impl BackgroundService for LB {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        info!("Starting background service");
        let (tx, mut rx) = mpsc::channel::<UpstreamsDashMap>(0);

        let from_file = self.config.get("upstreams_conf");
        match from_file {
            Some(from_file) => {
                let file_load = FromFileProvider { path: from_file.to_string() };
                let tx_file = tx.clone();
                let _ = tokio::spawn(async move { file_load.start(tx_file).await });
            }
            None => {
                error!("Can't read config file");
            }
        }

        let config_address = self.config.get("config_address");
        match config_address {
            Some(config_address) => {
                let api_load = APIUpstreamProvider {
                    address: config_address.to_string(),
                };
                let tx_api = tx.clone();
                let _ = tokio::spawn(async move { api_load.start(tx_api).await });
            }
            None => {
                error!("Can't read config file");
            }
        }

        let uu = self.ump_upst.clone();
        let ff = self.ump_full.clone();
        let _ = tokio::spawn(async move { healthcheck::hc2(uu, ff).await });

        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    break;
                }
                val = rx.next() => {
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
    async fn get_host(&self, peer: &str, path: &str, _upgrade: bool) -> Option<(String, u16, bool, String)> {
        let _proto = "";
        // if upgrade {
        //     _proto = "wsoc";
        // } else {
        //     _proto = "http"
        // }
        let host_entry = self.ump_upst.get(peer);
        match host_entry {
            Some(host_entry) => {
                let upstream = if let Some(entry) = host_entry.get(path) {
                    let (servers, index) = entry.value();
                    if servers.is_empty() {
                        return None;
                    }
                    let idx = index.fetch_add(1, Ordering::Relaxed) % servers.len();
                    // info!("Host: {}, Path: {}, Peer: {:?},  len: {}, idx: {}", peer, path, servers[idx], servers.len(), idx);
                    Some(servers[idx].clone())
                } else {
                    None
                };
                upstream
            }
            None => None,
        }
    }
}

#[async_trait]
impl ProxyHttp for LB {
    type CTX = ();
    fn new_ctx(&self) -> Self::CTX {}
    async fn upstream_peer(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<Box<HttpPeer>> {
        let host_name = session.req_header().headers.get("host");
        match host_name {
            Some(host) => {
                let header_host = host.to_str().unwrap().split(':').collect::<Vec<&str>>();
                let ddr = self.get_host(header_host[0], session.req_header().uri.path(), session.is_upgrade_req());
                match ddr.await {
                    Some((host, port, ssl, _proto)) => {
                        let peer = Box::new(HttpPeer::new((host, port), ssl, String::new()));
                        Ok(peer)
                    }
                    None => {
                        warn!("Upstream not found. Host: {:?}, Path: {}", host, session.req_header().uri);
                        let peer = Box::new(HttpPeer::new(self.local.deref(), false, String::new()));
                        Ok(peer)
                    }
                }
            }
            None => {
                warn!("Upstream not found. Host: {:?}, Path: {}", host_name, session.req_header().uri);
                let peer = Box::new(HttpPeer::new(self.local.deref(), false, String::new()));
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

    async fn response_filter(&self, _session: &mut Session, _upstream_response: &mut ResponseHeader, _ctx: &mut Self::CTX) -> Result<()>
    where
        Self::CTX: Send + Sync,
    {
        // -------------------------------------------------------------------------------------------- //
        // let header_host = _session.req_header().headers.get("host");
        // match header_host {
        //     Some(header_host) => {
        //         let data = self.ump_full.get(header_host.to_str().unwrap()).unwrap();
        //         let detail = data.get(_session.req_header().uri.path());
        //         let thelast = detail.unwrap().value().0[0].clone().3;
        //         println!("        Host ==> {}", thelast);
        //     }
        //     None => {}
        // }
        // _upstream_response.insert_header("X-DoSome-About", "Yaaaaaaaaaaaaaaa").unwrap();
        // -------------------------------------------------------------------------------------------- //

        _upstream_response.insert_header("X-Proxied-From", "Fooooooooooooooo").unwrap();

        Ok(())
    }

    async fn logging(&self, session: &mut Session, _e: Option<&pingora::Error>, ctx: &mut Self::CTX) {
        let response_code = session.response_written().map_or(0, |resp| resp.status.as_u16());
        debug!("{}, response code: {response_code}", self.request_summary(session, ctx));
    }
}
