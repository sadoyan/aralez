use crate::utils::auth::authenticate;
use crate::utils::structs::{AppConfig, Extraparams, Headers, UpstreamsDashMap, UpstreamsIdMap};
use crate::web::gethosts::GetHost;
use arc_swap::ArcSwap;
use async_trait::async_trait;
use log::{debug, warn};
use pingora::http::{RequestHeader, ResponseHeader, StatusCode};
use pingora::prelude::*;
use pingora_core::listeners::ALPN;
use pingora_core::prelude::HttpPeer;
use pingora_proxy::{ProxyHttp, Session};
use std::sync::Arc;

pub struct LB {
    pub ump_upst: Arc<UpstreamsDashMap>,
    pub ump_full: Arc<UpstreamsDashMap>,
    pub ump_byid: Arc<UpstreamsIdMap>,
    pub headers: Arc<Headers>,
    pub config: Arc<AppConfig>,
    pub extraparams: Arc<ArcSwap<Extraparams>>,
}

pub struct Context {
    backend_id: String,
    to_https: bool,
    redirect_to: String,
}

#[async_trait]
impl ProxyHttp for LB {
    // type CTX = ();
    // fn new_ctx(&self) -> Self::CTX {}
    type CTX = Context;
    fn new_ctx(&self) -> Self::CTX {
        Context {
            backend_id: String::new(),
            to_https: false,
            redirect_to: String::new(),
        }
    }
    async fn request_filter(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        if let Some(auth) = self.extraparams.load().authentication.get("authorization") {
            let authenticated = authenticate(&auth.value(), &session);
            if !authenticated {
                let _ = session.respond_error(401).await;
                warn!("Forbidden: {:?}, {}", session.client_addr(), session.req_header().uri.path().to_string());
                return Ok(true);
            }
        };

        // if session.req_header().uri.path().starts_with("/denied") {
        //     let _ = session.respond_error(403).await;
        //     warn!("Forbidden: {:?}, {}", session.client_addr(), session.req_header().uri.path().to_string());
        //     return Ok(true);
        // };
        Ok(false)
    }
    async fn upstream_peer(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<Box<HttpPeer>> {
        let host_name = return_header_host(&session);

        match host_name {
            Some(hostname) => {
                // session.req_header_mut().headers.insert("X-Host-Name", host.to_string().parse().unwrap());
                let mut backend_id = None;
                if self.extraparams.load().stickysessions {
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

                let ddr = self.get_host(hostname, hostname, backend_id);

                match ddr {
                    Some((address, port, ssl, is_h2, to_https)) => {
                        let mut peer = Box::new(HttpPeer::new((address.clone(), port.clone()), ssl, String::new()));
                        // if session.is_http2() {
                        if is_h2 {
                            peer.options.alpn = ALPN::H2;
                        }
                        if ssl {
                            peer.sni = hostname.to_string();
                            peer.options.verify_cert = false;
                            peer.options.verify_hostname = false;
                        }
                        // println!("{}, {}, alpn {}, h2 {:?}, to_https {}", hostname, address.as_str(), peer.options.alpn, is_h2, _to_https);
                        // let mut gogo = false;
                        // let mut location = String::new();
                        if to_https {
                            if let Some(stream) = session.stream() {
                                if stream.get_ssl().is_none() {
                                    if let Some(addr) = session.server_addr() {
                                        if let Some((host, _)) = addr.to_string().split_once(':') {
                                            let uri = session.req_header().uri.path_and_query().map_or("/", |pq| pq.as_str());
                                            let port = self.config.proxy_port_tls.unwrap_or(443);
                                            // let location = format!("https://{}:{}{}", host, port, uri);
                                            // println!("Redirect: {} => {}", hostname, location);
                                            // gogo = true;
                                            _ctx.to_https = true;
                                            _ctx.redirect_to = format!("https://{}:{}{}", host, port, uri);
                                        }
                                    }
                                }
                            }
                        }

                        // if gogo {
                        //     println!("Redirect: {} => {}", hostname, location);
                        //     let _ = session.respond_error(301).await;
                        //     warn!("Forbidden: {:?}, {}", session.client_addr(), session.req_header().uri.path().to_string());
                        // }
                        _ctx.backend_id = format!("{}:{}:{}", address.clone(), port.clone(), ssl);
                        // println!("{:?}, {:?}", session.request_summary(), session.server_addr());
                        Ok(peer)
                    }
                    None => {
                        warn!("Upstream not found. Host: {:?}, Path: {}", hostname, session.req_header().uri);
                        Ok(return_no_host(&self.config.local_server))
                    }
                }
            }
            None => {
                warn!("Upstream not found. Host: {:?}, Path: {}", host_name, session.req_header().uri);
                Ok(return_no_host(&self.config.local_server))
            }
        }
    }

    async fn upstream_request_filter(&self, session: &mut Session, _upstream_request: &mut RequestHeader, _ctx: &mut Self::CTX) -> Result<()> {
        match session.client_addr() {
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

    async fn response_filter(&self, session: &mut Session, _upstream_response: &mut ResponseHeader, _ctx: &mut Self::CTX) -> Result<()> {
        // _upstream_response.insert_header("X-Proxied-From", "Fooooooooooooooo").unwrap();
        if self.extraparams.load().stickysessions {
            let backend_id = _ctx.backend_id.clone();
            if let Some(bid) = self.ump_byid.get(&backend_id) {
                // let _ = _upstream_response.insert_header("set-cookie", format!("backend {}", bid.0));
                let _ = _upstream_response.insert_header("set-cookie", format!("backend_id={}; Path=/; Max-Age=600; HttpOnly; SameSite=Lax", bid.0));
            }
        }

        if _ctx.to_https {
            // println!("{} => {}", _ctx.to_https, _ctx.redirect_to);
            let mut redirect_response = ResponseHeader::build(StatusCode::MOVED_PERMANENTLY, None)?;
            redirect_response.insert_header("Location", _ctx.redirect_to.clone())?;
            redirect_response.insert_header("Content-Length", "0")?;
            session.write_response_header(Box::new(redirect_response), false).await?;
            // return Ok(());
            // let response = session.write_response_header(Box::new(redirect_response), false).await?;
            // return Ok(response);
        }

        match return_header_host(&session) {
            Some(host) => {
                let path = session.req_header().uri.path();
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

fn return_no_host(inp: &Option<(String, u16)>) -> Box<HttpPeer> {
    match inp {
        Some(t) => Box::new(HttpPeer::new(t, false, String::new())),
        None => Box::new(HttpPeer::new(("0.0.0.0", 0), false, String::new())),
    }
}
