use dashmap::DashMap;
use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use hyper::Server;
use web::handler;
use web::peers;

#[tokio::main]
async fn main() {
    let bind_addr = "0.0.0.0:6193";
    let addr: SocketAddr = bind_addr.parse().expect("Could not parse ip:port.");

    let main_peers: peers::Peers = Arc::new(DashMap::new());
    let work_peers: peers::Peers = Arc::new(DashMap::new());

    main_peers.insert(Arc::from("/first"), vec![]);
    let p = main_peers.clone();
    peers::add_peers(p, "/first");

    main_peers.insert(Arc::from("/second"), vec![]);
    let r = main_peers.clone();
    peers::add_peers(r, "/second");

    let main_prs = main_peers.clone();
    let work_prs = work_peers.clone();
    tokio::spawn(async move {
        let h = handler::healthcheck(main_prs.clone(), work_prs.clone());
        h.await.expect("health check failed");
        // handler::healthcheck(main_prs.clone(), work_prs.clone()).await;
    });

    let make_svc = make_service_fn(|conn: &AddrStream| {
        let remote_addr = conn.remote_addr().ip();
        let value = work_peers.clone();
        async move { Ok::<_, Infallible>(service_fn(move |req| handler::proxy_http(remote_addr, req, value.clone()))) }
    });

    let server = Server::bind(&addr).serve(make_svc);

    println!("Running server on {:?}", addr);

    if let Err(e) = server.await {
        println!("server error: {}", e);
    }
}
