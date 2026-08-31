use crate::utils::consul::ConsulService;
use crate::utils::kuberconsul::match_path;
use crate::utils::kubernetes::KubeEndpoints;
use crate::utils::structs::{GlobalServiceMapping, InnerMap};
use ahash::HashMap;
use dashmap::DashMap;
use pingora_core::connectors::http::Connector;
use pingora_core::listeners::ALPN;
use pingora_core::prelude::HttpPeer;
use pingora_http::RequestHeader;
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

pub static CONNECTOR: LazyLock<Connector> = LazyLock::new(|| Connector::new(None));

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ConsulServicesInternal {
    #[serde(rename = "aralez.host")]
    pub host: String,
    #[serde(rename = "aralez.path")]
    pub path: String,
    #[serde(rename = "aralez.auth")]
    pub auth: Option<String>,
    #[serde(rename = "aralez.redirect")]
    pub redirect: Option<String>,
    #[serde(rename = "aralez.rate")]
    pub rate: Option<isize>,
    #[serde(rename = "aralez.4xx_rate")]
    pub xrate: Option<u32>,
    #[serde(rename = "aralez.client_headers")]
    pub client_headers: Option<Vec<String>>,
    #[serde(rename = "aralez.server_headers")]
    pub server_headers: Option<Vec<String>>,
    #[serde(rename = "aralez.to_https")]
    pub to_https: Option<bool>,
}
pub type ConsulServices = HashMap<String, ConsulServicesInternal>;
pub async fn for_consul_list(url: &str, token: Option<String>) -> Option<ConsulServices> {
    if let Some(data) = getfromapi(url, token, "consul").await {
        let yo = parse_services(data);
        if let Ok(y) = yo {
            return Some(y);
        }
        return None;
    }
    None
}

fn parse_services(json: Vec<u8>) -> Result<ConsulServices, serde_json::Error> {
    let raw: HashMap<String, Vec<String>> = serde_json::from_slice(&json)?;
    Ok(raw
        .into_iter()
        .map(|(service, tags)| {
            let mut internal = ConsulServicesInternal::default();
            for tag in tags {
                if let Some((key, value)) = tag.split_once('=') {
                    match key {
                        "aralez.host" => internal.host = value.to_string(),
                        "aralez.path" => internal.path = value.to_string(),
                        "aralez.rate" => internal.rate = value.parse::<isize>().ok(),
                        "aralez.4xx_rate" => internal.xrate = value.parse::<u32>().ok(),
                        "aralez.to_https" => internal.to_https = value.parse::<bool>().ok(),
                        "aralez.auth" => internal.auth = Option::from(value.to_string()),
                        "aralez.redirect" => internal.redirect = Option::from(value.to_string()),
                        "aralez.client_header" => internal.client_headers.get_or_insert_default().push(value.to_string()),
                        "aralez.server_header" => internal.server_headers.get_or_insert_default().push(value.to_string()),
                        _ => {}
                    }
                }
            }

            (service, internal)
        })
        .collect())
}
pub async fn for_consul(url: &str, token: Option<String>, conf: &GlobalServiceMapping) -> Option<DashMap<Arc<str>, (Vec<Arc<InnerMap>>, AtomicUsize)>> {
    if let Some(data) = getfromapi(url, token, "consul").await {
        let endpoints: Vec<ConsulService> = serde_json::from_slice(&data).ok()?;
        let mut inner_vec = Vec::new();
        let upstreams: DashMap<Arc<str>, (Vec<Arc<InnerMap>>, AtomicUsize)> = DashMap::new();
        for subsets in endpoints {
            let to_add = Arc::from(InnerMap {
                address: Arc::from(&*subsets.address),
                port: subsets.port,
                is_ssl: false,
                is_http2: false,
                to_https: conf.to_https.unwrap_or(false),
                rate_limit: conf.rate_limit,
                x4xx_limit: conf.x4xx_limit,
                redirect_to: conf.redirect_to.clone().map(Arc::<str>::from),
                healthcheck: None,
                authorization: None,
            });
            inner_vec.push(to_add);
        }
        match_path(conf, &upstreams, inner_vec);
        return Some(upstreams);
    };
    None
}

