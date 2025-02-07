use crate::peers::Peers;
use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;
use std::net::IpAddr;
use std::thread::sleep;
use std::time::Duration;

pub async fn proxy_http(client_ip: IpAddr, req: Request<Body>, db: Peers) -> Result<Response<Body>, Infallible> {
    let p = db.clone();
    let rurl = req.uri().path();
    let yoyo = req.uri().path().to_string(); // Bad thing, only for debug
    let peer = crate::peers::return_peer(p, rurl);
    match hyper_reverse_proxy::call(client_ip, peer.as_ref(), req).await {
        Ok(response) => {
            println!("Peer: {}, Client: {}, Path: {}, Status: {}", peer.as_ref(), client_ip, yoyo, response.status());
            Ok(response)
        }
        Err(_error) => {
            println!("Error: no live peers for: {}", yoyo);
            Ok(Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).body(Body::empty()).unwrap())
        }
    }
}

pub async fn healthcheck(peers: Peers, work: Peers) -> Result<Response<Body>, Infallible> {
    loop {
        println!("Main Peers -> {:?}", peers);
        println!("Work Peers -> {:?}", work);
        sleep(Duration::from_secs(10));

        peers.clone().iter().for_each(|peer| {
            work.insert(peer.key().clone(), peer.value().clone());
            // work[peer.key()] = peer.value();
        });
    }
}

// #[tokio::main]
// async fn client_check(url: &str) {
//     let url = url.parse::<hyper::Uri>().unwrap();
//     let host = url.host().expect("uri has no host");
//     let port = url.port_u16().unwrap_or(80);
//     let address = format!("{}:{}", host, port);
//     let stream = TcpStream::connect(address).await.unwrap();
//     let io = TokioIo::new(stream);
//     let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();
//
//     // Spawn a task to poll the connection, driving the HTTP state
//     tokio::task::spawn(async move {
//         if let Err(err) = conn.await {
//             println!("Connection failed: {:?}", err);
//         }
//     });
// }
