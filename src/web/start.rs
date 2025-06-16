// use rustls::crypto::ring::default_provider;
use crate::utils::structs::Extraparams;
use crate::utils::tls;
use crate::utils::tools::listdir;
use crate::web::proxyhttp::LB;
use arc_swap::ArcSwap;
use dashmap::DashMap;
use log::info;
use openssl::ssl::{SslAlert, SslRef};
use pingora_core::listeners::tls::TlsSettings;
use pingora_core::prelude::{background_service, Opt};
use pingora_core::server::Server;
use std::env;
use std::sync::Arc;

pub fn run() {
    // default_provider().install_default().expect("Failed to install rustls crypto provider");
    let parameters = Some(Opt::parse_args()).unwrap();
    let file = parameters.conf.clone().unwrap();
    let maincfg = crate::utils::parceyaml::parce_main_config(file.as_str());

    let mut server = Server::new(parameters).unwrap();
    server.bootstrap();

    let uf_config = Arc::new(DashMap::new());
    let ff_config = Arc::new(DashMap::new());
    let im_config = Arc::new(DashMap::new());
    let hh_config = Arc::new(DashMap::new());
    let ec_config = Arc::new(ArcSwap::from_pointee(Extraparams {
        sticky_sessions: false,
        to_https: None,
        authentication: DashMap::new(),
    }));

    let cfg = Arc::new(maincfg);

    let lb = LB {
        ump_upst: uf_config.clone(),
        ump_full: ff_config.clone(),
        ump_byid: im_config.clone(),
        config: cfg.clone(),
        headers: hh_config.clone(),
        extraparams: ec_config.clone(),
    };
    let bg = LB {
        ump_upst: uf_config.clone(),
        ump_full: ff_config.clone(),
        ump_byid: im_config.clone(),
        config: cfg.clone(),
        headers: hh_config.clone(),
        extraparams: ec_config.clone(),
    };

    // env_logger::Env::new();
    // env_logger::init();

    let log_level = cfg.log_level.clone();
    unsafe {
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
    }
    env_logger::builder()
        // .format_timestamp(None)
        // .format_module_path(false)
        // .format_source_path(false)
        // .format_target(false)
        .init();

    let bg_srvc = background_service("bgsrvc", bg);
    let mut proxy = pingora_proxy::http_proxy_service(&server.configuration, lb);
    let bind_address_http = cfg.proxy_address_http.clone();

    let bind_address_tls = cfg.proxy_address_tls.clone();
    // let foo = crate::utils::tls::build_ssl_context_builder();
    match bind_address_tls {
        Some(bind_address_tls) => {
            info!("Running TLS listener on :{}", bind_address_tls);
            // let cert_path = cfg.tls_certificate.clone().unwrap();
            // let key_path = cfg.tls_key_file.clone().unwrap();
            // let mut tls_settings = tls::TlsSettings::intermediate(&cert_path, &key_path).unwrap();
            // tls_settings.enable_h2();
            // proxy.add_tls_with_settings(&bind_address_tls, None, tls_settings);

            let certificate_configs = listdir(cfg.proxy_certificates.clone().unwrap());
            let certificates = tls::Certificates::new(&certificate_configs);
            let mut tls_settings = TlsSettings::intermediate(&certificates.default_cert_path, &certificates.default_key_path).expect("unable to load or parse cert/key");
            tls_settings.enable_h2();
            tls_settings.set_servername_callback(move |ssl_ref: &mut SslRef, ssl_alert: &mut SslAlert| certificates.server_name_callback(ssl_ref, ssl_alert));
            tls_settings.set_alpn_select_callback(tls::prefer_h2);
            proxy.add_tls_with_settings(&bind_address_tls, None, tls_settings);
        }
        None => {}
    }
    info!("Running HTTP listener on :{}", bind_address_http.as_str());
    proxy.add_tcp(bind_address_http.as_str());
    server.add_service(proxy);
    server.add_service(bg_srvc);
    server.run_forever();
}
