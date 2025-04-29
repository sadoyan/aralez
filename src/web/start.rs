use crate::utils::tools::*;
use crate::web::proxyhttp::LB;
use dashmap::DashMap;
use log::info;
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
    let im: UpstreamsIdMap = DashMap::new();
    let hh: Headers = DashMap::new();

    let uf_config = Arc::new(uf);
    let ff_config = Arc::new(ff);
    let im_config = Arc::new(im);
    let hh_config = Arc::new(hh);

    let cfg = Arc::new(maincfg);
    let local = Arc::new(local_conf);

    let proxyconf: DashMap<String, Vec<String>> = Default::default();
    let pconf = Arc::new(proxyconf);

    let lb = LB {
        ump_upst: uf_config.clone(),
        ump_full: ff_config.clone(),
        ump_byid: im_config.clone(),
        config: cfg.clone(),
        local: local.clone(),
        headers: hh_config.clone(),
        proxyconf: pconf.clone(),
    };
    let bg = LB {
        ump_upst: uf_config.clone(),
        ump_full: ff_config.clone(),
        ump_byid: im_config.clone(),
        config: cfg.clone(),
        local: local.clone(),
        headers: hh_config.clone(),
        proxyconf: pconf.clone(),
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
    let bind_address_http = cfg.get("proxy_address_http").unwrap();

    let bind_address_tls = cfg.get("proxy_address_tls");
    match bind_address_tls {
        Some(bind_address_tls) => {
            info!("Running TLS listener on :{}", bind_address_tls.value());
            let cert_path = cfg.get("tls_certificate").unwrap();
            let key_path = cfg.get("tls_key_file").unwrap();
            let mut tls_settings = pingora_core::listeners::tls::TlsSettings::intermediate(&cert_path, &key_path).unwrap();
            tls_settings.enable_h2();
            proxy.add_tls_with_settings(bind_address_tls.value(), None, tls_settings);
        }
        None => {}
    }
    info!("Running HTTP listener on :{}", bind_address_http.as_str());
    proxy.add_tcp(bind_address_http.as_str());
    server.add_service(proxy);
    server.add_service(bg_srvc);
    // let mut prometheus_service_http = Service::prometheus_http_service();
    // prometheus_service_http.add_tcp("0.0.0.0:1234");
    // server.add_service(prometheus_service_http);
    server.run_forever();
}
