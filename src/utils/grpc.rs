use bytes::Bytes;
use http_body_util::Full;
use hyper::Request;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use log::error;

pub async fn ping_grpc(addr: &str) -> bool {
    let connector = HttpConnector::new();
    let client = Client::builder(TokioExecutor::new()).http2_only(true).build(connector);
    let uri = format!("{}{}", addr, "/aralez.Probe/Ping");
    let body = Full::new(Bytes::from(vec![0, 0, 0, 0, 0]));

    match Request::post(uri.clone()).header("content-type", "application/grpc").header("te", "trailers").body(body) {
        Ok(request) => match client.request(request).await {
            Ok(resp) => resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.starts_with("application/grpc"))
                .unwrap_or(false),
            Err(_) => false,
        },
        Err(e) => {
            error!("Failed to create request: {}", e);
            false
        }
    }
}
