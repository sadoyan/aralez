// use rustls::crypto::ring::default_provider;
use crate::utils::structs::Extraparams;
use crate::utils::tls;
use crate::utils::tls::CertificateConfig;
use crate::utils::tools::*;
use crate::web::proxyhttp::LB;
use arc_swap::ArcSwap;
use dashmap::DashMap;
use log::info;
use pingora::tls::ssl::{SslAlert, SslRef};
use pingora_core::listeners::tls::TlsSettings;
use pingora_core::prelude::{background_service, Opt};
use pingora_core::server::Server;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::{env, thread};

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
        rate_limit: None,
    }));

    let cfg = Arc::new(maincfg);

    let lb = LB {
        ump_upst: uf_config,
        ump_full: ff_config,
        ump_byid: im_config,
        config: cfg.clone(),
        headers: hh_config,
        extraparams: ec_config,
    };

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
    env_logger::builder().init();

    let bg_srvc = background_service("bgsrvc", lb.clone());
    let mut proxy = pingora_proxy::http_proxy_service(&server.configuration, lb.clone());
    let bind_address_http = cfg.proxy_address_http.clone();

    let bind_address_tls = cfg.proxy_address_tls.clone();
    match bind_address_tls {
        Some(bind_address_tls) => {
            let (tx, rx): (Sender<Vec<CertificateConfig>>, Receiver<Vec<CertificateConfig>>) = channel();
            let certs_path = cfg.proxy_certificates.clone().unwrap();
            thread::spawn(move || {
                watch_folder(certs_path, tx).unwrap();
            });
            let certificate_configs = rx.recv().unwrap();
            let first_set = tls::Certificates::new(&certificate_configs).unwrap_or_else(|| panic!("Unable to load initial certificate info"));
            let certificates = Arc::new(ArcSwap::from_pointee(first_set));
            let certs_for_callback = certificates.clone();

            let certs_for_watcher = certificates.clone();
            let new_certs = tls::Certificates::new(&certificate_configs);
            certs_for_watcher.store(Arc::new(new_certs.unwrap()));

            let mut tls_settings =
                TlsSettings::intermediate(&certs_for_callback.load().default_cert_path, &certs_for_callback.load().default_key_path).expect("unable to load or parse cert/key");

            tls_settings.set_servername_callback(move |ssl_ref: &mut SslRef, ssl_alert: &mut SslAlert| certs_for_callback.load().server_name_callback(ssl_ref, ssl_alert));
            tls_settings.set_alpn_select_callback(tls::prefer_h2);
            proxy.add_tls_with_settings(&bind_address_tls, None, tls_settings);

            let certs_for_watcher = certificates.clone();
            thread::spawn(move || {
                while let Ok(new_configs) = rx.recv() {
                    let new_certs = tls::Certificates::new(&new_configs);
                    match new_certs {
                        Some(new_certs) => {
                            certs_for_watcher.store(Arc::new(new_certs));
                            info!("Reload TLS certificates from {}", cfg.proxy_certificates.clone().unwrap())
                        }
                        None => {}
                    };
                }
            });
        }
        None => {}
    }
    info!("Running HTTP listener on :{}", bind_address_http.as_str());
    proxy.add_tcp(bind_address_http.as_str());
    server.add_service(proxy);
    server.add_service(bg_srvc);
    server.run_forever();
}
