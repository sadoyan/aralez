use crate::utils::auth::authenticate;
use crate::utils::lazylock::{LOCALHOST, RATE_LIMITER, REQUESTS_4XX, REVERSE_STORE};
use crate::utils::metrics::*;
use crate::utils::structs::{AppConfig, Extraparams, Headers, InnerMap, UpstreamsDashMap, UpstreamsIdMap};
use crate::web::gethosts::{GetHost, GetHostsReturHeaders};
use crate::web::logging::access_log;
use arc_swap::ArcSwap;
use async_trait::async_trait;
use axum::body::Bytes;
use log::error;
use pingora::http::{RequestHeader, ResponseHeader, StatusCode};
use pingora::prelude::*;
use pingora::ErrorSource::Upstream;
use pingora_core::listeners::ALPN;
use pingora_core::prelude::HttpPeer;
use pingora_proxy::{ProxyHttp, Session};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::fmt::Write;
use std::sync::Arc;
use tokio::time::Instant;

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
    start_time: Instant,
    hostname: Option<Arc<str>>,
    upstream_peer: Option<Arc<InnerMap>>,
    extraparams: arc_swap::Guard<Arc<Extraparams>>,
    client_headers: Option<Vec<(String, Arc<str>)>>,
    x4xx_limit: Option<u32>,
}

