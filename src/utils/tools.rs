use crate::tls::load;
use crate::tls::load::CertificateConfig;
use crate::utils::types::{Extraparams, InnerMapForJson, UpstreamSnapshotForJson, UpstreamsDashMap, UpstreamsIdMap};
use log::{error, info};
use privdrop::PrivDrop;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt::{Debug, Write};
use std::fs::OpenOptions;
use std::io::Write as IoWrite;
use std::net::SocketAddr;
use std::net::TcpListener;
use std::ops::Deref;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::panic::Location;
use std::process::Command;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{env, fs, process, thread, time};

pub fn print_upstreams(upstreams: &UpstreamsDashMap, extraparams: &Extraparams) -> String {
    let mut out = String::new();
    for (host, paths) in upstreams.iter() {
        writeln!(out, "Hostname: {}", host).unwrap_or_default();
        for (path, (servers, _index)) in paths.iter() {
            writeln!(out, "    Path: {}", path).unwrap_or_default();
            for f in servers {
                writeln!(
                    out,
                    "        IP: {}, Port: {}, SSL: {}, H2: {}, To HTTPS: {}, Rate Limit: {}, 4xx Limit: {}",
                    f.address,
                    f.port,
                    f.is_ssl,
                    f.is_http2,
                    f.to_https,
                    if f.rate_limit > 0 { f.rate_limit } else { extraparams.rate_limit },
                    if f.x4xx_limit > 0 { f.x4xx_limit } else { extraparams.x4xx_limit }
                )
                .unwrap_or_default();
            }
        }
    }
    let out = out.trim_end();
    info!("\n{}", out);
    out.to_string()
}
pub fn clone_upstreams(original: &UpstreamsDashMap) -> UpstreamsDashMap {
    original
        .iter()
        .map(|(host, inner_map)| {
            let new_inner = inner_map.iter().map(|(path, (vec, _))| (path.clone(), (vec.clone(), AtomicUsize::new(0)))).collect();
            (host.clone(), new_inner)
        })
        .collect()
}

pub fn clone_dashmap_into(original: &UpstreamsDashMap, cloned: &mut UpstreamsDashMap) {
    cloned.clear();
    for (key, value) in original.iter() {
        let mut new_inner_map = HashMap::new();
        for (inner_key, inner_value) in value.iter() {
            let (vec, _) = inner_value;
            new_inner_map.insert(inner_key.clone(), (vec.clone(), AtomicUsize::new(0)));
        }
        cloned.insert(key.clone(), new_inner_map);
    }
}

pub fn compare_dashmaps(map1: &UpstreamsDashMap, map2: &UpstreamsDashMap) -> bool {
    if map1.len() != map2.len() {
        return false;
    }
    for (host, inner_map1) in map1 {
        let Some(inner_map2) = map2.get(host) else {
            return false;
        };

        if inner_map1.len() != inner_map2.len() {
            return false;
        }
        for (path, (vec1, _)) in inner_map1 {
            let Some((vec2, _)) = inner_map2.get(path) else {
                return false;
            };
            if vec1 != vec2 {
                return false;
            }
        }
    }
    true
}

pub fn merge_headers(target: &mut HashMap<Arc<str>, Vec<(String, Arc<str>)>>, source: &HashMap<Arc<str>, Vec<(String, Arc<str>)>>) {
    for (global_key, global_values) in source.iter() {
        let target_entry = target.entry(global_key.clone()).or_default();
        target_entry.extend(global_values.iter().cloned());
    }
}

