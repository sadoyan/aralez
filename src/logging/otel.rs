use crate::logging::core::StructuredSystemLog;
use crate::logging::types::{LogBackendPlugin, WriteLog};
use async_trait::async_trait;
use log::Level;
use opentelemetry::logs::{AnyValue, LogRecord as _, Logger, LoggerProvider as _, Severity};
use opentelemetry_otlp::WithTonicConfig;
use opentelemetry_otlp::{LogExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::{BatchConfigBuilder, BatchLogProcessor, SdkLoggerProvider};
use std::collections::HashMap;
use std::env;
use std::sync::LazyLock;
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc;
use tonic::metadata::MetadataMap;

pub struct Otel;

#[derive(Debug, Clone)]
pub struct OtelLogConfig {
    pub grpc_endpoint: String,
    pub http_endpoint: String,
    pub protocol: OtelProtocol,
    pub service_name: String,
    pub batch_delay_ms: u64,
    pub batch_max_size: usize,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtelProtocol {
    Grpc,
    Http,
}

static OTEL: LazyLock<OtelLogConfig> = LazyLock::new(OtelLogConfig::from_env);

fn parse_headers_env(var_name: &str) -> HashMap<String, String> {
    env::var(var_name)
        .unwrap_or_default()
        .split(',')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            let key = k.trim();
            let val = v.trim();
            if key.is_empty() || val.is_empty() {
                return None;
            }
            Some((key.to_string(), val.to_string()))
        })
        .collect()
}

fn build_exporter(cfg: &OtelLogConfig) -> Result<LogExporter, Box<dyn std::error::Error + Send + Sync>> {
    match cfg.protocol {
        OtelProtocol::Grpc => {
            let mut builder = LogExporter::builder().with_tonic().with_endpoint(cfg.grpc_endpoint.clone());

            if !cfg.headers.is_empty() {
                let mut metadata = MetadataMap::new();
                for (k, v) in &cfg.headers {
                    if let (Ok(key), Ok(val)) = (
                        k.to_lowercase().parse::<tonic::metadata::MetadataKey<tonic::metadata::Ascii>>(),
                        v.parse::<tonic::metadata::MetadataValue<tonic::metadata::Ascii>>(),
                    ) {
                        metadata.insert(key, val);
                    } else {
                        eprintln!("Skipping invalid gRPC metadata header '{k}'");
                    }
                }
                builder = builder.with_metadata(metadata);
            }

            Ok(builder.build()?)
        }
        OtelProtocol::Http => {
            let mut builder = LogExporter::builder().with_http().with_endpoint(cfg.http_endpoint.clone());

            if !cfg.headers.is_empty() {
                builder = builder.with_headers(cfg.headers.clone());
            }

            Ok(builder.build()?)
        }
    }
}
impl OtelLogConfig {
    pub fn from_env() -> Self {
        let protocol = match env::var("LOG_OTEL_PROTOCOL").unwrap_or_default().trim().to_ascii_lowercase().as_str() {
            "http" => OtelProtocol::Http,
            "grpc" | "" => OtelProtocol::Grpc,
            other => {
                eprintln!("Unknown LOG_OTEL_PROTOCOL '{other}', defaulting to grpc");
                OtelProtocol::Grpc
            }
        };

        let batch_max_size = env::var("LOG_OTEL_BATCH_SIZE")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|&n| n > 0)
            .unwrap_or(512);

        let batch_delay_ms = env::var("LOG_OTEL_DELAY").ok().and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(2000);

        Self {
            grpc_endpoint: env::var("LOG_OTEL_GRPC_HOST").unwrap_or_else(|_| "http://localhost:4317".to_string()),
            http_endpoint: env::var("LOG_OTEL_HTTP_HOST").unwrap_or_else(|_| "http://localhost:4318".to_string()),
            service_name: env::var("LOG_OTEL_SERVICE_NAME").unwrap_or_else(|_| "aralez".to_string()),
            headers: parse_headers_env("LOG_OTEL_HEADERS"),
            protocol,
            batch_delay_ms,
            batch_max_size,
        }
    }
}

fn build_logger_provider() -> Result<SdkLoggerProvider, Box<dyn std::error::Error + Send + Sync>> {
    let exporter = build_exporter(&OTEL)?;

    let resource = Resource::builder().with_service_name(OTEL.service_name.clone()).build();

    let batch_config = BatchConfigBuilder::default()
        .with_scheduled_delay(Duration::from_millis(OTEL.batch_delay_ms))
        .with_max_export_batch_size(OTEL.batch_max_size)
        .build();

    let batch_processor = BatchLogProcessor::builder(exporter).with_batch_config(batch_config).build();

    let provider = SdkLoggerProvider::builder().with_resource(resource).with_log_processor(batch_processor).build();

    Ok(provider)
}

#[async_trait]
impl WriteLog for Otel {
    async fn run(&self, mut rx: mpsc::Receiver<StructuredSystemLog>) {
        let provider = match build_logger_provider() {
            Ok(p) => p,
            Err(err) => {
                eprintln!("Failed to initialize OTLP logger provider: {err}");
                while rx.recv().await.is_some() {}
                return;
            }
        };

        let logger = provider.logger("aralez.logging");
        while let Some(msg) = rx.recv().await {
            let severity = match msg.level {
                Level::Error => Severity::Error,
                Level::Warn => Severity::Warn,
                Level::Info => Severity::Info,
                Level::Debug => Severity::Debug,
                Level::Trace => Severity::Trace,
            };

            let now = SystemTime::now();
            let mut record = logger.create_log_record();
            record.set_timestamp(now);
            record.set_observed_timestamp(now);
            record.set_severity_number(severity);
            record.set_severity_text(msg.level.as_str());
            record.set_body(AnyValue::String(msg.message.into()));
            record.add_attribute("target", msg.target);

            logger.emit(record);
        }
        let flush_provider = provider.clone();
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || {
                let _ = flush_provider.force_flush();
            }),
        )
        .await;
        let _ = provider.shutdown();
    }
}

inventory::submit! {
    LogBackendPlugin {
        name: "otel",
        factory: || Box::new(Otel),
    }
}