#[async_trait]
impl ProxyHttp for LB {
    type CTX = Context;
    fn new_ctx(&self) -> Self::CTX {
        Context {
            backend_id: None,
            start_time: Instant::now(),
            hostname: None,
            upstream_peer: None,
            extraparams: self.extraparams.load(),
            client_headers: None,
            x4xx_limit: None,
        }
    }
    async fn request_filter(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        ACTIVE_SESSIONS.inc();
        let hostname = return_header_host_from_upstream(session, &self.ump_upst);
        _ctx.hostname = hostname;
        let mut backend_id = None;
        if let Some(_) = _ctx.extraparams.sticky_sessions {
            if let Some(cookies) = session.req_header().headers.get("cookie") {
                if let Ok(cookie_str) = cookies.to_str() {
                    if let Some(pos) = cookie_str.find("backend_id=") {
                        let value = &cookie_str[pos + "backend_id=".len()..];
                        let end = value.find(';').unwrap_or(value.len());
                        backend_id = Some(&value[..end]);
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
                        if let Some(auth) = _ctx.extraparams.authentication.as_ref().or(innermap.authorization.as_ref()) {
                            if !authenticate(&auth, session).await {
                                let _ = session.respond_error(401).await;
                                return Ok(true);
                            }
                        }
                        if let Some(rate) = innermap.x4xx_limit.or(_ctx.extraparams.x4xx_limit) {
                            _ctx.x4xx_limit = innermap.x4xx_limit;
                            let rate_key = session.client_addr().and_then(|addr| addr.as_inet()).map(|inet| inet.ip());
                            if let Some(rk) = rate_key {
                                let count = REQUESTS_4XX.get(&rk).unwrap_or(0);
                                if count > rate {
                                    let header = ResponseHeader::build(429, None)?;
                                    session.set_keepalive(None);
                                    session.write_response_header(Box::new(header), true).await?;
                                    // if let (Some(oi), Some(oa)) = (&_ctx.hostname, rate_key) {
                                    //     warn!("Limit 4XX: {}-rps exceed on {} from {} path {}", rate, oi, oa, session.req_header().uri.path());
                                    // }
                                    return Ok(true);
                                }
                            }
                        }
                        if let Some(rate) = innermap.rate_limit.or(_ctx.extraparams.rate_limit) {
                            let rate_key = session.client_addr().and_then(|addr| addr.as_inet()).map(|inet| inet.ip());
                            let curr_window_requests = RATE_LIMITER.observe(&rate_key, 1);
                            if curr_window_requests > rate {
                                let header = ResponseHeader::build(429, None)?;
                                session.set_keepalive(None);
                                session.write_response_header(Box::new(header), true).await?;
                                // if let (Some(oi), Some(oa)) = (&_ctx.hostname, rate_key) {
                                //     warn!("Limit: {}-rps exceed on {} from {}", rate, oi, oa);
                                // }
                                return Ok(true);
                            }
                        }

                        if let Some(redirect_to) = &innermap.redirect_to {
                            let uri = session.req_header().uri.path();
                            let capacity = redirect_to.len() + uri.len();
                            let mut s = String::with_capacity(capacity);
                            s.push_str(redirect_to);
                            s.push_str(uri);
                            let mut resp = ResponseHeader::build(StatusCode::MOVED_PERMANENTLY, None)?;
                            resp.insert_header("Location", s)?;
                            resp.insert_header("Content-Length", "0")?;
                            session.write_response_header(Box::new(resp), true).await?;
                            return Ok(true);
                        }

                        if _ctx.extraparams.to_https.unwrap_or(false) || innermap.to_https {
                            if let Some(stream) = session.stream() {
                                if stream.get_ssl().is_none() {
                                    if let Some(host) = _ctx.hostname.as_ref() {
                                        let port = self.config.proxy_port_tls.as_deref().unwrap_or("443");
                                        let uri = session.req_header().uri.path();
                                        let capacity = host.len() + uri.len() + 8;
                                        let mut s = String::with_capacity(capacity);
                                        s.push_str("https://");
                                        s.push_str(host);
                                        if port != "443" {
                                            s.push(':');
                                            s.push_str(port);
                                        }
                                        s.push_str(uri);
                                        let mut resp = ResponseHeader::build(StatusCode::MOVED_PERMANENTLY, None)?;
                                        resp.insert_header("Location", s)?;
                                        resp.insert_header("Content-Length", "0")?;
                                        session.write_response_header(Box::new(resp), true).await?;
                                        return Ok(true);
                                    }
                                }
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
                    /*
                    Experimental optionsv
                    The following TCP optimizations were tested but caused performance degrade under heavy load:
                    peer.options.tcp_keepalive = Some(TcpKeepalive {
                        idle: Duration::from_secs(60),
                        interval: Duration::from_secs(10),
                        count: 5,
                        user_timeout: Duration::from_secs(30),
                    });

                    peer.options.idle_timeout = Some(Duration::from_secs(300));
                    peer.options.tcp_recv_buf = Some(128 * 1024);
                    End of experimental options
                    */
                    if let Some(_) = ctx.extraparams.sticky_sessions {
                        let mut s = String::with_capacity(64);
                        write!(
                            &mut s,
                            "{}:{}:{}:{}:{}:{}:{}:{}:{:?}",
                            hostname,
                            innermap.address,
                            innermap.port,
                            innermap.is_http2,
                            innermap.to_https,
                            innermap.x4xx_limit.unwrap_or_default(),
                            innermap.rate_limit.unwrap_or_default(),
                            innermap.healthcheck.unwrap_or_default(),
                            innermap.authorization
                        )
                        .unwrap_or(());
                        ctx.backend_id = Some(s);
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
        if let Some(ip) = session.client_addr().and_then(|a| a.as_inet()).map(|i| i.ip()) {
            IP_BUFFER.with(|buffer| {
                let mut buf = buffer.borrow_mut();
                buf.clear();
                write!(buf, "{}", ip).unwrap_or(());
                upstream_request.append_header("X-Forwarded-For", buf.as_str()).unwrap_or(false);
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
                upstream_request.insert_header(k, v.as_ref())?;
            }
        }
        if let Some(ch) = client_headers {
            ctx.client_headers = Some(ch);
        }
        Ok(())
    }
    async fn response_filter(&self, _session: &mut Session, _upstream_response: &mut ResponseHeader, ctx: &mut Self::CTX) -> Result<()> {
        if let Some(val) = ctx.extraparams.sticky_sessions {
            if let Some(bid) = &ctx.backend_id {
                let tt = if let Some(existing) = REVERSE_STORE.get(bid) {
                    existing.value().clone()
                } else {
                    let mut hasher = Sha256::new();
                    hasher.update(bid.as_bytes());
                    let hash = hasher.finalize();
                    let hex_hash = base16ct::lower::encode_string(&hash);
                    let hh = hex_hash[0..50].to_string();
                    REVERSE_STORE.insert(bid.clone(), hh.clone());
                    REVERSE_STORE.insert(hh.clone(), bid.clone());
                    hh
                };
                let mut buf = String::with_capacity(80);
                buf.push_str("backend_id=");
                buf.push_str(&tt);
                buf.push_str("; Path=/; Max-Age=");
                buf.push_str(&val.to_string());
                buf.push_str("; HttpOnly; SameSite=Lax");
                let _ = _upstream_response.append_header("set-cookie", buf.as_str());
            }
        }

        if let Some(client_headers) = &ctx.client_headers {
            for (k, v) in client_headers.iter() {
                _upstream_response.append_header(k.clone(), v.as_ref())?;
            }
        }
        Ok(())
    }

    async fn logging(&self, session: &mut Session, _e: Option<&pingora::Error>, ctx: &mut Self::CTX) {
        let response_code = session.response_written().map_or(0, |resp| resp.status.as_u16());
        let m = &MetricTypes {
            method: session.req_header().method.clone(),
            code: session.response_written().map(|resp| resp.status),
            latency: ctx.start_time.elapsed(),
            version: session.req_header().version,
            upstream: ctx.hostname.take().unwrap_or_else(|| LOCALHOST.clone()),
        };
        calc_metrics(m);
        ACTIVE_SESSIONS.dec();
        if let Some(_) = ctx.x4xx_limit.or(ctx.extraparams.x4xx_limit) {
            if (400..=499).contains(&response_code) {
                if let Some(ip) = session.client_addr().and_then(|a| a.as_inet()).map(|i| i.ip()) {
                    let current = REQUESTS_4XX.get(&ip).unwrap_or(0);
                    REQUESTS_4XX.insert(ip, current + 1);
                }
            }
        }
        access_log(response_code, &self.request_summary(session, ctx), session);
    }
}

fn return_header_host_from_upstream(session: &Session, ump_upst: &UpstreamsDashMap) -> Option<Arc<str>> {
    let host_str = if session.is_http2() {
        session.req_header().uri.host()?
    } else {
        let h = session.req_header().headers.get("host")?.to_str().ok()?;
        h.split_once(':').map_or(h, |(host, _)| host)
    };

    ump_upst.get(host_str).or_else(|| ump_upst.get("DEFAULT")).map(|entry| entry.key().clone())
}
