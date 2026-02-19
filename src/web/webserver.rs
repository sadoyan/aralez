use crate::utils::discovery::APIUpstreamProvider;
use crate::utils::structs::{Config, Configuration, UpstreamsDashMap};
use crate::utils::tools::{upstreams_liveness_json, upstreams_to_json};
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
// use axum_server::tls_openssl::OpenSSLConfig;
use futures::channel::mpsc::Sender;
use futures::SinkExt;
use jsonwebtoken::{encode, EncodingKey, Header};
use log::{error, info, warn};
use prometheus::{gather, Encoder, TextEncoder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
// use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

#[derive(Deserialize)]
struct InputKey {
    master_key: String,
    owner: String,
    valid: u64,
}

#[derive(Serialize, Debug)]
struct OutToken {
    token: String,
}

#[derive(Clone)]
struct AppState {
    master_key: String,
    config_sender: Sender<Configuration>,
    config_api_enabled: bool,
    current_upstreams: Arc<UpstreamsDashMap>,
    full_upstreams: Arc<UpstreamsDashMap>,
}

#[allow(unused_mut)]
pub async fn run_server(config: &APIUpstreamProvider, mut to_return: Sender<Configuration>, upstreams_curr: Arc<UpstreamsDashMap>, upstreams_full: Arc<UpstreamsDashMap>) {
    let app_state = AppState {
        master_key: config.masterkey.clone(),
        config_sender: to_return.clone(),
        config_api_enabled: config.config_api_enabled.clone(),
        current_upstreams: upstreams_curr,
        full_upstreams: upstreams_full,
    };
    let app = Router::new()
        // .route("/{*wildcard}", get(senderror))
        // .route("/{*wildcard}", post(senderror))
        // .route("/{*wildcard}", put(senderror))
        // .route("/{*wildcard}", head(senderror))
        // .route("/{*wildcard}", delete(senderror))
        // .nest_service("/static", static_files)
        .route("/jwt", post(jwt_gen))
        .route("/conf", post(conf))
        .route("/metrics", get(metrics))
        .route("/status", get(status))
        .with_state(app_state);

    // if let Some(value) = &config.tls_address {
    //     let cf = OpenSSLConfig::from_pem_file(config.tls_certificate.clone().unwrap(), config.tls_key_file.clone().unwrap()).unwrap();
    //     let addr: SocketAddr = value.parse().expect("Unable to parse socket address");
    //     let tls_app = app.clone();
    //     tokio::spawn(async move {
    //         if let Err(e) = axum_server::bind_openssl(addr, cf).serve(tls_app.into_make_service()).await {
    //             eprintln!("TLS server failed: {}", e);
    //         }
    //     });
    //     info!("Starting the TLS API server on: {}", value);
    // }

    if let (Some(address), Some(folder)) = (&config.file_server_address, &config.file_server_folder) {
        let static_files = ServeDir::new(folder);
        let static_serve: Router = Router::new().fallback_service(static_files);
        let static_listen = TcpListener::bind(address).await.unwrap();
        let _ = tokio::spawn(async move { axum::serve(static_listen, static_serve).await.unwrap() });
    }

    let listener = TcpListener::bind(config.address.clone()).await.unwrap();
    info!("Starting the API server on: {}", config.address);
    axum::serve(listener, app).await.unwrap();
}

async fn conf(State(st): State<AppState>, Query(params): Query<HashMap<String, String>>, content: String) -> impl IntoResponse {
    if !st.config_api_enabled {
        return Response::builder().status(StatusCode::FORBIDDEN).body(Body::from("Config API is disabled !\n")).unwrap();
    }
    if let Some(s) = params.get("key") {
        if s.to_owned() == st.master_key {
            let strcontent = content.as_str();
            let parsed = serde_yaml::from_str::<Config>(strcontent);
            match parsed {
                Ok(_) => {
                    if let Some(s) = params.get("key") {
                        if s.to_owned() == st.master_key {
                            let _ = tokio::spawn(async move { apply_config(content.as_str(), st).await });
                            return Response::builder().status(StatusCode::OK).body(Body::from("Accepted! Applying in background\n")).unwrap();
                        }
                    }
                    return Response::builder().status(StatusCode::FORBIDDEN).body(Body::from("Access Denied !\n")).unwrap();
                }
                Err(err) => {
                    error!("Failed to parse upstreams file: {}", err);
                    return Response::builder().status(StatusCode::BAD_GATEWAY).body(Body::from(format!("Failed: {}\n", err))).unwrap();
                }
            }
        }
    }
    Response::builder().status(StatusCode::FORBIDDEN).body(Body::from("Access Denied !\n")).unwrap()
}

async fn apply_config(content: &str, mut st: AppState) {
    let sl = crate::utils::parceyaml::load_configuration(content, "content").await;
    if let Some(serverlist) = sl.0 {
        let _ = st.config_sender.send(serverlist).await;
    }
}

async fn jwt_gen(State(state): State<AppState>, Json(payload): Json<InputKey>) -> (StatusCode, Json<OutToken>) {
    if payload.master_key == state.master_key {
        let now = SystemTime::now() + Duration::from_secs(payload.valid * 60);
        let a = now.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let claim = crate::utils::jwt::Claims { user: payload.owner, exp: a };
        match encode(&Header::default(), &claim, &EncodingKey::from_secret(payload.master_key.as_ref())) {
            Ok(t) => {
                let tok = OutToken { token: t };
                info!("Generating token: {:?}", tok);
                (StatusCode::CREATED, Json(tok))
            }
            Err(e) => {
                let tok = OutToken { token: "ERROR".to_string() };
                error!("Failed to generate token: {:?}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, Json(tok))
            }
        }
    } else {
        let tok = OutToken {
            token: "Unauthorised".to_string(),
        };
        warn!("Unauthorised JWT generate request: {:?}", tok);
        (StatusCode::FORBIDDEN, Json(tok))
    }
}

async fn metrics() -> impl IntoResponse {
    let metric_families = gather();
    let encoder = TextEncoder::new();
    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        // encoding error fallback
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(format!("Failed to encode metrics: {}", e)))
            .unwrap();
    }
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", encoder.format_type())
        .body(Body::from(buffer))
        .unwrap()
}

async fn status(State(st): State<AppState>, Query(params): Query<HashMap<String, String>>) -> impl IntoResponse {
    if let Some(_) = params.get("live") {
        let r = upstreams_liveness_json(&st.full_upstreams, &st.current_upstreams);
        return Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Body::from(format!("{}", r)))
            .unwrap();
    }
    if let Some(_) = params.get("all") {
        let resp = upstreams_to_json(&st.current_upstreams);
        match resp {
            Ok(j) => {
                return Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .body(Body::from(j))
                    .unwrap()
            }
            Err(e) => {
                return Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from(format!("Failed to get status: {}", e)))
                    .unwrap();
            }
        }
    }
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Body::from(format!("Parameter mismatch")))
        .unwrap()
}
