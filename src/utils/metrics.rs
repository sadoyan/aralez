use http::method::Method;
use pingora_http::Version;
use prometheus::{register_histogram, register_int_counter, register_int_counter_vec, Histogram, IntCounter, IntCounterVec};
use std::sync::Arc;
use std::time::Duration;

pub struct MetricTypes {
    pub method: Method,
    pub upstream: Arc<str>,
    pub code: String,
    pub latency: Duration,
    pub version: Version,
}
lazy_static::lazy_static! {
    pub static ref REQUEST_COUNT: IntCounter = register_int_counter!(
        "aralez_requests_total",
        "Total number of requests handled by Aralez"
    ).unwrap();
      pub static ref RESPONSE_CODES: IntCounterVec = register_int_counter_vec!(
        "aralez_responses_total",
        "Responses grouped by status code",
        &["status"]
    ).unwrap();
    pub static ref REQUEST_LATENCY:  Histogram = register_histogram!(
        "aralez_request_latency_seconds",
        "Request latency in seconds",
        vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0]
    ).unwrap();
    pub static ref RESPONSE_LATENCY: Histogram = register_histogram!(
        "aralez_response_latency_seconds",
        "Response latency in seconds",
        vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0]
    ).unwrap();
    pub static ref REQUESTS_BY_METHOD: IntCounterVec = register_int_counter_vec!(
        "aralez_requests_by_method_total",
        "Number of requests by HTTP method",
        &["method"]
    ).unwrap();
    pub static ref REQUESTS_BY_UPSTREAM: IntCounterVec = register_int_counter_vec!(
        "aralez_requests_by_upstream",
        "Number of requests by UPSTREAM server",
        &["method"]
    ).unwrap();
    pub static ref REQUESTS_BY_VERSION: IntCounterVec = register_int_counter_vec!(
        "aralez_requests_by_version_total",
        "Number of requests by HTTP versions",
        &["version"]
    ).unwrap();
    pub static ref ERROR_COUNT: IntCounter = register_int_counter!(
        "aralez_errors_total",
        "Total number of errors"
    ).unwrap();
}

pub fn calc_metrics(metric_types: &MetricTypes) {
    REQUEST_COUNT.inc();
    let timer = REQUEST_LATENCY.start_timer();
    timer.observe_duration();

    let version_str = match &metric_types.version {
        &Version::HTTP_11 => "HTTP/1.1",
        &Version::HTTP_2 => "HTTP/2.0",
        &Version::HTTP_3 => "HTTP/3.0",
        &Version::HTTP_10 => "HTTP/1.0",
        _ => "Unknown",
    };
    REQUESTS_BY_VERSION.with_label_values(&[&version_str]).inc();
    RESPONSE_CODES.with_label_values(&[&metric_types.code]).inc();
    REQUESTS_BY_METHOD.with_label_values(&[&metric_types.method]).inc();
    REQUESTS_BY_UPSTREAM.with_label_values(&[metric_types.upstream.as_ref()]).inc();
    RESPONSE_LATENCY.observe(metric_types.latency.as_secs_f64());
}
