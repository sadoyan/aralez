use crate::logging::types::start_logging_backend;
use crate::utils::metrics::LOGGING_ERRORS;
use crate::utils::types::AppConfig;
use anyhow::Result;
use log::{error, info, log, Level, LevelFilter, Record};
use log4rs::append::rolling_file::policy::compound::roll::fixed_window::FixedWindowRoller;
use log4rs::append::rolling_file::policy::compound::trigger::size::SizeTrigger;
use log4rs::append::rolling_file::policy::compound::CompoundPolicy;
use log4rs::append::rolling_file::RollingFileAppender;
use log4rs::append::Append;
use log4rs::config::Logger;
use log4rs::{
    append::console::ConsoleAppender,
    config::{Appender, Config as Log4rsConfig, Root},
    encode::pattern::PatternEncoder,
};
use pingora_cache::CachePhase;
use pingora_http::Version;
use pingora_proxy::Session;
use serde::Serialize;
use std::cell::RefCell;
use std::fmt::Write;
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

#[derive(Debug, Serialize, Clone)]
pub struct StructuredSystemLog {
    pub target: &'static str,
    pub level: Level,
    pub message: String,
}
thread_local! {
    static LOG_BUF: RefCell<String> = RefCell::new(String::with_capacity(512));
}

static ACCESS_LOG_SENDER: OnceLock<mpsc::Sender<LogMessage>> = OnceLock::new();
pub(crate) static SYSTEM_LOG_SENDER: OnceLock<mpsc::Sender<StructuredSystemLog>> = OnceLock::new();
static ACCESS_LOG: OnceLock<LogLevel> = OnceLock::new();
const LOG_BUFFER: usize = 16384;
static IS_STRUCTURED: OnceLock<bool> = OnceLock::new();
static LOG_BACKEND: OnceLock<&'static str> = OnceLock::new();

pub fn set_backend(key: String) -> Result<(), &'static str> {
    let static_key: &'static str = Box::leak(key.into_boxed_str());
    LOG_BACKEND.set(static_key).map_err(|_| "LOG_BACKEND was already initialized!")
}

pub fn get_backend() -> &'static str {
    LOG_BACKEND.get().copied().unwrap_or("system")
}

#[derive(Debug)]
pub struct StructuredChannelAppender {
    sender: mpsc::Sender<StructuredSystemLog>,
}

impl StructuredChannelAppender {
    pub fn new(sender: mpsc::Sender<StructuredSystemLog>) -> Self {
        Self { sender }
    }
}

impl Append for StructuredChannelAppender {
    fn append(&self, record: &Record) -> Result<()> {
        let message = LOG_BUF.with(|buf| {
            let mut b = buf.borrow_mut();
            b.clear();
            let _ = write!(&mut *b, "{}", record.args());
            b.clone()
        });

        let target: &'static str = unsafe { std::mem::transmute(record.target()) };

        let msg = StructuredSystemLog {
            target,
            level: record.level(),
            message,
        };

        let _ = self.sender.try_send(msg);
        Ok(())
    }

