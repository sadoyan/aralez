// use rustls::crypto::ring::default_provider;
use crate::tls::grades;
use crate::tls::load;
use crate::tls::load::CertificateConfig;
use crate::utils::structs::Extraparams;
use crate::utils::tools::*;
use crate::web::proxyhttp::LB;
use arc_swap::ArcSwap;
use dashmap::DashMap;
use log::info;
use pingora::tls::ssl::{SslAlert, SslRef};
use pingora_core::listeners::tls::TlsSettings;
use pingora_core::listeners::TcpSocketOptions;
use pingora_core::prelude::{background_service, Opt};
use pingora_core::protocols::TcpKeepalive;
use pingora_core::server::Server;
use privdrop::reexports::libc::SIGQUIT;
use sd_notify::NotifyState;
use signal_hook::{
    consts::{SIGINT, SIGTERM},
    iterator::Signals,
};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;
use std::{fs, thread};

pub fn run() {
    // default_provider().install_default().expect("Failed to install rustls crypto provider");
    let parameters = Opt::parse_args();
    let file = parameters.conf.clone().unwrap();
    let maincfg = crate::utils::parceyaml::parce_main_config(file.as_str());

    let mut server = Server::new(parameters).unwrap();
    server.bootstrap();

    let uf_config = Arc::new(DashMap::new());
    let ff_config = Arc::new(DashMap::new());
    let im_config = Arc::new(DashMap::new());
    let ch_config = Arc::new(DashMap::new());
    let sh_config = Arc::new(DashMap::new());

    let ec_config = Arc::new(ArcSwap::from_pointee(Extraparams {
        to_https: None,
        sticky_sessions: None,
        authentication: None,
        rate_limit: None,
        x4xx_limit: None,
    }));

    let cfg = Arc::new(maincfg);

    let lb = LB {
        ump_upst: uf_config,
        ump_full: ff_config,
        ump_byid: im_config,
        config: cfg.clone(),
        client_headers: ch_config,
        server_headers: sh_config,
        extraparams: ec_config,
    };

    let grade = cfg.proxy_tls_grade.clone().unwrap_or("medium".to_string());
    info!("TLS grade set to: [ {} ]", grade);

    let bg_srvc = background_service("bgsrvc", lb.clone());
    let bind_address_http = cfg.proxy_address_http.clone();
    let bind_address_tls = cfg.proxy_address_tls.clone();

    let mut proxy = pingora_proxy::http_proxy_service(&server.configuration, lb.clone());

    check_priv(bind_address_http.as_str());

    // let mut tcp_options: Option<TcpSocketOptions> = Some(TcpSocketOptions::default());
    // let mut tcp_options = TcpSocketOptions::default();

    let mut tcp_options: Option<TcpSocketOptions> = None;
    if let Some(idle) = cfg.tcp_keepalive_idle {
        let mut to = TcpSocketOptions::default();
        to.tcp_keepalive = Some(TcpKeepalive {
            idle: Duration::from_secs(idle),
            interval: Duration::from_secs(cfg.tcp_keepalive_interval.unwrap_or(10)),
            user_timeout: Default::default(),
            count: cfg.tcp_keepalive_count.unwrap_or(5usize),
        });
        tcp_options = Some(to);
        info!(
            "Applying kernel tcp_keepalive parameters: idle {}, interval {}, count {}",
            idle,
            cfg.tcp_keepalive_interval.unwrap_or(60),
            cfg.tcp_keepalive_count.unwrap_or(5),
        );
    }

    if let Some(bind_address_tls) = bind_address_tls {
        check_priv(bind_address_tls.as_str());
        let (tx, rx): (Sender<Vec<CertificateConfig>>, Receiver<Vec<CertificateConfig>>) = channel();
        let certs_path = cfg.proxy_configs.clone().unwrap() + "/certificates";

        if fs::metadata(certs_path.clone()).is_err() {
            fs::create_dir_all(certs_path.clone()).unwrap();
        }
        thread::spawn(move || {
            watch_folder(certs_path, tx).unwrap();
        });
        let certificate_configs = rx.recv().unwrap();
        let first_set = load::Certificates::new(&certificate_configs, grade.as_str()).unwrap_or_else(|| panic!("Unable to load initial certificate info"));
        let certificates = Arc::new(ArcSwap::from_pointee(first_set));
        let certs_for_callback = certificates.clone();

        let certs_for_watcher = certificates.clone();
        let new_certs = load::Certificates::new(&certificate_configs, grade.as_str());
        certs_for_watcher.store(Arc::new(new_certs.unwrap()));

        let mut tls_settings =
            TlsSettings::intermediate(&certs_for_callback.load().default_cert_path, &certs_for_callback.load().default_key_path).expect("unable to load or parse cert/key");

        grades::set_tsl_grade(&mut tls_settings, grade.as_str());
        tls_settings.set_servername_callback(move |ssl_ref: &mut SslRef, ssl_alert: &mut SslAlert| certs_for_callback.load().server_name_callback(ssl_ref, ssl_alert));
        tls_settings.set_alpn_select_callback(grades::prefer_h2);

        proxy.add_tls_with_settings(&bind_address_tls, tcp_options.clone(), tls_settings);

        let certs_for_watcher = certificates.clone();
        thread::spawn(move || {
            while let Ok(new_configs) = rx.recv() {
                let new_certs = load::Certificates::new(&new_configs, grade.as_str());
                if let Some(new_certs) = new_certs {
                    certs_for_watcher.store(Arc::new(new_certs));
                };
            }
        });
    }
    info!("Running HTTP listener on :{}", bind_address_http);
    if let Some(tc) = tcp_options {
        proxy.add_tcp_with_settings(&bind_address_http, tc);
    } else {
        proxy.add_tcp(&bind_address_http)
    }

    server.add_service(proxy);
    server.add_service(bg_srvc);
    thread::spawn(move || server.run_forever());

    if let (Some(user), Some(group)) = (cfg.rungroup.clone(), cfg.runuser.clone()) {
        drop_priv(user, group, cfg.proxy_address_http.clone(), cfg.proxy_address_tls.clone());
    }
    let _ = sd_notify::notify(&[NotifyState::Ready]);

    let pf = cfg.pid_file.clone().unwrap_or("/tmp/aralez.pid".to_string());
    if let Err(e) = write_pid_file(pf.as_str()) {
        panic!("Failed to write PID file: {} : {}", pf, e);
    }
    let mut signals = Signals::new(&[SIGINT, SIGTERM, SIGQUIT]).unwrap();
    for sig in signals.forever() {
        match sig {
            SIGINT => info!("SIGINT received! Exiting..."),
            SIGTERM => info!("SIGTERM received! Exiting..."),
            SIGQUIT => {
                thread::sleep(Duration::from_secs(300));
                info!("SIGQUIT received! Exiting...")
            }
            _ => unreachable!(),
        }
        break;
    }
}
