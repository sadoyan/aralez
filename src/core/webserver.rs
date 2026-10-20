use crate::auth::jwt::Claims;
use crate::ingress::APIUpstreamProvider;
use crate::tls::acme::http01::{acme_create, acme_order, http01_challenge};
use crate::utils::lazylock::NODE_STATUS;
use crate::utils::metrics::{MEMORY_USAGE, OPEN_FILES, get_memory_usage, get_open_files};
use crate::utils::tools::{Die, upstreams_liveness_json, upstreams_to_json};
use crate::utils::types::{Config, Configuration, UpstreamsDashMap};
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{any, get, post};
use axum::{Json, Router};
use jsonwebtoken::{EncodingKey, Header, encode};
use log::{debug, error, info, warn};
use prometheus::{Encoder, TextEncoder, gather};
use serde::Serialize;
use signal_hook::{consts::SIGQUIT, iterator::Signals};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tower_http::services::ServeDir;

#[derive(Serialize, Debug)]
struct OutToken {
    token: String,
}

#[derive(Clone)]
pub struct AppState {
    master_key: Option<String>,
    pub acme_dns_provider: Option<String>,
    pub cert_creds: String,
    pub certs_dir: String,
    upstreams_file: String,
    config_sender: mpsc::Sender<Configuration>,
    config_api_enabled: bool,
    current_upstreams: Arc<UpstreamsDashMap>,
    full_upstreams: Arc<UpstreamsDashMap>,
}

#[allow(unused_mut)]
pub async fn run_server(config: &APIUpstreamProvider, mut to_return: mpsc::Sender<Configuration>, upstreams_curr: Arc<UpstreamsDashMap>, upstreams_full: Arc<UpstreamsDashMap>) {
    let credsfile = config.config_dir.clone() + "/acme_credentials.json";
    let app_state = AppState {
        master_key: config.masterkey.clone(),
        cert_creds: credsfile,
        certs_dir: config.certs_dir.clone(),
        upstreams_file: config.upstreams_file.clone(),
        config_sender: to_return.clone(),
        config_api_enabled: config.config_api_enabled,
        current_upstreams: upstreams_curr,
        full_upstreams: upstreams_full,
        acme_dns_provider: config.acme_dns_provider.clone(),
    };
    let app = Router::new()
        // .route("/{*wildcard}", get(senderror))
        .route("/jwt", post(jwt_gen))
        .route("/acme_create", any(acme_create))
        .route("/acme_order/{*domain}", any(acme_order))
        .route("/.well-known/acme-challenge/{*token}", any(http01_challenge))
        .route("/conf", post(conf))
        .route("/workerconf", post(workerconf))
        .route("/metrics", get(metrics))
        .route("/status", get(status))
        .with_state(app_state);

    let mut static_handle: Option<tokio::task::JoinHandle<()>> = None;
    if let (Some(address), Some(folder)) = (&config.file_server_address, &config.file_server_folder) {
        let static_listen = port_is_available("File Server", address).await;
        let static_files = ServeDir::new(folder);
        let static_serve: Router = Router::new().fallback_service(static_files);
        info!("Starting file server on: {}", address);

        static_handle = Some(tokio::spawn(async move {
            match axum::serve(static_listen, static_serve).await {
                Ok(o) => info!("File server exited normally: {:?}", o),
                Err(e) => error!("File server exited: {}", e),
            }
        }))
    }

    let listener = port_is_available("Config API", &config.address).await;
    info!("Starting the API server on: {}", config.address);
    let api_server = tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .die("Can't start API server")
    });
    let (tx, mut rx) = mpsc::channel(1);
    std::thread::spawn(move || {
        let mut signals = Signals::new([SIGQUIT]).die("Can't initialize OS SIGNALS");
        if let Some(sig) = signals.forever().next() {
            tx.blocking_send(sig).die("Can;t send signal to main thread");
        }
    });
    rx.recv().await;
    api_server.abort();
    if let Some(handle) = static_handle {
        handle.abort();
    }
    info!("Exiting...");
}

async fn conf(State(st): State<AppState>, Query(params): Query<HashMap<String, String>>, content: String) -> impl IntoResponse {
    if !st.config_api_enabled {
        return (StatusCode::FORBIDDEN, "Config API is disabled !\n").into_response();
    }

    let strcontent = content.as_str();
    let parsed = noyalib::from_str::<Config>(strcontent);
    match parsed {
        Ok(_) => {
            if params.contains_key("save") {
                tokio::spawn(async move { apply_config(content.as_str(), st, true).await });
            } else {
                tokio::spawn(async move { apply_config(content.as_str(), st, false).await });
            }
            (StatusCode::OK, "Accepted! Applying in background\n").into_response()
        }
        Err(err) => {
            error!("Failed to parse upstreams file: {}", err);
            (StatusCode::BAD_GATEWAY, format!("Failed: {}\n", err)).into_response()
        }
    }
}

