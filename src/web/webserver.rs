use crate::utils::tools::*;
use axum::body::Body;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, head, post, put};
use axum::Router;
use futures::channel::mpsc::Sender;
use futures::SinkExt;
use log::info;
use tokio::net::TcpListener;

#[allow(unused_mut)]
pub async fn run_server(bindaddress: String, mut toreturn: Sender<(UpstreamsDashMap, Headers)>) {
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
                let serverlist = crate::utils::parceyaml::load_configuration(up.as_str(), "content");

                match serverlist {
                    Some(serverlist) => {
                        let _ = tr.send((serverlist.upstreams, serverlist.headers)).await.unwrap();
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
