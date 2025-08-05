use crate::utils::structs::*;
use dashmap::DashMap;
use log::{error, info, warn};
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::{env, fs};

pub fn load_configuration(d: &str, kind: &str) -> Option<Configuration> {
    let yaml_data = match kind {
        "filepath" => match fs::read_to_string(d) {
            Ok(data) => {
                info!("Reading upstreams from {}", d);
                data
            }
            Err(e) => {
                error!("Reading: {}: {:?}", d, e);
                warn!("Running with empty upstreams list, update it via API");
                return None;
            }
        },
        "content" => {
            info!("Reading upstreams from API post body");
            d.to_string()
        }
        _ => {
            error!("Mismatched parameter, only filepath|content is allowed");
            return None;
        }
    };

    let parsed: Config = match serde_yaml::from_str(&yaml_data) {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("Failed to parse upstreams file: {}", e);
            return None;
        }
    };

    let mut toreturn = Configuration::default();

    populate_headers_and_auth(&mut toreturn, &parsed);
    toreturn.typecfg = parsed.provider.clone();

    match parsed.provider.as_str() {
        "file" => {
            populate_file_upstreams(&mut toreturn, &parsed);
            Some(toreturn)
        }
        "consul" => {
            toreturn.consul = parsed.consul;
            if toreturn.consul.is_some() {
                Some(toreturn)
            } else {
                None
            }
        }
        "kubernetes" => None,
        _ => {
            warn!("Unknown provider {}", parsed.provider);
            None
        }
    }
}

fn populate_headers_and_auth(config: &mut Configuration, parsed: &Config) {
    if let Some(headers) = &parsed.headers {
        let mut hl = Vec::new();
        for header in headers {
            if let Some((key, val)) = header.split_once(':') {
                hl.push((key.trim().to_string(), val.trim().to_string()));
            }
        }

        let global_headers = DashMap::new();
        global_headers.insert("/".to_string(), hl);
        config.headers.insert("GLOBAL_HEADERS".to_string(), global_headers);
    }

    config.extraparams.sticky_sessions = parsed.sticky_sessions;
    config.extraparams.to_https = parsed.to_https;
    config.extraparams.rate_limit = parsed.rate_limit;

    if let Some(rate) = &parsed.rate_limit {
        info!("Applied Global Rate Limit : {} request per second", rate);
    }

    if let Some(auth) = &parsed.authorization {
        let name = auth.get("type").unwrap_or(&"".to_string()).to_string();
        let creds = auth.get("creds").unwrap_or(&"".to_string()).to_string();
        config.extraparams.authentication.insert("authorization".to_string(), vec![name, creds]);
    } else {
        config.extraparams.authentication = DashMap::new();
    }
}

fn populate_file_upstreams(config: &mut Configuration, parsed: &Config) {
    if let Some(upstreams) = &parsed.upstreams {
        for (hostname, host_config) in upstreams {
            let path_map = DashMap::new();
            let header_list = DashMap::new();
            for (path, path_config) in &host_config.paths {
                if let Some(rate) = &path_config.rate_limit {
                    info!("Applied Rate Limit for {} : {} request per second", hostname, rate);
                }

                let mut server_list = Vec::new();
                let mut hl = Vec::new();

                if let Some(headers) = &path_config.headers {
                    for header in headers {
                        if let Some((key, val)) = header.split_once(':') {
                            hl.push((key.trim().to_string(), val.trim().to_string()));
                        }
                    }
                }
                header_list.insert(path.clone(), hl);

                for server in &path_config.servers {
                    if let Some((ip, port_str)) = server.split_once(':') {
                        if let Ok(port) = port_str.parse::<u16>() {
                            server_list.push(InnerMap {
                                address: ip.trim().to_string(),
                                port,
                                is_ssl: true,
                                is_http2: false,
                                to_https: path_config.to_https.unwrap_or(false),
                                // rate_limit: rate,
                                rate_limit: path_config.rate_limit,
                            });
                        }
                    }
                }
                path_map.insert(path.clone(), (server_list, AtomicUsize::new(0)));
            }
            config.headers.insert(hostname.clone(), header_list);
            config.upstreams.insert(hostname.clone(), path_map);
        }
    }
}

pub fn parce_main_config(path: &str) -> AppConfig {
    let data = fs::read_to_string(path).unwrap();
    let reply = DashMap::new();
    let cfg: HashMap<String, String> = serde_yaml::from_str(&*data).expect("Failed to parse main config file");
    let mut cfo: AppConfig = serde_yaml::from_str(&*data).expect("Failed to parse main config file");
    log_builder(&cfo);
    cfo.hc_method = cfo.hc_method.to_uppercase();
    for (k, v) in cfg {
        reply.insert(k.to_string(), v.to_string());
    }
    if let Some((ip, port_str)) = cfo.config_address.split_once(':') {
        if let Ok(port) = port_str.parse::<u16>() {
            cfo.local_server = Option::from((ip.to_string(), port));
        }
    }
    if let Some(tlsport_cfg) = cfo.proxy_address_tls.clone() {
        if let Some((_, port_str)) = tlsport_cfg.split_once(':') {
            if let Ok(port) = port_str.parse::<u16>() {
                cfo.proxy_port_tls = Some(port);
            }
        }
    };
    cfo.proxy_tls_grade = parce_tls_grades(cfo.proxy_tls_grade.clone());
    cfo
}

fn parce_tls_grades(what: Option<String>) -> Option<String> {
    match what {
        Some(g) => match g.to_ascii_lowercase().as_str() {
            "high" => {
                // info!("TLS grade set to: [ HIGH ]");
                Some("high".to_string())
            }
            "medium" => {
                // info!("TLS grade set to: [ MEDIUM ]");
                Some("medium".to_string())
            }
            "unsafe" => {
                // info!("TLS grade set to: [ UNSAFE ]");
                Some("unsafe".to_string())
            }
            _ => {
                warn!("Error parsing TLS grade, defaulting to: `medium`");
                Some("medium".to_string())
            }
        },
        None => {
            warn!("TLS grade not set, defaulting to: medium");
            Some("b".to_string())
        }
    }
}

fn log_builder(conf: &AppConfig) {
    let log_level = conf.log_level.clone();
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
}