pub fn clone_idmap_into(original: &UpstreamsDashMap, cloned: &mut UpstreamsIdMap) {
    cloned.clear();
    let mut id_buf = String::with_capacity(128);
    for (okey, inner_map) in original.iter() {
        for (_, (vec, _)) in inner_map.iter() {
            for x in vec {
                id_buf.clear();
                let _ = write!(
                    &mut id_buf,
                    "{}:{}:{}:{}:{}:{}:{}:{}:{:?}",
                    okey, x.address, x.port, x.is_http2, x.to_https, x.x4xx_limit, x.rate_limit, x.healthcheck, x.authorization
                );
                let mut hasher = Sha256::new();
                hasher.update(id_buf.as_bytes());
                let hash = hasher.finalize();
                let hex_hash = base16ct::lower::encode_string(&hash);
                let hh = hex_hash[..50].to_string();
                cloned.insert(hh, x.clone());
            }
        }
    }
    info!("Upstreams are fully populated. Ready to server requests");
}

pub fn listdir(dir: String) -> Vec<load::CertificateConfig> {
    let mut f = HashMap::new();
    let mut certificate_configs: Vec<load::CertificateConfig> = vec![];
    let paths = fs::read_dir(dir).die("listing certificates directory failed");
    for path in paths {
        let path_str = path.die("getting certs path").path().to_str().die("certs pat t &str").to_owned();
        if path_str.ends_with(".crt") {
            let name = path_str.replace(".crt", "");
            let mut inner = vec![];
            let domain = name.split("/").collect::<Vec<&str>>();
            inner.push(name.clone() + ".crt");
            inner.push(name.clone() + ".key");
            f.insert(domain[domain.len() - 1].to_owned(), inner);
            let y = CertificateConfig {
                cert_path: name.clone() + ".crt",
                key_path: name.clone() + ".key",
            };
            certificate_configs.push(y);
        }
    }
    // for (_, v) in f.iter() {
    //     let y = CertificateConfig {
    //         cert_path: v[0].clone(),
    //         key_path: v[1].clone(),
    //     };
    //     certificate_configs.push(y);
    // }
    certificate_configs
}

pub fn drop_priv(user: String, group: String, http_addr: String, tls_addr: Option<String>) {
    thread::sleep(time::Duration::from_millis(10));
    loop {
        thread::sleep(time::Duration::from_millis(10));
        if TcpListener::bind(&http_addr).is_err() {
            break;
        }
    }
    if let Some(tls_addr) = tls_addr {
        loop {
            thread::sleep(time::Duration::from_millis(10));
            if TcpListener::bind(&tls_addr).is_err() {
                break;
            }
        }
    }
    info!("Dropping ROOT privileges to: {}:{}", user, group);
    if let Err(e) = PrivDrop::default().user(user).group(group).apply() {
        error!("Failed to drop privileges: {}", e);
        process::exit(1)
    }
}

pub fn check_priv(addr: &str) {
    let port = SocketAddr::from_str(addr).map(|sa| sa.port()).die("Failed to parse address port ");
    if port < 1024 {
        let meta = std::fs::metadata("/proc/self").map(|m| m.uid()).die("Reading /proc/self");
        if meta != 0 {
            error!("Running on privileged port requires to start as ROOT");
            process::exit(1)
        }
    }
}

#[allow(dead_code)]
pub fn upstreams_to_json(upstreams: &UpstreamsDashMap) -> serde_json::Result<String> {
    let mut outer = HashMap::new();

    for (okey, ovalue) in upstreams.iter() {
        let mut inner_map = HashMap::new();

        for (ikey, ivalue) in ovalue.iter() {
            let (backends, counter) = ivalue;

            inner_map.insert(
                ikey,
                UpstreamSnapshotForJson {
                    backends: backends
                        .iter()
                        .map(|a| InnerMapForJson {
                            address: a.address.to_string(),
                            port: a.port,
                            is_ssl: a.is_ssl,
                            is_http2: a.is_http2,
                            to_https: a.to_https,
                            rate_limit: a.rate_limit,
                            x4xx_limit: a.x4xx_limit,
                            healthcheck: a.healthcheck,
                        })
                        .collect(),
                    requests: counter.load(Ordering::Relaxed),
                },
            );
        }
        outer.insert(okey.to_string(), inner_map);
    }
    serde_json::to_string(&outer)
}

