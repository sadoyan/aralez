use crate::utils::auth::authenticate;
use crate::utils::metrics::*;
use crate::utils::structs::{AppConfig, Extraparams, Headers, InnerMap, UpstreamsDashMap, UpstreamsIdMap};
use crate::web::gethosts::{GetHost, GetHostsReturHeaders};
use arc_swap::ArcSwap;
use async_trait::async_trait;
use axum::body::Bytes;
use dashmap::DashMap;
use log::{debug, error, warn};
use once_cell::sync::Lazy;
use pingora::http::{RequestHeader, ResponseHeader, StatusCode};
use pingora::prelude::*;
use pingora::ErrorSource::Upstream;
use pingora_core::listeners::ALPN;
use pingora_core::prelude::HttpPeer;
use pingora_limits::rate::Rate;
use pingora_proxy::{ProxyHttp, Session};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::fmt::Write;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

static RATE_LIMITER: Lazy<Rate> = Lazy::new(|| Rate::new(Duration::from_secs(1)));
static REVERSE_STORE: Lazy<DashMap<String, String>> = Lazy::new(|| DashMap::new());
thread_local! {static IP_BUFFER: RefCell<String> = RefCell::new(String::with_capacity(50));}

#[derive(Clone)]
pub struct LB {
    pub ump_upst: Arc<UpstreamsDashMap>,
    pub ump_full: Arc<UpstreamsDashMap>,
    pub ump_byid: Arc<UpstreamsIdMap>,
    pub client_headers: Arc<Headers>,
    pub server_headers: Arc<Headers>,
    pub config: Arc<AppConfig>,
    pub extraparams: Arc<ArcSwap<Extraparams>>,
}

pub struct Context {
    backend_id: Option<String>,
    to_https: bool,
    sticky_sessions: bool,
    redirect_to: Option<String>,
    start_time: Instant,
    hostname: Option<Arc<str>>,
    upstream_peer: Option<Arc<InnerMap>>,
    extraparams: arc_swap::Guard<Arc<Extraparams>>,
    client_headers: Option<Arc<Vec<(Arc<str>, Arc<str>)>>>,
}

