use crate::utils::auth::authenticate;
use crate::utils::tools::*;
use crate::web::gethosts::GetHost;
use async_trait::async_trait;
use dashmap::DashMap;
use log::{debug, warn};
use pingora::http::RequestHeader;
use pingora::prelude::*;
use pingora_core::listeners::ALPN;
use pingora_core::prelude::HttpPeer;
use pingora_http::ResponseHeader;
use pingora_proxy::{ProxyHttp, Session};
use std::ops::Deref;
use std::sync::Arc;

pub struct LB {
    pub ump_upst: Arc<UpstreamsDashMap>,
    pub ump_full: Arc<UpstreamsDashMap>,
    pub ump_byid: Arc<UpstreamsIdMap>,
    pub headers: Arc<Headers>,
    pub config: Arc<DashMap<String, String>>,
    pub local: Arc<(String, u16)>,
    pub proxyconf: Arc<DashMap<String, Vec<String>>>,
}

pub struct MyCtx {
    backend_id: String,
}

#[async_trait]
impl ProxyHttp for LB {
    // type CTX = ();
    // fn new_ctx(&self) -> Self::CTX {}
    type CTX = MyCtx;
    fn new_ctx(&self) -> Self::CTX {
        MyCtx { backend_id: String::new() }
    }
    async fn upstream_peer(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<Box<HttpPeer>> {
        let host_name = return_header_host(&session);

        match host_name {
            Some(host) => {
                // session.req_header_mut().headers.insert("X-Host-Name", host.to_string().parse().unwrap());

                // let mut backend_id = Option::<&str>::None;
                // if let Some(_) = self.config.get("sticky_sessions") {
                //     if let Some(cookies) = session.req_header().headers.get("cookie") {
                //         let cookie_str = cookies.to_str().unwrap_or("").split(" ").collect::<Vec<&str>>()[1];
                //         backend_id = Some(cookie_str);
                //     }
                // }

                let mut backend_id = None;

                if let Some(_) = self.config.get("sticky_sessions") {
                    if let Some(cookies) = session.req_header().headers.get("cookie") {
                        if let Ok(cookie_str) = cookies.to_str() {
                            for cookie in cookie_str.split(';') {
                                let trimmed = cookie.trim();
                                if let Some(value) = trimmed.strip_prefix("backend_id=") {
                                    backend_id = Some(value);
                                    break;
                                }
                            }
                        }
                    }
                }

                let ddr = self.get_host(host, host, backend_id);

                match ddr {
                    Some((host, port, ssl)) => {
                        // let mut peer = Box::new(HttpPeer::new((host, port), ssl, String::new()));
                        let mut peer = Box::new(HttpPeer::new((host.clone(), port.clone()), ssl, String::new()));
                        if session.is_http2() {
                            peer.options.alpn = ALPN::H2;
                        }
                        _ctx.backend_id = format!("{}:{}:{}", host.clone(), port.clone(), ssl);
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
    async fn request_filter(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        if let Some(auth) = self.proxyconf.get("authorization") {
            let authenticated = authenticate(&auth.value(), &session);
            if !authenticated {
                let _ = session.respond_error(401).await;
                warn!("Forbidden: {:?}, {}", session.client_addr(), session.req_header().uri.path().to_string());
                return Ok(true);
            }
        };
        if session.req_header().uri.path().starts_with("/denied") {
            let _ = session.respond_error(403).await;
            warn!("Forbidden: {:?}, {}", session.client_addr(), session.req_header().uri.path().to_string());
            return Ok(true);
        };
        Ok(false)
    }
    async fn upstream_request_filter(&self, _session: &mut Session, _upstream_request: &mut RequestHeader, _ctx: &mut Self::CTX) -> Result<()> {
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

    async fn response_filter(&self, _session: &mut Session, _upstream_response: &mut ResponseHeader, _ctx: &mut Self::CTX) -> Result<()> {
        // _upstream_response.insert_header("X-Proxied-From", "Fooooooooooooooo").unwrap();

        if let Some(_) = self.config.get("sticky_sessions") {
            let backend_id = _ctx.backend_id.clone();
            if let Some(bid) = self.ump_byid.get(&backend_id) {
                // let _ = _upstream_response.insert_header("set-cookie", format!("backend {}", bid.0));
                let _ = _upstream_response.insert_header("set-cookie", format!("backend_id={}; Path=/; Max-Age=600; HttpOnly; SameSite=Lax", bid.0));
            }
        }

        let host_name = return_header_host(&_session);
        match host_name {
            Some(host) => {
                let path = _session.req_header().uri.path();
                let host_header = host;
                let split_header = host_header.split_once(':');
                match split_header {
                    Some(sh) => {
                        let yoyo = self.get_header(sh.0, path);
                        for k in yoyo.iter() {
                            for t in k.iter() {
                                _upstream_response.insert_header(t.0.clone(), t.1.clone()).unwrap();
                            }
                        }
                    }
                    None => {
                        let yoyo = self.get_header(host_header, path);
                        for k in yoyo.iter() {
                            for t in k.iter() {
                                _upstream_response.insert_header(t.0.clone(), t.1.clone()).unwrap();
                            }
                        }
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
    }
}

fn return_header_host(session: &Session) -> Option<&str> {
    if session.is_http2() {
        match session.req_header().uri.host() {
            Some(host) => Option::from(host),
            None => None,
        }
    } else {
        match session.req_header().headers.get("host") {
            Some(host) => {
                let header_host = host.to_str().unwrap().splitn(2, ':').collect::<Vec<&str>>();
                Option::from(header_host[0])
            }
            None => None,
        }
    }
}
