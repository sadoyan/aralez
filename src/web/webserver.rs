use axum::body::Body;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, head, post, put};
use axum::{Json, Router};
use dashmap::DashMap;
use futures::channel::mpsc::Sender;
use futures::SinkExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use tokio::net::TcpListener;

#[derive(Debug, Serialize, Deserialize)]
struct UpstreamData {
    servers: Vec<(String, u16)>,
    counter: usize,
}

pub async fn run_server(mut toreturn: Sender<DashMap<String, (Vec<(String, u16)>, AtomicUsize)>>) {
    let mut tr = toreturn.clone();
    let app = Router::new()
        .route("/{*wildcard}", get(getconfig))
        .route("/{*wildcard}", post(getconfig))
        .route("/{*wildcard}", put(getconfig))
        .route("/{*wildcard}", head(getconfig))
        .route("/{*wildcard}", delete(getconfig))
        .route(
            "/conf",
            post(|up: String| async move {
                let serverlist = crate::utils::discovery::build_upstreams(up.as_str(), "content");
                let _ = tr.send(serverlist).await.unwrap();
                Response::builder().status(StatusCode::CREATED).body(Body::from("Config, conf file, updated!\n")).unwrap()
            })
            .with_state("state"),
        )
        .route(
            "/json",
            post(|Json(payload): Json<HashMap<String, UpstreamData>>| async move {
                let upstreams = DashMap::new();
                for (key, value) in payload {
                    upstreams.insert(key, (value.servers, AtomicUsize::new(value.counter)));
                }
                let _ = toreturn.send(upstreams).await.unwrap();
                Response::builder().status(StatusCode::CREATED).body(Body::from("Config, json, updated!\n")).unwrap()
            }),
        );
    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Axum API server running on port 3000");
    axum::serve(listener, app).await.unwrap();
}

async fn getconfig() -> impl IntoResponse {
    "Hello from Axum API inside Pingora!\n".to_string();
    Response::builder().status(StatusCode::BAD_GATEWAY).body(Body::from("No live upstream found!\n")).unwrap()
}
// curl -XPOST -H 'Content-Type: application/json' --data-binary @./push.json 127.0.0.1:3000/json
// curl -XPOST --data-binary @./etc/upstreams.txt 127.0.0.1:3000/conf

/*
async fn config(Json(payload): Json<HashMap<String, UpstreamData>>) -> impl IntoResponse {
    let upstreams = DashMap::new();
    for (key, value) in payload {
        upstreams.insert(key, (value.servers, AtomicUsize::new(value.counter)));
    }
    println!("{:?}", upstreams);
    Response::builder().status(StatusCode::CREATED).body(Body::from("Config updated!\n")).unwrap()
}
async fn parse_upstreams(up: String) -> impl IntoResponse {
    println!("Parsing: {}", up);
    let serverlist = read_upstreams_from_file(up.as_str());
    println!("{:?}", serverlist);
    Response::builder().status(StatusCode::CREATED).body(Body::from("Config updated!\n")).unwrap()
}
*/
