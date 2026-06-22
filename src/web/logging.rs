use log::info;
use pingora_proxy::Session;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::OnceLock;

pub static ACCESS_LOG: OnceLock<LogLevel> = OnceLock::new();

pub fn init_access_log(level_str: &str) {
    let level = LogLevel::from_str(level_str);
    let _ = ACCESS_LOG.set(level);
}

pub enum LogLevel {
    Access,
    Error,
    None,
}

impl LogLevel {
    pub fn from_str(s: &str) -> Self {
        match s {
            "all" => LogLevel::Access,
            "error" => LogLevel::Error,
            _ => LogLevel::None,
        }
    }
}

pub fn access_log(response_code: u16, summary: &str, session: &Session) {
    let level = ACCESS_LOG.get().unwrap_or(&LogLevel::None);

    let should_log = match level {
        LogLevel::Access => true,
        LogLevel::None => false,
        LogLevel::Error => !(100..=399).contains(&response_code),
    };

    if !should_log {
        return;
    }

    let ip = session
        .client_addr()
        .and_then(|addr| addr.as_inet())
        .map(|addr| addr.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));

    let user_agent = session.req_header().headers.get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("-");

    info!(
        "{}, response code: {response_code}, client: {}, version: {:?}, useragent: {}",
        summary,
        ip,
        session.req_header().version,
        user_agent,
    );
}