pub async fn for_kuber(url: &str, token: &str, conf: &GlobalServiceMapping) -> Option<DashMap<Arc<str>, (Vec<Arc<InnerMap>>, AtomicUsize)>> {
    if let Some(data) = getfromapi(url, Some(token.to_string()), "kubernetes").await {
        let endpoints: KubeEndpoints = serde_json::from_slice(&data).ok()?;
        let upstreams: DashMap<Arc<str>, (Vec<Arc<InnerMap>>, AtomicUsize)> = DashMap::new();
        let mut inner_vec = Vec::new();

        if let Some(subsets) = endpoints.subsets {
            for subset in subsets {
                if let (Some(addresses), Some(ports)) = (subset.addresses, subset.ports) {
                    for addr in addresses {
                        for port in &ports {
                            let to_add = Arc::from(InnerMap {
                                address: Arc::from(addr.ip.as_str()),
                                port: port.port,
                                is_ssl: false,
                                is_http2: false,
                                to_https: conf.to_https.unwrap_or(false),
                                rate_limit: conf.rate_limit,
                                x4xx_limit: conf.x4xx_limit,
                                healthcheck: None,
                                redirect_to: None,
                                authorization: None,
                            });
                            inner_vec.push(to_add);
                        }
                    }
                }
            }
        }
        if !inner_vec.is_empty() {
            match_path(conf, &upstreams, inner_vec);
            return Some(upstreams);
        }
    }
    None
}
pub async fn getfromapi(url: &str, token: Option<String>, provider: &str) -> Option<Vec<u8>> {
    let (host, port, path, is_tls) = parse_url(&url).ok()?;

    let mut peer = HttpPeer::new((host, port), is_tls, host.to_string());
    peer.options.total_connection_timeout = Some(Duration::from_secs(5));
    peer.options.read_timeout = Some(Duration::from_secs(5));

    if is_tls {
        peer.options.verify_cert = false;
        peer.options.verify_hostname = false;
        peer.options.alpn = ALPN::H2H1;
    }

    let mut http_session = CONNECTOR.get_http_session(&peer).await.ok()?;
    let mut req = RequestHeader::build("GET", path.as_bytes(), None).ok()?;

    let host_header = if (is_tls && port == 443) || (!is_tls && port == 80) {
        host.to_string()
    } else {
        format!("{}:{}", host, port)
    };
    req.insert_header("Host", host_header).ok()?;
    req.insert_header("Accept", "application/json").ok()?;

    match provider {
        "consul" => {
            if let Some(token) = token {
                req.insert_header("X-Consul-Token", token).ok()?;
            }
        }
        "kubernetes" => {
            if let Some(token) = token {
                req.insert_header("Authorization", format!("Bearer {}", token)).ok()?;
            }
        }
        _ => {}
    }

    if http_session.0.write_request_header(Box::new(req)).await.is_err() {
        CONNECTOR.release_http_session(http_session.0, &peer, None).await;
        return None;
    }

    let status = match http_session.0.read_response_header().await {
        Ok(_) => http_session.0.response_header().map(|r| r.status.as_u16()).unwrap_or(500),
        Err(e) => {
            log::warn!("API call failed to read response header for {}: {}", url, e);
            500
        }
    };

    let mut body_bytes = Vec::new();
    if status == 200 {
        while let Ok(Some(chunk)) = http_session.0.read_response_body().await {
            body_bytes.extend_from_slice(&chunk);
        }
    }

    CONNECTOR.release_http_session(http_session.0, &peer, None).await;

    if status == 200 && !body_bytes.is_empty() {
        Some(body_bytes)
    } else {
        None
    }
}

fn parse_url(url: &str) -> Result<(&str, u16, &str, bool), &'static str> {
    let is_https = url.starts_with("https://");
    let default_port = if is_https { 443 } else { 80 };

    let no_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);

    let (authority, uri) = no_scheme.find('/').map_or((no_scheme, "/"), |i| (&no_scheme[..i], &no_scheme[i..]));

    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => {
            let port_num = p.parse::<u16>().map_err(|_| "Invalid port number")?;
            (h, port_num)
        }
        None => (authority, default_port),
    };

    if host.is_empty() {
        return Err("Empty host in URL");
    }

    Ok((host, port, uri, is_https))
}

// fn base64_decode(encoded: &str) -> Option<String> {
//     let decoded_bytes = general_purpose::STANDARD.decode(encoded).unwrap_or_default();
//     let decoded: Result<HashMap<String, String>, serde_json::Error> = serde_json::from_slice(&decoded_bytes);
//     println!("Decoded: {:?}", decoded);
//     None
// }
