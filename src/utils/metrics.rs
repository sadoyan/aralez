use crate::utils::lazylock::EVICTION;
use pingora_cache::eviction::EvictionManager;
use pingora_http::Method;
use pingora_http::StatusCode;
use pingora_http::Version;
use prometheus::{register_histogram, register_int_counter, register_int_counter_vec, register_int_gauge, Histogram, IntCounter, IntCounterVec, IntGauge};
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

pub struct MetricTypes {
    pub method: Method,
    pub upstream: Arc<str>,
    pub code: Option<StatusCode>,
    pub latency: Duration,
    pub version: Version,
}

pub static OPEN_FILES: LazyLock<IntGauge> = LazyLock::new(|| register_int_gauge!("aralez_open_files", "Number of open file descriptors").unwrap());
pub static LOGGING_ERRORS: LazyLock<IntGauge> = LazyLock::new(|| register_int_gauge!("aralez_logging_errors", "Number of log errors").unwrap());
pub static MEMORY_USAGE: LazyLock<IntGauge> = LazyLock::new(|| register_int_gauge!("aralez_memory_bytes", "Total memory allocated in bytes").unwrap());
pub static ACTIVE_SESSIONS: LazyLock<IntGauge> = LazyLock::new(|| register_int_gauge!("aralez_active_sessions", "Current number of active sessions").unwrap());
pub static REQUEST_COUNT: LazyLock<IntCounter> = LazyLock::new(|| register_int_counter!("aralez_requests_total", "Total number of requests handled by Aralez").unwrap());

pub static CACHE_SIZE_BYTES: LazyLock<IntGauge> = LazyLock::new(|| register_int_gauge!("aralez_cache_size_bytes", "Current cache size in bytes").unwrap());
pub static CACHE_ITEMS: LazyLock<IntGauge> = LazyLock::new(|| register_int_gauge!("aralez_cache_items", "Current number of cached objects").unwrap());
pub static CACHE_EVICTED_BYTES: LazyLock<IntGauge> = LazyLock::new(|| register_int_gauge!("aralez_cache_evicted_bytes_total", "Total bytes evicted from cache").unwrap());

pub static CACHE_EVICTED_ITEMS: LazyLock<IntGauge> = LazyLock::new(|| register_int_gauge!("aralez_cache_evicted_items_total", "Total cache items evicted").unwrap());
pub static RESPONSE_CODES: LazyLock<IntCounterVec> =
    LazyLock::new(|| register_int_counter_vec!("aralez_responses_total", "Responses grouped by status code", &["status"]).unwrap());

pub static RESPONSE_LATENCY: LazyLock<Histogram> = LazyLock::new(|| {
    register_histogram!(
        "aralez_response_latency_seconds",
        "Response latency in seconds",
        vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0]
    )
    .unwrap()
});

pub static REQUESTS_BY_METHOD: LazyLock<IntCounterVec> =
    LazyLock::new(|| register_int_counter_vec!("aralez_requests_by_method_total", "Number of requests by HTTP method", &["method"]).unwrap());

pub static REQUESTS_BY_UPSTREAM: LazyLock<IntCounterVec> =
    LazyLock::new(|| register_int_counter_vec!("aralez_requests_by_upstream", "Number of requests by UPSTREAM server", &["upstream"]).unwrap());

pub static REQUESTS_BY_VERSION: LazyLock<IntCounterVec> =
    LazyLock::new(|| register_int_counter_vec!("aralez_requests_by_version_total", "Number of requests by HTTP versions", &["version"]).unwrap());
pub static HTTP_11_COUNT: LazyLock<IntCounter> = LazyLock::new(|| REQUESTS_BY_VERSION.with_label_values(&["HTTP/1.1"]));
pub static HTTP_20_COUNT: LazyLock<IntCounter> = LazyLock::new(|| REQUESTS_BY_VERSION.with_label_values(&["HTTP/2.0"]));
pub static HTTP_30_COUNT: LazyLock<IntCounter> = LazyLock::new(|| REQUESTS_BY_VERSION.with_label_values(&["HTTP/3.0"]));
pub static HTTP_10_COUNT: LazyLock<IntCounter> = LazyLock::new(|| REQUESTS_BY_VERSION.with_label_values(&["HTTP/1.0"]));
pub static HTTP_UNK_COUNT: LazyLock<IntCounter> = LazyLock::new(|| REQUESTS_BY_VERSION.with_label_values(&["Unknown"]));

pub static METHOD_GET: LazyLock<IntCounter> = LazyLock::new(|| REQUESTS_BY_METHOD.with_label_values(&["GET"]));
pub static METHOD_POST: LazyLock<IntCounter> = LazyLock::new(|| REQUESTS_BY_METHOD.with_label_values(&["POST"]));
pub static METHOD_PUT: LazyLock<IntCounter> = LazyLock::new(|| REQUESTS_BY_METHOD.with_label_values(&["PUT"]));
pub static METHOD_DELETE: LazyLock<IntCounter> = LazyLock::new(|| REQUESTS_BY_METHOD.with_label_values(&["DELETE"]));
pub static METHOD_HEAD: LazyLock<IntCounter> = LazyLock::new(|| REQUESTS_BY_METHOD.with_label_values(&["HEAD"]));
pub static METHOD_OPTIONS: LazyLock<IntCounter> = LazyLock::new(|| REQUESTS_BY_METHOD.with_label_values(&["OPTIONS"]));
pub static METHOD_PATCH: LazyLock<IntCounter> = LazyLock::new(|| REQUESTS_BY_METHOD.with_label_values(&["PATCH"]));
pub static METHOD_OTHER: LazyLock<IntCounter> = LazyLock::new(|| REQUESTS_BY_METHOD.with_label_values(&["OTHER"]));

