use crate::utils::discovery::{APIUpstreamProvider, ConsulProvider, Discovery, FromFileProvider};
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
    pub headers: Arc<Headers>,
    pub config: Arc<DashMap<String, String>>,
    pub local: Arc<(String, u16)>,
}

#[async_trait]
impl BackgroundService for LB {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        info!("Starting background service");
        let (tx, mut rx) = mpsc::channel::<(UpstreamsDashMap, Headers)>(0);

        let from_file = self.config.get("upstreams_conf");
        match from_file {
            Some(from_file) => {
                let tx_file = tx.clone();
                let tx_consul = tx.clone();

                let file_load = FromFileProvider { path: from_file.to_string() };
                let consul_load = ConsulProvider { path: from_file.to_string() };

                let _ = tokio::spawn(async move { file_load.start(tx_file).await });
                let _ = tokio::spawn(async move { consul_load.start(tx_consul).await });
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
                            clone_dashmap_into(&ss.0, &self.ump_full);
                            clone_dashmap_into(&ss.0, &self.ump_upst);
                            self.headers.clear();

                            for entry in ss.0.iter() {
                                let global_key = entry.key().clone();
                                let global_values = DashMap::new();
                                let mut target_entry = ss.1.entry(global_key).or_insert_with(DashMap::new);
                                target_entry.extend(global_values);
                                self.headers.insert(target_entry.key().to_owned(), target_entry.value().to_owned());
                            }

                            for path in ss.1.iter() {
                                let path_key = path.key().clone();
                                let path_headers = path.value().clone();
                                self.headers.insert(path_key.clone(), path_headers);
                                if let Some(global_headers) = ss.1.get("GLOBAL_HEADERS") {
                                    if let Some(existing_headers) = self.headers.get_mut(&path_key) {
                                        merge_headers(&existing_headers, &global_headers);
                                    }
                                }
                            }
                            info!("Upstreams list is changed, updating to:");
                            print_upstreams(&self.ump_full);
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
    async fn get_host(&self, peer: &str, path: &str, upgrade: bool) -> Option<(String, u16, bool)>;
    async fn get_header(&self, peer: &str, path: &str) -> Option<Vec<(String, String)>>;
}
#[async_trait]
impl GetHost for LB {
    /*
    async fn get_host(&self, peer: &str, path: &str, _upgrade: bool) -> Option<(String, u16, bool)> {
        let host_entry = self.ump_upst.get(peer);
        match host_entry {
            Some(host_entry) => {
                let upstream = if let Some(entry) = host_entry.get(path) {
                    let (servers, index) = entry.value();
                    if servers.is_empty() {
                        return None;
                    }
                    let idx = index.fetch_add(1, Ordering::Relaxed) % servers.len();
                    Some(servers[idx].clone())
                } else {
                    None
                };
                upstream
            }
            None => None,
        }
    }
    */
    async fn get_host(&self, peer: &str, path: &str, _upgrade: bool) -> Option<(String, u16, bool)> {
        let host_entry = self.ump_upst.get(peer)?;
        let mut current_path = path.to_string();
        let mut best_match: Option<(String, u16, bool)> = None;

        loop {
            if let Some(entry) = host_entry.get(&current_path) {
                let (servers, index) = entry.value();
                if !servers.is_empty() {
                    let idx = index.fetch_add(1, Ordering::Relaxed) % servers.len();
                    best_match = Some(servers[idx].clone());
                    break;
                }
            }
            if let Some(pos) = current_path.rfind('/') {
                current_path.truncate(pos);
            } else {
                break;
            }
        }
        if best_match.is_none() {
            if let Some(entry) = host_entry.get("/") {
                let (servers, index) = entry.value();
                if !servers.is_empty() {
                    let idx = index.fetch_add(1, Ordering::Relaxed) % servers.len();
                    best_match = Some(servers[idx].clone());
                }
            }
        }
        best_match
    }
    async fn get_header(&self, peer: &str, path: &str) -> Option<Vec<(String, String)>> {
        let host_entry = self.headers.get(peer)?;
        let mut current_path = path.to_string();
        let mut best_match: Option<Vec<(String, String)>> = None;

        loop {
            if let Some(entry) = host_entry.get(&current_path) {
                if !entry.value().is_empty() {
                    best_match = Some(entry.value().clone());
                    break;
                }
            }
            if let Some(pos) = current_path.rfind('/') {
                current_path.truncate(pos);
            } else {
                break;
            }
        }
        if best_match.is_none() {
            if let Some(entry) = host_entry.get("/") {
                if !entry.value().is_empty() {
                    best_match = Some(entry.value().clone());
                }
            }
        }
        best_match
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
                    Some((host, port, ssl)) => {
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
        // _upstream_response.insert_header("X-Proxied-From", "Fooooooooooooooo").unwrap();

        let host = _session.req_header().headers.get("Host");
        match host {
            Some(host) => {
                let path = _session.req_header().uri.path();
                let yoyo = self.get_header(host.to_str().unwrap(), path).await;

                for k in yoyo.iter() {
                    for t in k.iter() {
                        _upstream_response.insert_header(t.0.clone(), t.1.clone()).unwrap();
                    }
                }
            }
            None => {}
        }
        Ok(())
    }

    async fn logging(&self, session: &mut Session, _e: Option<&pingora::Error>, ctx: &mut Self::CTX) {
        let response_code = session.response_written().map_or(0, |resp| resp.status.as_u16());
        debug!("{}, response code: {response_code}", self.request_summary(session, ctx));
        // info!("{}, response code: {response_code}", self.request_summary(session, ctx));
    }
}
