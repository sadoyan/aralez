use crate::utils::auth::authenticate;
use crate::utils::metrics::*;
use crate::utils::structs::{AppConfig, Extraparams, Headers, InnerMap, UpstreamsDashMap, UpstreamsIdMap};
use crate::web::gethosts::GetHost;
use arc_swap::ArcSwap;
use async_trait::async_trait;
use axum::body::Bytes;
use log::{debug, error, warn};
use once_cell::sync::Lazy;
use pingora::http::{RequestHeader, ResponseHeader, StatusCode};
use pingora::prelude::*;
use pingora::ErrorSource::Upstream;
use pingora_core::listeners::ALPN;
use pingora_core::prelude::HttpPeer;
use pingora_limits::rate::Rate;
use pingora_proxy::{ProxyHttp, Session};
// use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

static RATE_LIMITER: Lazy<Rate> = Lazy::new(|| Rate::new(Duration::from_secs(1)));

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
    backend_id: Arc<str>,
    // backend_id: Arc<(IpAddr, u16, bool)>,
    to_https: bool,
    redirect_to: Arc<str>,
    start_time: Instant,
    hostname: Option<Arc<str>>,
    upstream_peer: Option<InnerMap>,
    extraparams: arc_swap::Guard<Arc<Extraparams>>,
    client_headers: Arc<Vec<(Arc<str>, Arc<str>)>>,
}

#[async_trait]
impl ProxyHttp for LB {
    type CTX = Context;
    fn new_ctx(&self) -> Self::CTX {
        Context {
            backend_id: Arc::from(""),
            // backend_id: Arc::new((IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0, false)),
            to_https: false,
            redirect_to: Arc::from(""),
            start_time: Instant::now(),
            hostname: None,
            upstream_peer: None,
            extraparams: self.extraparams.load(),
            client_headers: Arc::new(Vec::new()),
        }
    }
    async fn request_filter(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<bool> {
        let ep = _ctx.extraparams.clone();

        if let Some(auth) = ep.authentication.get("authorization") {
            let authenticated = authenticate(&auth.value(), &session);
            if !authenticated {
                let _ = session.respond_error(401).await;
                warn!("Forbidden: {:?}, {}", session.client_addr(), session.req_header().uri.path());
                return Ok(true);
            }
        };

        let hostname = return_header_host(&session);
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
                // let optioninnermap = self.get_host(host.as_str(), host.as_str(), backend_id);
                let optioninnermap = self.get_host(host, session.req_header().uri.path(), backend_id);
                match optioninnermap {
                    None => return Ok(false),
                    Some(ref innermap) => {
                        if let Some(rate) = innermap.rate_limit.or(ep.rate_limit) {
                            // let rate_key = session.client_addr().and_then(|addr| addr.as_inet()).map(|inet| inet.ip().to_string()).unwrap_or_else(|| host.to_string());
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
                    let mut peer = Box::new(HttpPeer::new((innermap.address.clone(), innermap.port.clone()), innermap.is_ssl, String::new()));
                    if innermap.is_http2 {
                        peer.options.alpn = ALPN::H2;
                    }
                    if innermap.is_ssl {
                        peer.sni = hostname.to_string();
                        peer.options.verify_cert = false;
                        peer.options.verify_hostname = false;
                    }
                    if ctx.to_https || innermap.to_https {
                        if let Some(stream) = session.stream() {
                            if stream.get_ssl().is_none() {
                                if let Some(addr) = session.server_addr() {
                                    if let Some((host, _)) = addr.to_string().split_once(':') {
                                        let uri = session.req_header().uri.path_and_query().map_or("/", |pq| pq.as_str());
                                        let port = self.config.proxy_port_tls.unwrap_or(403);
                                        ctx.to_https = true;
                                        ctx.redirect_to = Arc::from(format!("https://{}:{}{}", host, port, uri));
                                    }
                                }
                            }
                        }
                    }
                    ctx.backend_id = Arc::from(format!("{}:{}:{}", innermap.address, innermap.port, innermap.is_ssl));
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
                // session.respond_error_with_body(502, Bytes::from("502 Bad Gateway\n")).await.expect("Failed to send error");
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
        if let Some(hostname) = ctx.hostname.as_ref() {
            upstream_request.insert_header("Host", hostname.as_ref())?;
        }
        if let Some(peer) = ctx.upstream_peer.as_ref() {
            upstream_request.insert_header("X-Forwarded-For", peer.address.to_string())?;
        }

        if let Some(headers) = self.get_header(ctx.hostname.as_ref().unwrap_or(&Arc::from("localhost")), session.req_header().uri.path()) {
            if let Some(server_headers) = headers.server_headers {
                for k in server_headers {
                    upstream_request.insert_header(k.0.to_string(), k.1.as_ref())?;
                }
            }
            if let Some(client_headers) = headers.client_headers {
                let converted: Vec<(Arc<str>, Arc<str>)> = client_headers.into_iter().map(|(k, v)| (Arc::<str>::from(k), Arc::<str>::from(v))).collect();

                ctx.client_headers = Arc::new(converted);
            }
        }

        Ok(())
    }
    async fn response_filter(&self, session: &mut Session, _upstream_response: &mut ResponseHeader, ctx: &mut Self::CTX) -> Result<()> {
        if ctx.extraparams.sticky_sessions {
            let backend_id = ctx.backend_id.clone();
            if let Some(bid) = self.ump_byid.get(backend_id.as_ref()) {
                let _ = _upstream_response.insert_header("set-cookie", format!("backend_id={}; Path=/; Max-Age=600; HttpOnly; SameSite=Lax", bid.address));
            }
        }
        if ctx.to_https {
            let mut redirect_response = ResponseHeader::build(StatusCode::MOVED_PERMANENTLY, None)?;
            redirect_response.insert_header("Location", ctx.redirect_to.as_ref())?;
            redirect_response.insert_header("Content-Length", "0")?;
            session.write_response_header(Box::new(redirect_response), false).await?;
        }

        for (key, value) in ctx.client_headers.iter() {
            _upstream_response.insert_header(key.to_string(), value.as_ref()).unwrap();
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

fn return_header_host(session: &Session) -> Option<Arc<str>> {
    if session.is_http2() {
        match session.req_header().uri.host() {
            Some(host) => Option::from(Arc::from(host)),
            None => None,
        }
    } else {
        match session.req_header().headers.get("host") {
            Some(host) => {
                let header_host: &str = host.to_str().unwrap().split_once(':').map_or(host.to_str().unwrap(), |(h, _)| h);
                Option::from(Arc::<str>::from(header_host))
            }
            None => None,
        }
    }
}