pub static STATUS_100: LazyLock<IntCounter> = LazyLock::new(|| RESPONSE_CODES.with_label_values(&["100"]));
pub static STATUS_101: LazyLock<IntCounter> = LazyLock::new(|| RESPONSE_CODES.with_label_values(&["101"]));
pub static STATUS_200: LazyLock<IntCounter> = LazyLock::new(|| RESPONSE_CODES.with_label_values(&["200"]));
pub static STATUS_204: LazyLock<IntCounter> = LazyLock::new(|| RESPONSE_CODES.with_label_values(&["204"]));
pub static STATUS_301: LazyLock<IntCounter> = LazyLock::new(|| RESPONSE_CODES.with_label_values(&["301"]));
pub static STATUS_302: LazyLock<IntCounter> = LazyLock::new(|| RESPONSE_CODES.with_label_values(&["302"]));
pub static STATUS_304: LazyLock<IntCounter> = LazyLock::new(|| RESPONSE_CODES.with_label_values(&["304"]));
pub static STATUS_400: LazyLock<IntCounter> = LazyLock::new(|| RESPONSE_CODES.with_label_values(&["400"]));
pub static STATUS_403: LazyLock<IntCounter> = LazyLock::new(|| RESPONSE_CODES.with_label_values(&["403"]));
pub static STATUS_404: LazyLock<IntCounter> = LazyLock::new(|| RESPONSE_CODES.with_label_values(&["404"]));
pub static STATUS_500: LazyLock<IntCounter> = LazyLock::new(|| RESPONSE_CODES.with_label_values(&["500"]));
pub static STATUS_501: LazyLock<IntCounter> = LazyLock::new(|| RESPONSE_CODES.with_label_values(&["501"]));
pub static STATUS_502: LazyLock<IntCounter> = LazyLock::new(|| RESPONSE_CODES.with_label_values(&["502"]));
pub static STATUS_503: LazyLock<IntCounter> = LazyLock::new(|| RESPONSE_CODES.with_label_values(&["503"]));

pub fn calc_metrics(metric_types: &MetricTypes) {
    REQUEST_COUNT.inc();
    match metric_types.version {
        Version::HTTP_11 => HTTP_11_COUNT.inc(),
        Version::HTTP_2 => HTTP_20_COUNT.inc(),
        Version::HTTP_3 => HTTP_30_COUNT.inc(),
        Version::HTTP_10 => HTTP_10_COUNT.inc(),
        _ => HTTP_UNK_COUNT.inc(),
    };
    match metric_types.method.as_str() {
        "GET" => METHOD_GET.inc(),
        "POST" => METHOD_POST.inc(),
        "PUT" => METHOD_PUT.inc(),
        "DELETE" => METHOD_DELETE.inc(),
        "HEAD" => METHOD_HEAD.inc(),
        "OPTIONS" => METHOD_OPTIONS.inc(),
        "PATCH" => METHOD_PATCH.inc(),
        _ => METHOD_OTHER.inc(),
    }
    let code = metric_types.code.unwrap_or(StatusCode::GONE);
    match code.as_u16() {
        100 => STATUS_100.inc(),
        101 => STATUS_101.inc(),
        200 => STATUS_200.inc(),
        204 => STATUS_204.inc(),
        301 => STATUS_301.inc(),
        302 => STATUS_302.inc(),
        304 => STATUS_304.inc(),
        400 => STATUS_400.inc(),
        403 => STATUS_403.inc(),
        404 => STATUS_404.inc(),
        500 => STATUS_500.inc(),
        501 => STATUS_501.inc(),
        502 => STATUS_502.inc(),
        503 => STATUS_503.inc(),
        _ => RESPONSE_CODES.with_label_values(&[code.as_str()]).inc(), // Fallback for rare status codes
    }

    REQUESTS_BY_UPSTREAM.with_label_values(&[metric_types.upstream.as_ref()]).inc();
    RESPONSE_LATENCY.observe(metric_types.latency.as_secs_f64());

    // if let Some(eviction) = EVICTION.get() {
    //     CACHE_SIZE_BYTES.set(eviction.total_size() as i64);
    //     CACHE_ITEMS.set(eviction.total_items() as i64);
    //     CACHE_EVICTED_BYTES.set(eviction.evicted_size() as i64);
    //     CACHE_EVICTED_ITEMS.set(eviction.evicted_items() as i64);
    // }
}

pub(crate) fn get_memory_usage() -> usize {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<usize>().ok())
        })
        .unwrap_or(0)
        * 1024
}

pub fn get_open_files() -> usize {
    std::fs::read_dir("/proc/self/fd").map(|dir| dir.count()).unwrap_or(0)
}

pub async fn calc_cache_metrics() {
    loop {
        if let Some(eviction) = EVICTION.get() {
            CACHE_SIZE_BYTES.set(eviction.total_size() as i64);
            CACHE_ITEMS.set(eviction.total_items() as i64);
            CACHE_EVICTED_BYTES.set(eviction.evicted_size() as i64);
            CACHE_EVICTED_ITEMS.set(eviction.evicted_items() as i64);
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}
