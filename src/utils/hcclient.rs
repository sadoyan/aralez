use crate::utils::grpc::ping_grpc;
use bytes::Bytes;
use pingora_core::connectors::http::Connector;
use pingora_core::listeners::ALPN;
use pingora_core::prelude::HttpPeer;
use pingora_http::RequestHeader;
use std::sync::LazyLock;

pub static HC_CONNECTOR: LazyLock<Connector> = LazyLock::new(|| Connector::new(None));

pub async fn httpclient(method: &str, tls: bool, host: &str, path: &str, address: &str, port: u16, url: String) -> (bool, bool) {
    let method = match method {
        "HEAD" => "HEAD",
        "GET" => "GET",
        "POST" => "POST",
        _ => "GET",
    };

    let mut peer = HttpPeer::new((address, port), tls, host.to_string());

    if tls {
        peer.options.verify_cert = false;
        peer.options.verify_hostname = false;
    }

    let mut is_h2 = false;
    peer.options.alpn = ALPN::H2H1;
    let (mut http_session, _) = match HC_CONNECTOR.get_http_session(&peer).await {
        Ok(s) => s,
        Err(e) => {
            if let Some(msg) = e.context {
                log::warn!("{}, type: {}", msg.as_str(), e.etype.as_str());
            } else {
                log::warn!("Fail to connect to addr: {}, tls {}, host {}", address, tls, host);
            }
            return (false, false);
        }
    };

    match http_session.as_http2() {
        Some(_) => {
            peer.options.alpn = ALPN::H2;
            is_h2 = true;
        }
        None => {
            if ping_grpc(url.as_str()).await {
                return (true, true);
            }
        }
    }

    let mut req = match RequestHeader::build(method, path.as_bytes(), None) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("Failed to build request: {}", e);
            return (false, false);
        }
    };

    req.insert_header("Host", host).ok();

    if let Err(e) = http_session.write_request_header(Box::new(req)).await {
        log::warn!("Write header failed: {}", e);
        return (false, false);
    }

    let status = if is_h2 {
        if let Err(e) = http_session.write_request_body(Bytes::new(), true).await {
            log::warn!("Write body failed: {}", e);
            return (false, false);
        }
        match http_session.read_response_header().await {
            Ok(_) => http_session.response_header().map(|r| r.status.as_u16()).unwrap_or(500),
            Err(e) => {
                if ping_grpc(url.as_str()).await {
                    return (true, true);
                }
                log::warn!("Health Check H2 read failed: {} - {}", host, e);
                return (false, false);
            }
        }
    } else {
        match http_session.read_response_header().await {
            Ok(_) => http_session.response_header().map(|r| r.status.as_u16()).unwrap_or(500),
            Err(e) => {
                log::warn!("Health Check H1 read failed: {} - {}", e, host);
                return (false, false);
            }
        }
    };
    HC_CONNECTOR.release_http_session(http_session, &peer, None).await;
    ((200..500).contains(&status), is_h2)
}
