use crate::utils::auth::authenticate;
use crate::utils::metrics::*;
use crate::utils::structs::{AppConfig, Extraparams, Headers, UpstreamsDashMap, UpstreamsIdMap};
use crate::web::gethosts::GetHost;
use arc_swap::ArcSwap;
use async_trait::async_trait;
use axum::body::Bytes;
use log::{debug, warn};
use once_cell::sync::Lazy;
use pingora::http::{RequestHeader, ResponseHeader, StatusCode};
use pingora::prelude::*;
use pingora::ErrorSource::Upstream;
use pingora_core::listeners::ALPN;
use pingora_core::prelude::HttpPeer;
use pingora_limits::rate::Rate;
use pingora_proxy::{ProxyHttp, Session};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

#[derive(Clone)]
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
    start_time: Instant,
    hostname: Option<String>,
}
// Rate limiter
static RATE_LIMITER: Lazy<Rate> = Lazy::new(|| Rate::new(Duration::from_secs(1)));
// max request per second per client
// static MAX_REQ_PER_SEC: isize = 1;

#[async_trait]
impl ProxyHttp for LB {
    type CTX = Context;
    fn new_ctx(&self) -> Self::CTX {
        Context {
            backend_id: String::new(),
            to_https: false,
            redirect_to: String::new(),
            start_time: Instant::now(),
            hostname: None,
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

        let hostname = return_header_host(&session);
        _ctx.hostname = hostname.clone();
        if let Some(rate) = self.extraparams.load().rate_limit {
            match hostname {
                None => return Ok(false),
                Some(host) => {
                    let curr_window_requests = RATE_LIMITER.observe(&host, 1);
                    if curr_window_requests > rate {
                        let mut header = ResponseHeader::build(429, None).unwrap();
                        header.insert_header("X-Rate-Limit-Limit", rate.to_string()).unwrap();
                        header.insert_header("X-Rate-Limit-Remaining", "0").unwrap();
                        header.insert_header("X-Rate-Limit-Reset", "1").unwrap();
                        session.set_keepalive(None);
                        session.write_response_header(Box::new(header), true).await?;
                        debug!("Rate limited: {:?}, {}", session.client_addr(), rate);
                        return Ok(true);
                    }
                }
            };
        }
        Ok(false)
    }
    async fn upstream_peer(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<Box<HttpPeer>> {
        // let host_name = return_header_host(&session);
        match ctx.hostname.as_ref() {
            Some(hostname) => {
                let mut backend_id = None;

                if self.extraparams.load().sticky_sessions {
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

                let optioninnermap = self.get_host(hostname, hostname, backend_id);

                match optioninnermap {
                    // Some((address, port, ssl, is_h2, to_https)) => {
                    Some(innermap) => {
                        let mut peer = Box::new(HttpPeer::new((innermap.address.clone(), innermap.port.clone()), innermap.is_ssl, String::new()));
                        // if session.is_http2() {
                        if innermap.is_http2 {
                            peer.options.alpn = ALPN::H2;
                        }
                        if innermap.is_ssl {
                            peer.sni = hostname.clone();
                            peer.options.verify_cert = false;
                            peer.options.verify_hostname = false;
                        }

                        if self.extraparams.load().to_https.unwrap_or(false) || innermap.to_https {
                            if let Some(stream) = session.stream() {
                                if stream.get_ssl().is_none() {
                                    if let Some(addr) = session.server_addr() {
                                        if let Some((host, _)) = addr.to_string().split_once(':') {
                                            let uri = session.req_header().uri.path_and_query().map_or("/", |pq| pq.as_str());
                                            let port = self.config.proxy_port_tls.unwrap_or(403);
                                            ctx.to_https = true;
                                            ctx.redirect_to = format!("https://{}:{}{}", host, port, uri);
                                        }
                                    }
                                }
                            }
                        }

                        ctx.backend_id = format!("{}:{}:{}", innermap.address.clone(), innermap.port.clone(), innermap.is_ssl);
                        Ok(peer)
                    }
                    None => {
                        session.respond_error_with_body(502, Bytes::from("502 Bad Gateway\n")).await.expect("Failed to send error");
                        Err(Box::new(Error {
                            etype: HTTPStatus(502),
                            esource: Upstream,
                            retry: RetryType::Decided(false),
                            cause: None,
                            context: Option::from(ImmutStr::Static("Upstream not found")),
                        }))
                    }
                }
            }
            None => {
                session.respond_error_with_body(502, Bytes::from("502 Bad Gateway\n")).await.expect("Failed to send error");
                Err(Box::new(Error {
                    etype: HTTPStatus(502),
                    esource: Upstream,
                    retry: RetryType::Decided(false),
                    cause: None,
                    context: None,
                }))
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

    // async fn request_body_filter(&self, _session: &mut Session, _body: &mut Option<Bytes>, _end_of_stream: bool, _ctx: &mut Self::CTX) -> Result<()>
    // where
    //     Self::CTX: Send + Sync,
    // {
    //     Ok(())
    // }
    async fn response_filter(&self, session: &mut Session, _upstream_response: &mut ResponseHeader, ctx: &mut Self::CTX) -> Result<()> {
        // _upstream_response.insert_header("X-Proxied-From", "Fooooooooooooooo").unwrap();
        if self.extraparams.load().sticky_sessions {
            let backend_id = ctx.backend_id.clone();
            if let Some(bid) = self.ump_byid.get(&backend_id) {
                let _ = _upstream_response.insert_header("set-cookie", format!("backend_id={}; Path=/; Max-Age=600; HttpOnly; SameSite=Lax", bid.address));
            }
        }
        if ctx.to_https {
            let mut redirect_response = ResponseHeader::build(StatusCode::MOVED_PERMANENTLY, None)?;
            redirect_response.insert_header("Location", ctx.redirect_to.clone())?;
            redirect_response.insert_header("Content-Length", "0")?;
            session.write_response_header(Box::new(redirect_response), false).await?;
        }
        // match return_header_host(&session) {
        match ctx.hostname.as_ref() {
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
        session.set_keepalive(Some(300));
        Ok(())
    }

    async fn logging(&self, session: &mut Session, _e: Option<&pingora::Error>, ctx: &mut Self::CTX) {
        let response_code = session.response_written().map_or(0, |resp| resp.status.as_u16());
        debug!("{}, response code: {response_code}", self.request_summary(session, ctx));
        let m = &MetricTypes {
            method: session.req_header().method.to_string(),
            code: session.response_written().map(|resp| resp.status.as_str().to_owned()).unwrap_or("0".to_string()),
            latency: ctx.start_time.elapsed(),
            version: session.req_header().version,
        };
        calc_metrics(m);
    }
}

fn return_header_host(session: &Session) -> Option<String> {
    if session.is_http2() {
        match session.req_header().uri.host() {
            Some(host) => Option::from(host.to_string()),
            None => None,
        }
    } else {
        match session.req_header().headers.get("host") {
            Some(host) => {
                let header_host = host.to_str().unwrap().splitn(2, ':').collect::<Vec<&str>>();
                Option::from(header_host[0].to_string())
            }
            None => None,
        }
    }
}