#[async_trait]
impl ProxyHttp for LB {
    type CTX = Context;
    fn new_ctx(&self) -> Self::CTX {
        Context {
            backend_id: None,
            to_https: false,
            sticky_sessions: false,
            redirect_to: None,
            start_time: Instant::now(),
            hostname: None,
            upstream_peer: None,
            extraparams: self.extraparams.load(),
            client_headers: None,
        }
    }
    async fn request_filter(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        let ep = _ctx.extraparams.as_ref();
        if let Some(auth) = ep.authentication.get("authorization") {
            let authenticated = authenticate(auth.value(), &session);
            if !authenticated {
                let _ = session.respond_error(401).await;
                warn!("Forbidden: {:?}, {}", session.client_addr(), session.req_header().uri.path());
                return Ok(true);
            }
        };

        let hostname = return_header_host_from_upstream(session, &self.ump_upst);

        _ctx.hostname = hostname;
        let mut backend_id = None;

        if ep.sticky_sessions {
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
        match _ctx.hostname.as_ref() {
            None => return Ok(false),
            Some(host) => {
                let optioninnermap = self.get_host(host, session.req_header().uri.path(), backend_id);
                match optioninnermap {
                    None => return Ok(false),
                    Some(ref innermap) => {
                        if let Some(rate) = innermap.rate_limit.or(ep.rate_limit) {
                            let rate_key = session.client_addr().and_then(|addr| addr.as_inet()).map(|inet| inet.ip());
                            let curr_window_requests = RATE_LIMITER.observe(&rate_key, 1);
                            if curr_window_requests > rate {
                                let mut header = ResponseHeader::build(429, None).unwrap();
                                header.insert_header("X-Rate-Limit-Limit", rate.to_string()).unwrap();
                                header.insert_header("X-Rate-Limit-Remaining", "0").unwrap();
                                header.insert_header("X-Rate-Limit-Reset", "1").unwrap();
                                session.set_keepalive(None);
                                session.write_response_header(Box::new(header), true).await?;
                                debug!("Rate limited: {:?}, {}", rate_key, rate);
                                return Ok(true);
                            }
                        }
                    }
                }
                _ctx.upstream_peer = optioninnermap;
            }
        }
        Ok(false)
    }
    async fn upstream_peer(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<Box<HttpPeer>> {
        match ctx.hostname.as_ref() {
            Some(hostname) => match ctx.upstream_peer.as_ref() {
                Some(innermap) => {
                    let mut peer = Box::new(HttpPeer::new((&*innermap.address, innermap.port), innermap.is_ssl, hostname.to_string()));

                    if innermap.is_http2 {
                        peer.options.alpn = ALPN::H2;
                    }
                    if innermap.is_ssl {
                        peer.options.verify_cert = false;
                        peer.options.verify_hostname = false;
                    }

                    if ctx.extraparams.to_https.unwrap_or(false) || innermap.to_https {
                        if let Some(stream) = session.stream() {
                            if stream.get_ssl().is_none() {
                                if let Some(host) = ctx.hostname.as_ref() {
                                    let uri = session.req_header().uri.path_and_query().map_or("/", |pq| pq.as_str());
                                    let port = self.config.proxy_port_tls.unwrap_or(443);
                                    ctx.to_https = true;
                                    let mut s = String::with_capacity(64);
                                    write!(&mut s, "https://{}:{}{}", host, port, uri).unwrap_or_default();
                                    ctx.redirect_to = Some(s);
                                }
                            }
                        }
                    }

                    if ctx.extraparams.sticky_sessions {
                        let mut s = String::with_capacity(64);
                        write!(&mut s, "{}:{}:{}", innermap.address, innermap.port, innermap.is_ssl).unwrap();
                        ctx.backend_id = Some(s);
                        ctx.sticky_sessions = true;
                    }
                    Ok(peer)
                }
                None => {
                    if let Err(e) = session.respond_error_with_body(502, Bytes::from("502 Bad Gateway\n")).await {
                        error!("Failed to send error response: {:?}", e);
                    }
                    Err(Box::new(Error {
                        etype: HTTPStatus(502),
                        esource: Upstream,
                        retry: RetryType::Decided(false),
                        cause: None,
                        context: Option::from(ImmutStr::Static("Upstream not found")),
                    }))
                }
            },
            None => {
                if let Err(e) = session.respond_error_with_body(502, Bytes::from("502 Bad Gateway\n")).await {
                    error!("Failed to send error response: {:?}", e);
                }
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

    async fn upstream_request_filter(&self, session: &mut Session, upstream_request: &mut RequestHeader, ctx: &mut Self::CTX) -> Result<()> {
        if let Some(hostname) = ctx.hostname.as_deref() {
            upstream_request.insert_header("Host", hostname)?;
        }

        if let Some(client_ip) = session.client_addr() {
            IP_BUFFER.with(|buffer| {
                let mut buf = buffer.borrow_mut();
                buf.clear();
                write!(buf, "{}", client_ip).unwrap_or(());
                upstream_request.append_header("x-forward-for", buf.as_str()).unwrap_or(false);
            });
        }
        let hostname = ctx.hostname.as_deref().unwrap_or("localhost");
        let path = session.req_header().uri.path();
        let GetHostsReturHeaders { server_headers, client_headers } = match self.get_header(hostname, path) {
            Some(h) => h,
            None => return Ok(()),
        };

        if let Some(sh) = server_headers {
            for (k, v) in sh {
                upstream_request.insert_header(k.to_string(), v.as_ref())?;
            }
        }
        if let Some(ch) = client_headers {
            ctx.client_headers = Some(Arc::new(ch));
        }
        Ok(())
    }
    async fn response_filter(&self, session: &mut Session, _upstream_response: &mut ResponseHeader, ctx: &mut Self::CTX) -> Result<()> {
        if ctx.sticky_sessions {
            if let Some(bid) = ctx.backend_id.clone() {
                if REVERSE_STORE.get(&*bid).is_none() {
                    let mut hasher = Sha256::new();
                    hasher.update(bid.clone().into_bytes());
                    let hash = hasher.finalize();
                    let hex_hash = base16ct::lower::encode_string(&hash);
                    let hh = hex_hash[0..50].to_string();
                    REVERSE_STORE.insert(bid.clone(), hh.clone());
                    REVERSE_STORE.insert(hh.clone(), bid.clone());
                }
                if let Some(tt) = REVERSE_STORE.get(&*bid) {
                    let _ = _upstream_response.insert_header("set-cookie", format!("backend_id={}; Path=/; Max-Age=600; HttpOnly; SameSite=Lax", tt.value()));
                }
            }
        }

        if ctx.to_https {
            let mut redirect_response = ResponseHeader::build(StatusCode::MOVED_PERMANENTLY, None)?;
            redirect_response.insert_header("Location", ctx.redirect_to.clone().unwrap_or(String::from("/")))?;
            redirect_response.insert_header("Content-Length", "0")?;
            session.write_response_header(Box::new(redirect_response), false).await?;
        }

        // ALLOCATIONS !
        if let Some(client_headers) = &ctx.client_headers {
            for (k, v) in client_headers.iter() {
                _upstream_response.append_header(k.to_string(), v.as_ref())?;
            }
        }
        // END ALLOCATIONS !

        session.set_keepalive(Some(300));
        Ok(())
    }

    async fn logging(&self, session: &mut Session, _e: Option<&pingora::Error>, ctx: &mut Self::CTX) {
        let response_code = session.response_written().map_or(0, |resp| resp.status.as_u16());
        debug!("{}, response code: {response_code}", self.request_summary(session, ctx));
        let m = &MetricTypes {
            method: session.req_header().method.clone(),
            code: session.response_written().map(|resp| resp.status),
            latency: ctx.start_time.elapsed(),
            version: session.req_header().version,
            upstream: ctx.hostname.clone().unwrap_or(Arc::from("localhost")),
        };
        calc_metrics(m);
    }
}

fn return_header_host_from_upstream(session: &Session, ump_upst: &UpstreamsDashMap) -> Option<Arc<str>> {
    let host_str = if session.is_http2() {
        session.req_header().uri.host()?
    } else {
        let h = session.req_header().headers.get("host")?.to_str().ok()?;
        h.split_once(':').map_or(h, |(host, _)| host)
    };
    ump_upst.get(host_str).map(|entry| entry.key().clone())
}
