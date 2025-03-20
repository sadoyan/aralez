use crate::utils::tools::*;
use crate::web::proxyhttp::LB;
use dashmap::DashMap;
use pingora_core::prelude::{background_service, Opt};
use pingora_core::server::Server;
use std::env;
use std::sync::Arc;

pub fn run() {
    let parameters = Some(Opt::parse_args()).unwrap();
    let file = parameters.conf.clone().unwrap();
    let maincfg = crate::utils::parceyaml::parce_main_config(file.as_str());

    let mut local_conf: (String, u16) = ("0.0.0.0".to_string(), 0);
    if let Some((ip, port_str)) = maincfg.get("config_address").unwrap().split_once(':') {
        if let Ok(port) = port_str.parse::<u16>() {
            local_conf = (ip.to_string(), port);
        }
    }

    let mut server = Server::new(parameters).unwrap();
    server.bootstrap();

    let uf: UpstreamsDashMap = DashMap::new();
    let ff: UpstreamsDashMap = DashMap::new();
    let uf_config = Arc::new(uf);
    let ff_config = Arc::new(ff);
    let cfg = Arc::new(maincfg);
    let local = Arc::new(local_conf);

    let lb = LB {
        ump_upst: uf_config.clone(),
        ump_full: ff_config.clone(),
        config: cfg.clone(),
        local: local.clone(),
    };
    let bg = LB {
        ump_upst: uf_config.clone(),
        ump_full: ff_config.clone(),
        config: cfg.clone(),
        local: local.clone(),
    };

    // env_logger::Env::new();
    // env_logger::init();

    let log_level = cfg.get("log_level").unwrap();
    match log_level.as_str() {
        "info" => env::set_var("RUST_LOG", "info"),
        "error" => env::set_var("RUST_LOG", "error"),
        "warn" => env::set_var("RUST_LOG", "warn"),
        "debug" => env::set_var("RUST_LOG", "debug"),
        "trace" => env::set_var("RUST_LOG", "trace"),
        "off" => env::set_var("RUST_LOG", "off"),
        _ => {
            println!("Error reading log level, defaulting to: INFO");
            env::set_var("RUST_LOG", "info")
        }
    }
    env_logger::builder()
        // .format_timestamp(None)
        // .format_module_path(false)
        // .format_source_path(false)
        // .format_target(false)
        .init();

    let bg_srvc = background_service("bgsrvc", bg);
    let mut proxy = pingora_proxy::http_proxy_service(&server.configuration, lb);
    let bindaddress = cfg.get("proxy_address_http").unwrap();

    proxy.add_tcp(bindaddress.as_str());
    server.add_service(proxy);
    server.add_service(bg_srvc);

    // info!("Starting Gazan server on {}, port : {} !", args.address, args.port);

    server.run_forever();
}
