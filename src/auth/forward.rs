use crate::auth::types::AuthValidator;
use crate::utils::tools::split_host_port;
use axum::http::StatusCode;
use pingora_core::connectors::http::Connector;
use pingora_core::prelude::HttpPeer;
use pingora_http::{RequestHeader, ResponseHeader};
use pingora_proxy::Session;
use std::sync::LazyLock;

static AUTH_CONNECTOR: LazyLock<Connector> = LazyLock::new(|| Connector::new(None));
pub struct ForwardAuth<'a>(pub(crate) &'a str);
#[async_trait::async_trait]
impl AuthValidator for ForwardAuth<'_> {
    async fn validate(&self, session: &mut Session) -> bool {
        let method = match session.req_header().method.as_str() {
            "HEAD" => "HEAD",
            _ => "GET",
        };

        let auth_url = self.0;

        let (plain, tls) = if let Some(p) = auth_url.strip_prefix("http://") {
            (p, false)
        } else if let Some(p) = auth_url.strip_prefix("https://") {
            (p, true)
        } else {
            return false;
        };

        let (addr, uri) = if let Some(pos) = plain.find('/') {
            (&plain[..pos], &plain[pos..])
        } else {
            (plain, "/")
        };

        let hp = match split_host_port(addr, tls) {
            Some(hp) => hp,
            None => return false,
        };

        let peer = HttpPeer::new((hp.0, hp.1), tls, hp.0.to_string());

        let (mut http_session, _) = match AUTH_CONNECTOR.get_http_session(&peer).await {
            Ok(s) => s,
            Err(e) => {
                log::warn!("ForwardAuth: connect failed: {}", e);
                return false;
            }
        };

        let mut auth_req = match RequestHeader::build(method, uri.as_bytes(), None) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("ForwardAuth: failed to build request: {}", e);
                return false;
            }
        };

        // auth_req.headers = session.req_header().headers.clone();
        auth_req.insert_header("Host", addr).ok();
        auth_req.insert_header("X-Forwarded-Uri", uri).ok();
        auth_req.insert_header("X-Forwarded-Method", session.req_header().method.as_str()).ok();
        if let Some(auth) = session.req_header().headers.get("authorization") {
            auth_req.insert_header("Authorization", auth.clone()).ok();
        }

        if let Some(cookie) = session.req_header().headers.get("cookie") {
            auth_req.insert_header("Cookie", cookie.clone()).ok();
        }

        if tls {
            auth_req.insert_header("X-Forwarded-Proto", "https").ok();
        } else {
            auth_req.insert_header("X-Forwarded-Proto", "http").ok();
        }

        if let Err(e) = http_session.write_request_header(Box::new(auth_req)).await {
            log::warn!("ForwardAuth: write failed: {}", e);
            return false;
        }

        let status = match http_session.read_response_header().await {
            Ok(_) => http_session.response_header().map(|r| r.status.as_u16()).unwrap_or(500),
            Err(e) => {
                log::warn!("ForwardAuth: read failed: {}", e);
                return false;
            }
        };

        let auth_headers_to_forward: Vec<(String, String)> = if let Some(resp_header) = http_session.response_header() {
            resp_header
                .headers
                .iter()
                .filter_map(|(name, value)| {
                    let name_str = name.as_str();
                    if name_str.starts_with("x-") || name_str.starts_with("remote-") || name_str.starts_with("locat") {
                        value.to_str().ok().map(|v| (name_str.to_string(), v.to_string()))
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        AUTH_CONNECTOR.release_http_session(http_session, &peer, None).await;

        if (200..300).contains(&status) {
            for (name, value) in auth_headers_to_forward {
                session.req_header_mut().insert_header(name, value).ok();
            }
            true
        } else if status == 302 || status == 301 {
            let resp = ResponseHeader::build(StatusCode::MOVED_PERMANENTLY, None);
            match resp {
                Ok(mut r) => {
                    for (name, value) in auth_headers_to_forward {
                        r.insert_header(name, value).ok();
                    }
                    let _ = r.insert_header("Content-Length", "0");
                    let _ = session.write_response_header(Box::new(r), true).await;
                    true
                }
                Err(_) => return false,
            }
        } else {
            false
        }
    }
}
