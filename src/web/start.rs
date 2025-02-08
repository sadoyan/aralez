use crate::web::proxyhttp::{GetHost, LB};
use dashmap::DashMap;
use pingora_core::prelude::background_service;
use pingora_core::server::Server;
use std::sync::atomic::AtomicUsize;
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

    // let backends = Backends::new(Box::new(SD));
    // let load_balancer = LoadBalancer::from_backends(backends);

    // load_balancer.set_health_check(TcpHealthCheck::new());
    // load_balancer.health_check_frequency = Some(Duration::from_secs(1));
    // load_balancer.update_frequency = Some(Duration::from_secs(1));

    // let background = background_service("load balancer", load_balancer);

    let upstreams_map: DashMap<String, (Vec<(String, u16)>, AtomicUsize)> = DashMap::new();

    let mut ll = LB {
        upstreams_map,
        // upstreams_maps: DashMap::new(),
    };

    let background = background_service("load balancer", ll.discover_hosts());
    background.task();

    let mut lb = pingora_proxy::http_proxy_service(&server.configuration, ll);

    lb.add_tcp("0.0.0.0:6193");
    server.add_service(lb);
    // server.add_service(background.task());

    server.run_forever();
}
