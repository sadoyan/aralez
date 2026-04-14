use crate::utils::jwt::check_jwt;
// use reqwest::Client;
use axum::http::StatusCode;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use pingora_proxy::Session;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use subtle::ConstantTimeEq;
use urlencoding::decode;

// use pingora::http::{RequestHeader, ResponseHeader, StatusCode};
use pingora::http::RequestHeader;
// --------------------------------- //
use pingora_core::connectors::http::Connector;
use pingora_core::upstreams::peer::HttpPeer;
use pingora_http::ResponseHeader;
// --------------------------------- //

#[async_trait::async_trait]
trait AuthValidator {
    async fn validate(&self, session: &mut Session) -> bool;
}
struct BasicAuth<'a>(&'a str);
struct ApiKeyAuth<'a>(&'a str);
struct JwtAuth<'a>(&'a str);
struct ForwardAuth<'a>(&'a str);

pub static AUTH_CONNECTOR: LazyLock<Connector> = LazyLock::new(|| Connector::new(None));

#[async_trait::async_trait]
impl AuthValidator for ForwardAuth<'_> {
    async fn validate(&self, session: &mut Session) -> bool {
        // let method = match session.req_header().method.as_str() {
        //     "HEAD" => "HEAD",
        //     _ => "GET",
        // };

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

        let mut auth_req = match RequestHeader::build("GET", uri.as_bytes(), None) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("ForwardAuth: failed to build request: {}", e);
                return false;
            }
        };

        // Filter headers ????
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
        // (200..300).contains(&status)
    }
}

#[async_trait::async_trait]
impl AuthValidator for BasicAuth<'_> {
    async fn validate(&self, session: &mut Session) -> bool {
        if let Some(header) = session.get_header("authorization") {
            if let Some(h) = header.to_str().ok() {
                if let Some((_, val)) = h.split_once(' ') {
                    if let Some(decoded) = STANDARD.decode(val).ok() {
                        if decoded.as_slice().ct_eq(self.0.as_bytes()).into() {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
}

#[async_trait::async_trait]
impl AuthValidator for ApiKeyAuth<'_> {
    async fn validate(&self, session: &mut Session) -> bool {
        if let Some(header) = session.get_header("x-api-key") {
            if let Some(h) = header.to_str().ok() {
                return h.as_bytes().ct_eq(self.0.as_bytes()).into();
            }
        }
        false
    }
}

#[async_trait::async_trait]
impl AuthValidator for JwtAuth<'_> {
    async fn validate(&self, session: &mut Session) -> bool {
        let jwtsecret = self.0;
        if let Some(tok) = get_query_param(session, "araleztoken") {
            return check_jwt(tok.as_str(), jwtsecret);
        }
        if let Some(auth_header) = session.get_header("authorization") {
            if let Ok(header_str) = auth_header.to_str() {
                if let Some((scheme, token)) = header_str.split_once(' ') {
                    if scheme.eq_ignore_ascii_case("bearer") {
                        return check_jwt(token, jwtsecret);
                    }
                }
            }
        }
        false
    }
}

pub async fn authenticate(auth_type: &Arc<str>, credentials: &Arc<str>, session: &mut Session) -> bool {
    match &**auth_type {
        "basic" => BasicAuth(credentials).validate(session).await,
        "apikey" => ApiKeyAuth(credentials).validate(session).await,
        "jwt" => JwtAuth(credentials).validate(session).await,
        "forward" => ForwardAuth(credentials).validate(session).await,
        _ => {
            log::warn!("Unsupported authentication mechanism : {}", auth_type);
            false
        }
    }
}

pub fn get_query_param(session: &mut Session, key: &str) -> Option<String> {
    let query = session.req_header().uri.query()?;

    let params: HashMap<_, _> = query
        .split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let k = parts.next()?;
            let v = parts.next().unwrap_or(""); // Some params might have no value
            Some((k, v))
        })
        .collect();
    params.get(key).and_then(|v| decode(v).ok()).map(|s| s.to_string())
}

fn split_host_port(addr: &str, tls: bool) -> Option<(&str, u16, bool, &str)> {
    match addr.split_once(':') {
        Some((h, p)) => match p.parse::<u16>() {
            Ok(port) => return Some((h, port, tls, h)),
            Err(_) => {
                log::warn!("ForwardAuth: invalid port in {}", addr);
                return None;
            }
        },
        None => {
            if tls {
                return Some((addr, 443u16, tls, addr));
            } else {
                return Some((addr, 80u16, tls, addr));
            }
        }
    };
}