pub fn upstreams_liveness_json(configured: &UpstreamsDashMap, current: &UpstreamsDashMap) -> Value {
    let mut result = serde_json::Map::new();

    for (hostname, configured_paths) in configured.iter() {
        let mut paths_json = serde_json::Map::new();

        for (key, value) in configured_paths.iter() {
            let (configured_backends, _) = value;

            // Filter to keep ONLY backends that exist in the current live set
            let backends_json: Vec<Value> = configured_backends
                .iter()
                .filter_map(|backend| {
                    let is_alive = current
                        .get(hostname.as_ref())
                        .and_then(|host_map| host_map.get(key.as_ref()))
                        .map_or(false, |(current_backends, _)| {
                            current_backends.iter().any(|b| b.address == backend.address && b.port == backend.port)
                        });

                    if is_alive {
                        Some(json!({
                            "address": &*backend.address,
                            "port": backend.port,
                            "alive": true
                        }))
                    } else {
                        None // Drop dead backends
                    }
                })
                .collect();

            // Only include the path if it has at least one live backend
            if !backends_json.is_empty() {
                paths_json.insert(
                    key.to_string(),
                    json!({
                        "backends": backends_json
                    }),
                );
            }
        }

        // Only include the hostname if it has at least one live path
        if !paths_json.is_empty() {
            result.insert(hostname.deref().to_string(), Value::Object(paths_json));
        }
    }

    Value::Object(result)
}

#[allow(dead_code)]
pub fn prepend(prefix: &str, val: &Option<Arc<str>>, uri: &str, port: &str) -> Option<String> {
    val.as_ref().map(|s| {
        let mut buf = String::with_capacity(32);
        buf.push_str(prefix);
        buf.push_str(s);
        buf.push(':');
        buf.push_str(port);
        buf.push_str(uri);
        buf
    })
}

pub fn write_pid_file(path: &str) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW) // refuse to follow symlinks
        .open(path)?;
    file.write_all(process::id().to_string().as_bytes())?;
    Ok(())
}

#[allow(clippy::needless_return)]
pub fn split_host_port(addr: &str, tls: bool) -> Option<(&str, u16, bool, &str)> {
    match addr.split_once(':') {
        Some((h, p)) => match p.parse::<u16>() {
            Ok(port) => return Some((h, port, tls, h)),
            Err(_) => {
                log::warn!("ForwardAuth: invalid port in {}", addr);
                return None;
            }
        },
        None => {
            if tls {
                return Some((addr, 443u16, tls, addr));
            } else {
                return Some((addr, 80u16, tls, addr));
            }
        }
    };
}

pub fn get_hostname() -> String {
    if let Ok(host) = env::var("HOSTNAME").or_else(|_| env::var("COMPUTERNAME")) {
        if !host.trim().is_empty() {
            return host;
        }
    }
    Command::new("hostname")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

// pub fn die(message: &str) -> ! {
//     log::error!("{message}");
//     process::exit(1);
// }

pub trait Die<T> {
    fn die(self, message: &str) -> T;
}

impl<T, E: Debug> Die<T> for Result<T, E> {
    #[track_caller]
    fn die(self, message: &str) -> T {
        match self {
            Ok(value) => value,
            Err(e) => {
                let loc = Location::caller();
                eprintln!("\x1b[1;31mERROR:\x1b[0m {message}: {e:?}: {}: {}", loc.file(), loc.line());
                process::exit(1);
            }
        }
    }
}

impl<T> Die<T> for Option<T> {
    #[track_caller]
    fn die(self, message: &str) -> T {
        match self {
            Some(value) => value,
            None => {
                let loc = Location::caller();
                eprintln!("\x1b[1;31mERROR:\x1b[0m {message} at {}:{}", loc.file(), loc.line());
                process::exit(1);
            }
        }
    }
}
