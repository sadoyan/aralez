use crate::utils::parceyaml::Configuration;
use axum::body::Body;
use axum::extract::State;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, head, post, put};
use axum::{Json, Router};
use futures::channel::mpsc::Sender;
use futures::SinkExt;
use jsonwebtoken::{encode, EncodingKey, Header};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;

#[derive(Deserialize)]
struct InputKey {
    masterkey: String,
    owner: String,
    valid: u64,
}

#[derive(Serialize, Debug)]
struct OutToken {
    token: String,
}

#[allow(unused_mut)]
pub async fn run_server(bindaddress: String, masterkey: String, mut toreturn: Sender<Configuration>) {
    let mut tr = toreturn.clone();
    let app = Router::new()
        .route("/{*wildcard}", get(senderror))
        .route("/{*wildcard}", post(senderror))
        .route("/{*wildcard}", put(senderror))
        .route("/{*wildcard}", head(senderror))
        .route("/{*wildcard}", delete(senderror))
        .route("/jwt", post(jwt_gen))
        .with_state(masterkey.clone())
        .route(
            "/conf",
            post(|up: String| async move {
                let serverlist = crate::utils::parceyaml::load_configuration(up.as_str(), "content");

                match serverlist {
                    Some(serverlist) => {
                        let _ = tr.send(serverlist).await.unwrap();
                        Response::builder().status(StatusCode::CREATED).body(Body::from("Config, conf file, updated!\n")).unwrap()
                    }
                    None => Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("Failed to parce config file!\n"))
                        .unwrap(),
                }
            })
            .with_state("state"),
        );
    let listener = TcpListener::bind(bindaddress.clone()).await.unwrap();
    info!("Starting the API server on: {}", bindaddress);
    axum::serve(listener, app).await.unwrap();
}

#[allow(dead_code)]
async fn senderror() -> impl IntoResponse {
    Response::builder().status(StatusCode::BAD_GATEWAY).body(Body::from("No live upstream found!\n")).unwrap()
}

async fn jwt_gen(State(masterkey): State<String>, Json(payload): Json<InputKey>) -> (StatusCode, Json<OutToken>) {
    if payload.masterkey == masterkey {
        let now = SystemTime::now() + Duration::from_secs(payload.valid * 60);
        let a = now.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let claim = crate::utils::jwt::Claims { user: payload.owner, exp: a };
        match encode(&Header::default(), &claim, &EncodingKey::from_secret(payload.masterkey.as_ref())) {
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
