use crate::utils::tools::*;
use dashmap::DashMap;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use serde_yaml::Error;
use std::collections::HashMap;
use std::fs;
use std::sync::atomic::AtomicUsize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceMapping {
    pub proxy: String,
    pub real: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Consul {
    pub servers: Option<Vec<String>>,
    pub services: Option<Vec<ServiceMapping>>,
    pub token: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
struct Config {
    provider: String,
    upstreams: Option<HashMap<String, HostConfig>>,
    globals: Option<HashMap<String, Vec<String>>>,
    consul: Option<Consul>,
}

#[derive(Debug, Serialize, Deserialize)]
struct HostConfig {
    paths: HashMap<String, PathConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PathConfig {
    ssl: bool,
    servers: Vec<String>,
    headers: Option<Vec<String>>,
}

pub struct Configuration {
    pub upstreams: UpstreamsDashMap,
    pub headers: Headers,
    pub consul: Option<Consul>,
    pub typecfg: String,
}

// pub fn load_configuration(d: &str, kind: &str) -> Option<(UpstreamsDashMap, Headers, String)> {
pub fn load_configuration(d: &str, kind: &str) -> Option<Configuration> {
    let mut toreturn: Configuration = Configuration {
        upstreams: Default::default(),
        headers: Default::default(),
        consul: None,
        typecfg: "".to_string(),
    };
    toreturn.upstreams = UpstreamsDashMap::new();
    toreturn.headers = Headers::new();

    // let upstreamsmap = UpstreamsDashMap::new();
    // let headersmap = DashMap::new();

    let mut yaml_data = d.to_string();
    match kind {
        "filepath" => {
            let _ = match fs::read_to_string(d) {
                Ok(data) => {
                    info!("Reading upstreams from {}", d);
                    yaml_data = data
                }
                Err(e) => {
                    error!("Reading: {}: {:?}", d, e.to_string());
                    warn!("Running with empty upstreams list, update it via API");
                    return None;
                }
            };
        }
        "content" => {
            info!("Reading upstreams from API post body");
        }
        _ => error!("Mismatched parameter, only filepath|content is allowed "),
    }

    let p: Result<Config, Error> = serde_yaml::from_str(&yaml_data);
    match p {
        Ok(parsed) => {
            let global_headers = DashMap::new();
            let mut hl = Vec::new();
            if let Some(globals) = &parsed.globals {
                for headers in globals.get("headers").iter().by_ref() {
                    for header in headers.iter() {
                        if let Some((key, val)) = header.split_once(':') {
                            hl.push((key.to_string(), val.to_string()));
                        }
                    }
                }
                global_headers.insert("/".to_string(), hl);
                toreturn.headers.insert("GLOBAL_HEADERS".to_string(), global_headers);
            }
            match parsed.provider.as_str() {
                "file" => {
                    toreturn.typecfg = "file".to_string();
                    if let Some(upstream) = parsed.upstreams {
                        for (hostname, host_config) in upstream {
                            let path_map = DashMap::new();
                            let header_list = DashMap::new();
                            for (path, path_config) in host_config.paths {
                                let mut server_list = Vec::new();
                                let mut hl = Vec::new();
                                // Set global headers
                                // if let Some(globals) = &parsed.globals {
                                //     for headers in globals.get("headers").iter().by_ref() {
                                //         for header in headers.iter() {
                                //             if let Some((key, val)) = header.split_once(':') {
                                //                 hl.push((key.to_string(), val.to_string()));
                                //             }
                                //         }
                                //     }
                                // }
                                // Set per host/path headers
                                if let Some(headers) = &path_config.headers {
                                    for header in headers.iter().by_ref() {
                                        if let Some((key, val)) = header.split_once(':') {
                                            hl.push((key.to_string(), val.to_string()));
                                        }
                                    }
                                }
                                header_list.insert(path.clone(), hl);
                                for server in path_config.servers {
                                    if let Some((ip, port_str)) = server.split_once(':') {
                                        if let Ok(port) = port_str.parse::<u16>() {
                                            server_list.push((ip.to_string(), port, path_config.ssl));
                                        }
                                    }
                                }
                                path_map.insert(path, (server_list, AtomicUsize::new(0)));
                            }
                            toreturn.headers.insert(hostname.clone(), header_list);
                            toreturn.upstreams.insert(hostname, path_map);
                        }
                    }
                    Some(toreturn)
                }
                "consul" => {
                    toreturn.typecfg = "consul".to_string();
                    let consul = parsed.consul;
                    match consul {
                        Some(consul) => {
                            toreturn.consul = Some(consul);
                            Some(toreturn)
                        }
                        None => None,
                    }
                }
                "kubernetes" => None,
                _ => {
                    warn!("Unknown provider {}", parsed.provider);
                    None
                }
            }
        }
        Err(e) => {
            error!("Failed to parse upstreams file: {}", e);
            None
        }
    }
}

pub fn parce_main_config(path: &str) -> DashMap<String, String> {
    info!("Parsing configuration");
    let data = fs::read_to_string(path).unwrap();
    let reply = DashMap::new();
    let cfg: HashMap<String, String> = serde_yaml::from_str(&*data).expect("Failed to parse main config file");
    for (k, v) in cfg {
        reply.insert(k.to_string(), v.to_string());
    }
    reply
}
