use crate::utils::metrics::LOGGING_ERRORS;
use crate::utils::types::AppConfig;
use log::{error, info, warn, LevelFilter};
use log4rs::append::rolling_file::policy::compound::roll::fixed_window::FixedWindowRoller;
use log4rs::append::rolling_file::policy::compound::trigger::size::SizeTrigger;
use log4rs::append::rolling_file::policy::compound::CompoundPolicy;
use log4rs::append::rolling_file::RollingFileAppender;
use log4rs::config::Logger;
use log4rs::{
    append::console::ConsoleAppender,
    config::{Appender, Config as Log4rsConfig, Root},
    encode::pattern::PatternEncoder,
};
use pingora_cache::CachePhase;
use pingora_http::Version;
use pingora_proxy::Session;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::OnceLock;
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct LogMessage {
    pub response_code: u16,
    pub summary: String,
    pub client_ip: IpAddr,
    pub version: Version,
    pub user_agent: String,
    pub cache_status: CachePhase,
}
static LOG_SENDER: OnceLock<mpsc::Sender<LogMessage>> = OnceLock::new();
static ACCESS_LOG: OnceLock<LogLevel> = OnceLock::new();
const LOG_BUFFER: usize = 16384;

pub fn log_builder(conf: &AppConfig, location: &Option<String>) {
    let log_level = match conf.log_level.as_str() {
        "info" => LevelFilter::Info,
        "error" => LevelFilter::Error,
        "warn" => LevelFilter::Warn,
        "debug" => LevelFilter::Debug,
        "trace" => LevelFilter::Trace,
        "off" => LevelFilter::Off,
        _ => {
            println!("Error reading log level, defaulting to: INFO");
            LevelFilter::Info
        }
    };

    let mut pat: String = "{d(%Y-%m-%d %H:%M:%S)} {l} - {m} {n}".to_string();
    if let Some(ptrn) = conf.log_pattern.clone() {
        pat = ptrn;
    }
    let pattern = pat.as_str();

    if let Some(location) = location {
        let parts: Vec<&str> = location.splitn(4, ',').map(|s| s.trim()).collect();

        let path = parts.get(0).expect("Syntax error, could not get path for log files");
        let compress = parts.get(3).unwrap_or(&"No");
        let size_mb: u64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(100);
        let keep: u32 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);

        let pattern_str = match compress {
            &"compress" => format!("{}.{{}}.gz", path),
            _ => format!("{}.{{}}", path),
        };

        let roller = FixedWindowRoller::builder().build(pattern_str.as_str(), keep).unwrap();
        let trigger = SizeTrigger::new(size_mb * 1024 * 1024);
        let policy = CompoundPolicy::new(Box::new(trigger), Box::new(roller));

        let file = RollingFileAppender::builder()
            .encoder(Box::new(PatternEncoder::new(pattern)))
            .build(path, Box::new(policy))
            .unwrap();

        let config = Log4rsConfig::builder()
            .appender(Appender::builder().build("file", Box::new(file)))
            .logger(Logger::builder().build("pingora_proxy", LevelFilter::Off))
            .build(Root::builder().appender("file").build(log_level))
            .unwrap();
        log4rs::init_config(config).unwrap();
        info!("Logging to: {}, Max file size: {}mb, Files to keep: {}, compression: {} ", path, size_mb, keep, compress);
    } else {
        let stdout = ConsoleAppender::builder().encoder(Box::new(PatternEncoder::new(pattern))).build();

        let config = Log4rsConfig::builder()
            .appender(Appender::builder().build("stdout", Box::new(stdout)))
            .logger(Logger::builder().build("pingora_proxy", LevelFilter::Off))
            .build(Root::builder().appender("stdout").build(log_level))
            .unwrap();
        log4rs::init_config(config).unwrap();
        info!("No files are configured, logging to stdout");
    }
}

pub fn init_access_log(level_str: &str) {
    let level = LogLevel::from_str(level_str);
    let _ = ACCESS_LOG.set(level);
}

#[derive(Debug)]
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

#[derive(Debug)]
pub enum MatchStatus {
    Ok2xx,
    Er4xx,
    Er5xx,
}
impl MatchStatus {
    #[inline]
    pub fn from_code(code: u16) -> Self {
        match code {
            100..=399 => Self::Ok2xx,
            400..=499 => Self::Er4xx,
            _ => Self::Er5xx,
        }
    }
}
pub fn access_log(response_code: u16, summary: &str, session: &Session) {
    let level = ACCESS_LOG.get().unwrap_or(&LogLevel::None);
    let status = MatchStatus::from_code(response_code);
    let should_log = match level {
        LogLevel::Access => true,
        LogLevel::None => false,
        LogLevel::Error => matches!(status, MatchStatus::Er5xx),
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
    let log = LogMessage {
        response_code,
        summary: summary.to_owned(),
        client_ip: ip,
        version: session.req_header().version,
        user_agent: user_agent.to_owned(),
        cache_status: session.cache.phase(),
    };

    if let Some(sender) = LOG_SENDER.get() {
        let sender = sender;
        if let Err(_) = sender.try_send(log) {
            LOGGING_ERRORS.inc();
        }
    }
}

pub fn init_logging(enabled: Option<String>) {
    if let Some(_) = enabled {
        LOGGING_ERRORS.set(0);
        info!("Enabling {:?} log, with buffer of {} messages", ACCESS_LOG.get().unwrap_or(&LogLevel::None), LOG_BUFFER);
        let (ltx, lrx) = mpsc::channel(LOG_BUFFER);
        LOG_SENDER.set(ltx).unwrap();
        std::thread::spawn(move || log_receiver(lrx));
    }
}

pub fn log_receiver(mut receiver: mpsc::Receiver<LogMessage>) {
    while let Some(msg) = receiver.blocking_recv() {
        match MatchStatus::from_code(msg.response_code) {
            MatchStatus::Ok2xx => info!(
                "{}, {}, {}, client: {}, version: {:?}, useragent: {}",
                msg.response_code,
                msg.cache_status.as_str(),
                msg.summary,
                msg.client_ip,
                msg.version,
                msg.user_agent,
            ),
            MatchStatus::Er4xx => warn!(
                "{}, {}, {}, client: {}, version: {:?}, useragent: {}",
                msg.response_code,
                msg.cache_status.as_str(),
                msg.summary,
                msg.client_ip,
                msg.version,
                msg.user_agent,
            ),
            MatchStatus::Er5xx => error!(
                "{}, {}, {}, client: {}, version: {:?}, useragent: {}",
                msg.response_code,
                msg.cache_status.as_str(),
                msg.summary,
                msg.client_ip,
                msg.version,
                msg.user_agent,
            ),
        }
    }
}
