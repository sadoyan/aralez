use crate::utils::tools::*;
use crate::web::proxyhttp::LB;
use dashmap::DashMap;
use pingora_core::prelude::background_service;
use pingora_core::server::Server;
use std::sync::Arc;

pub fn run() {
    env_logger::init();

    let mut server = Server::new(None).unwrap();
    server.bootstrap();

    let upstreams_map: UpstreamMap = DashMap::new();
    let config = Arc::new(upstreams_map);

    let umap_full: UpstreamMap = DashMap::new();
    let fconfig = Arc::new(umap_full);

    let lb = LB {
        upstreams: config.clone(),
        umap_full: fconfig.clone(),
    };
    let bg = LB {
        upstreams: config.clone(),
        umap_full: fconfig.clone(),
    };

    let bg_srvc = background_service("bgsrvc", bg);
    let mut proxy = pingora_proxy::http_proxy_service(&server.configuration, lb);

    proxy.add_tcp("0.0.0.0:6193");
    server.add_service(proxy);
    server.add_service(bg_srvc);

    server.run_forever();
}
