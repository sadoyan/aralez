use crate::web::proxyhttp::{BGService, LB};
use dashmap::DashMap;
use pingora_core::prelude::background_service;
use pingora_core::server::Server;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use tokio::sync::RwLock;

/*
pub fn run1() {
    let mut upstreams = LoadBalancer::try_from_iter(vec!["192.168.1.10:8000", "192.168.1.1:8000", "127.0.0.1:8000"]).unwrap();
    env_logger::init();
    let hc = TcpHealthCheck::new();
    upstreams.set_health_check(hc);
    upstreams.health_check_frequency = Some(Duration::from_secs(1));

    let background = background_service("health check", upstreams);
    let upstreams = background.task();
    let mut server = Server::new(None).unwrap();
    server.bootstrap();

    let mut proxy = pingora_proxy::http_proxy_service(&server.configuration, proxyhttp::LB(upstreams));

    proxy.add_tcp("0.0.0.0:6193");
    server.add_service(background);
    server.add_service(proxy);
    server.run_forever();
}
*/

pub fn run() {
    env_logger::init();

    let mut server = Server::new(None).unwrap();
    server.bootstrap();

    let upstreams_map: DashMap<String, (Vec<(String, u16)>, AtomicUsize)> = DashMap::new();
    let config = Arc::new(RwLock::new(upstreams_map)); // Wrap in Arc<RwLock<...>>

    let lb = LB { upstreams_map: config.clone() }; // Share the Arc<RwLock<...>>
    let bg_service = BGService { upstreams_map: config.clone() }; // Share the Arc<RwLock<...>>

    let bg_srvc = background_service("bgsrvc", bg_service);
    bg_srvc.task();

    let mut proxy = pingora_proxy::http_proxy_service(&server.configuration, lb);
    proxy.add_tcp("0.0.0.0:6193");
    server.add_service(proxy);
    server.add_service(bg_srvc);

    server.run_forever();
}