use axum::extract::ConnectInfo;
use axum::http::HeaderMap;
async fn workerconf(State(st): State<AppState>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, content: String) -> impl IntoResponse {
    let api_key = headers.get("API-KEY").and_then(|v| v.to_str().ok());
    if api_key != st.master_key.as_deref() {
        warn!("Unauthorised access to workerconf endpoint from {}", addr.ip());
        return (StatusCode::UNAUTHORIZED, "Unauthorized: Invalid API-KEY\n").into_response();
    }
    if let Some(member) = NODE_STATUS.get() {
        if let Some(mbr) = member.as_ref() {
            match mbr.as_ref() {
                "worker" => {
                    info!("Accepted new config from master server {}", addr.ip());
                    let strcontent = content.as_str();
                    let parsed = noyalib::from_str::<Config>(strcontent);
                    match parsed {
                        Ok(_) => {
                            tokio::spawn(async move { apply_config(content.as_str(), st, true).await });
                            return (StatusCode::OK, "Accepted! Applying in background\n").into_response();
                        }
                        Err(err) => {
                            error!("Failed to parse upstreams file: {}", err);
                            return (StatusCode::BAD_GATEWAY, format!("Failed: {}\n", err)).into_response();
                        }
                    }
                }
                _ => {
                    return (StatusCode::BAD_GATEWAY, "Node is not running on worker mode !\n").into_response();
                }
            }
        }
        return (StatusCode::BAD_GATEWAY, "Node is not running on worker mode !\n").into_response();
    }
    (StatusCode::BAD_GATEWAY, "Node is not running on worker mode !\n").into_response()
}

async fn apply_config(content: &str, st: AppState, save: bool) {
    let sl = crate::utils::parceyaml::load_configuration(content, "content").await;
    if let Some(serverlist) = sl.0 {
        if save {
            info!("Saving new upstreams to: {}", st.upstreams_file);
            if let Err(err) = std::fs::write(&st.upstreams_file, content) {
                error!("Error saving to: {} : {}", st.upstreams_file, err);
            }
        }
        let _ = st.config_sender.send(serverlist).await;
    }
}

async fn jwt_gen(State(state): State<AppState>, Json(payload): Json<Claims>) -> (StatusCode, Json<OutToken>) {
    if let Some(master_key) = &state.master_key {
        if &payload.master_key == master_key {
            let now = SystemTime::now() + Duration::from_secs(payload.exp * 60);
            let expire = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

            let claim = Claims {
                master_key: String::new(),
                owner: payload.owner,
                exp: expire,
                random: payload.random,
            };
            match encode(&Header::default(), &claim, &EncodingKey::from_secret(payload.master_key.as_ref())) {
                Ok(t) => {
                    let tok = OutToken { token: t };
                    debug!("Generating token: {:?}", tok.token);
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
    } else {
        let tok = OutToken {
            token: "ERROR Getting JWT_KEY environment variable".to_string(),
        };
        error!("ERROR Getting JWT_KEY environment variable");
        (StatusCode::INTERNAL_SERVER_ERROR, Json(tok))
    }
}

async fn metrics() -> impl IntoResponse {
    MEMORY_USAGE.set(get_memory_usage() as i64);
    OPEN_FILES.set(get_open_files() as i64);

    let metric_families = gather();
    let encoder = TextEncoder::new();
    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to encode metrics: {}", e)).into_response();
    }
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", encoder.format_type())
        .body(Body::from(buffer))
        .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error\n").into_response())
}

#[allow(clippy::needless_return)]
async fn status(State(st): State<AppState>, Query(params): Query<HashMap<String, String>>) -> impl IntoResponse {
    if params.contains_key("live") {
        let r = upstreams_liveness_json(&st.full_upstreams, &st.current_upstreams);
        return Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Body::from(format!("{}", r)))
            .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error\n").into_response());
    }
    if params.contains_key("all") {
        let resp = upstreams_to_json(&st.current_upstreams);
        match resp {
            Ok(j) => {
                return Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .body(Body::from(j))
                    .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error\n").into_response());
            }
            Err(e) => return (StatusCode::BAD_GATEWAY, format!("Internal Server Error {}\n", e)).into_response(),
        }
    }
    (StatusCode::INTERNAL_SERVER_ERROR, "Parameter mismatch\n").into_response()
}

pub async fn port_is_available(name: &str, address: &str) -> TcpListener {
    let addr = SocketAddr::from_str(address).unwrap_or_else(|e| panic!("{}: Invalid address format: {:?}", name, e));
    let t = Duration::from_secs(2);
    let p = addr.port();
    loop {
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                return listener;
            }
            Err(_) => {
                warn!("{} port is not available: {} will try again in {:?}", name, p, t);
                tokio::time::sleep(t).await;
            }
        }
    }
}

// -- ⚝ by Dave -- in NeoVim ⚝ --