    fn flush(&self) {}
}
pub async fn log_builder(conf: &AppConfig, location: &Option<String>) {
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

    if let Some(backend) = conf.log_structured.as_deref() {
        if let Some(backend_name) = backend.split_whitespace().next() {
            let _ = set_backend(backend_name.to_string());
            init_structured_log().await;
        }
        let syslog_appender: Option<Box<dyn Append>> = SYSTEM_LOG_SENDER
            .get()
            .cloned()
            .map(|sender| Box::new(StructuredChannelAppender::new(sender)) as Box<dyn Append>);

        let stdout = ConsoleAppender::builder().encoder(Box::new(PatternEncoder::new(pattern))).build();

        let mut config_builder = Log4rsConfig::builder().appender(Appender::builder().build("stdout", Box::new(stdout)));

        if let Some(appender) = syslog_appender {
            config_builder = config_builder
                .appender(Appender::builder().build("syslog_appender", appender))
                .logger(Logger::builder().appender("syslog_appender").additive(false).build("pingora_proxy", log_level))
                .logger(Logger::builder().appender("syslog_appender").additive(false).build("pingora_core", log_level))
                .logger(Logger::builder().appender("syslog_appender").additive(false).build("pingora_pool", log_level))
                .logger(Logger::builder().appender("syslog_appender").additive(false).build("pingora_cache", log_level))
                .logger(Logger::builder().appender("syslog_appender").additive(false).build("hyper_util", log_level))
                .logger(Logger::builder().appender("syslog_appender").additive(false).build("aralez", log_level))
                .logger(Logger::builder().appender("syslog_appender").additive(false).build("h2", log_level));
        } else {
            config_builder = config_builder
                .logger(Logger::builder().build("pingora_core", LevelFilter::Off))
                .logger(Logger::builder().build("pingora_pool", LevelFilter::Off))
                .logger(Logger::builder().build("pingora_cache", LevelFilter::Off))
                .logger(Logger::builder().build("hyper_util", LevelFilter::Off))
                .logger(Logger::builder().build("aralez", LevelFilter::Off))
                .logger(Logger::builder().build("h2", LevelFilter::Off));
        }

        let config = config_builder.build(Root::builder().appender("stdout").build(log_level)).unwrap();

        if let Err(e) = log4rs::init_config(config) {
            eprintln!("Warning: log4rs logger already initialized: {:?}", e);
        }
        info!("Enabling structured logging with backend : {}", backend);
        let _ = IS_STRUCTURED.set(true);
        return;
    }
    if let Some(location) = location {
        // let parts: Vec<&str> = location.splitn(4, ',').map(|s| s.trim()).collect();
        // let path = parts.get(0).expect("Syntax error, could not get path for log files");

        let parts: Vec<&str> = location.split(',').map(|s| s.trim()).collect();
        let path = match parts.get(0).filter(|s| !s.is_empty()) {
            Some(p) => p,
            None => {
                error!("Invalid log location string provided; falling back to stdout");
                return;
            }
        };

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

        let file = match RollingFileAppender::builder().encoder(Box::new(PatternEncoder::new(pattern))).build(path, Box::new(policy)) {
            Ok(f) => f,
            Err(e) => {
                error!("Failed to create log file at {}: {}. Falling back to stdout.", path, e);
                return;
            }
        };

        let config = Log4rsConfig::builder()
            .appender(Appender::builder().build("file", Box::new(file)))
            .logger(Logger::builder().build("pingora_proxy", LevelFilter::Off))
            .build(Root::builder().appender("file").build(log_level))
            .unwrap();
        if let Err(e) = log4rs::init_config(config) {
            eprintln!("Warning: log4rs logger already initialized: {:?}", e);
        }
        info!("Logging to: {}, Max file size: {}mb, Files to keep: {}, compression: {} ", path, size_mb, keep, compress);
    } else {
        let stdout = ConsoleAppender::builder().encoder(Box::new(PatternEncoder::new(pattern))).build();
        let config = Log4rsConfig::builder()
            .appender(Appender::builder().build("stdout", Box::new(stdout)))
            .logger(Logger::builder().build("pingora_proxy", LevelFilter::Off))
            .build(Root::builder().appender("stdout").build(log_level))
            .unwrap();
        if let Err(e) = log4rs::init_config(config) {
            eprintln!("Warning: log4rs logger already initialized: {:?}", e);
        }
        info!("No files are configured, logging to stdout");
    }
}

pub async fn init_access_log(level_str: &str) {
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

pub async fn access_log(response_code: u16, summary: &str, session: &Session) {
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

    let msg = LogMessage {
        response_code,
        summary: summary.to_owned(),
        client_ip: ip,
        version: session.req_header().version,
        user_agent: user_agent.to_owned(),
        cache_status: session.cache.phase(),
    };

    if IS_STRUCTURED.get().is_some() {
        write_access_log(&msg).await;
        return;
    }
    if let Some(sender) = ACCESS_LOG_SENDER.get() {
        if let Err(_) = sender.try_send(msg) {
            LOGGING_ERRORS.inc();
        }
    }
}

pub async fn init_access_logging(enabled: Option<String>) {
    if enabled.is_some() {
        LOGGING_ERRORS.set(0);
        info!("Enabling {:?} log, with buffer of {} messages", ACCESS_LOG.get().unwrap_or(&LogLevel::None), LOG_BUFFER);
        let (ltx, lrx) = mpsc::channel(LOG_BUFFER);
        let _ = ACCESS_LOG_SENDER.set(ltx);
        tokio::spawn(async move { access_log_receiver(lrx).await });
    }
}

pub async fn init_structured_log() {
    let (tx, rx) = mpsc::channel::<StructuredSystemLog>(LOG_BUFFER);
    let _ = SYSTEM_LOG_SENDER.set(tx);
    let backend = get_backend();
    // Start backend receiver directly without double-channel forwarding
    start_logging_backend(backend, rx);
}

pub async fn access_log_receiver(mut receiver: mpsc::Receiver<LogMessage>) {
    while let Some(msg) = receiver.recv().await {
        write_access_log(&msg).await;
    }
}

async fn write_access_log(msg: &LogMessage) {
    let level = match MatchStatus::from_code(msg.response_code) {
        MatchStatus::Ok2xx => Level::Info,
        MatchStatus::Er4xx => Level::Warn,
        MatchStatus::Er5xx => Level::Error,
    };
    log!(
        level,
        "{}, {}, {}, client: {}, version: {:?}, useragent: {}",
        msg.response_code,
        msg.cache_status.as_str(),
        msg.summary,
        msg.client_ip,
        msg.version,
        msg.user_agent,
    );
}
