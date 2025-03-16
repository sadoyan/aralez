use crate::utils::tools::*;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::sync::atomic::AtomicUsize;

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    upstreams: HashMap<String, HostConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct HostConfig {
    paths: HashMap<String, PathConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PathConfig {
    protocol: String,
    ssl: bool,
    servers: Vec<String>,
}

pub fn load_yaml_to_dashmap(d: &str, kind: &str) -> UpstreamsDashMap {
    let dashmap = UpstreamsDashMap::new();
    let mut yaml_data = d.to_string();
    match kind {
        "filepath" => {
            println!("Reading upstreams from {}", d);
            let _ = match fs::read_to_string(d) {
                Ok(data) => yaml_data = data,
                Err(e) => {
                    eprintln!("Error reading file: {:?}", e);
                    return dashmap;
                }
            };
        }
        "content" => {
            println!("Reading upstreams from API post body");
        }
        _ => println!("*******************> nothing <*******************"),
    }
    let parsed: Config = serde_yaml::from_str(&yaml_data).expect("Failed to parse YAML");
    for (hostname, host_config) in parsed.upstreams {
        let path_map = DashMap::new();
        for (path, path_config) in host_config.paths {
            let mut server_list = Vec::new();
            for server in path_config.servers {
                if let Some((ip, port_str)) = server.split_once(':') {
                    if let Ok(port) = port_str.parse::<u16>() {
                        server_list.push((ip.to_string(), port, path_config.ssl, path_config.protocol.clone()));
                    }
                }
            }
            path_map.insert(path, (server_list, AtomicUsize::new(0)));
        }
        dashmap.insert(hostname, path_map);
    }
    dashmap
}
