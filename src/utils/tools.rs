use crate::utils::structs::{InnerMap, UpstreamsDashMap, UpstreamsIdMap};
use crate::utils::tls;
use crate::utils::tls::CertificateConfig;
use dashmap::DashMap;
use log::{error, info};
use notify::{event::ModifyKind, Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use port_check::is_port_reachable;
use privdrop::PrivDrop;
use sha2::{Digest, Sha256};
use std::any::type_name;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::net::SocketAddr;
use std::os::unix::fs::MetadataExt;
use std::str::FromStr;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{fs, process, thread, time};

#[allow(dead_code)]
pub fn print_upstreams(upstreams: &UpstreamsDashMap) {
    for host_entry in upstreams.iter() {
        let hostname = host_entry.key();
        println!("Hostname: {}", hostname);

        for path_entry in host_entry.value().iter() {
            let path = path_entry.key();
            println!("    Path: {}", path);
            for f in path_entry.value().0.clone() {
                println!(
                    "        IP: {}, Port: {}, SSL: {}, H2: {}, To HTTPS: {}, Rate Limit: {}",
                    f.address,
                    f.port,
                    f.is_ssl,
                    f.is_http2,
                    f.to_https,
                    f.rate_limit.unwrap_or(0)
                );
            }
        }
    }
}

#[allow(dead_code)]
pub fn typeoff<T>(_: T) {
    let to = type_name::<T>();
    println!("{:?}", to);
}

#[allow(dead_code)]
pub fn string_to_bool(val: Option<&str>) -> Option<bool> {
    match val {
        Some(v) => match v {
            "yes" => Some(true),
            "true" => Some(true),
            _ => Some(false),
        },
        None => Some(false),
    }
}

pub fn clone_dashmap(original: &UpstreamsDashMap) -> UpstreamsDashMap {
    let new_map: UpstreamsDashMap = DashMap::new();

    for outer_entry in original.iter() {
        let hostname = outer_entry.key();
        let inner_map = outer_entry.value();

        let new_inner_map = DashMap::new();

        for inner_entry in inner_map.iter() {
            let path = inner_entry.key();
            let (vec, _) = inner_entry.value();
            let new_vec = vec.clone();
            let new_counter = AtomicUsize::new(0);
            new_inner_map.insert(path.clone(), (new_vec, new_counter));
        }
        new_map.insert(hostname.clone(), new_inner_map);
    }
    new_map
}

pub fn clone_dashmap_into(original: &UpstreamsDashMap, cloned: &UpstreamsDashMap) {
    cloned.clear();
    for outer_entry in original.iter() {
        let hostname = outer_entry.key();
        let inner_map = outer_entry.value();
        let new_inner_map = DashMap::new();
        for inner_entry in inner_map.iter() {
            let path = inner_entry.key();
            let (vec, _) = inner_entry.value();
            let new_vec = vec.clone();
            let new_counter = AtomicUsize::new(0);
            new_inner_map.insert(path.clone(), (new_vec, new_counter));
        }
        cloned.insert(hostname.clone(), new_inner_map);
    }
}

pub fn compare_dashmaps(map1: &UpstreamsDashMap, map2: &UpstreamsDashMap) -> bool {
    let keys1: HashSet<_> = map1.iter().map(|entry| entry.key().clone()).collect();
    let keys2: HashSet<_> = map2.iter().map(|entry| entry.key().clone()).collect();
    if keys1 != keys2 {
        return false;
    }
    for entry1 in map1.iter() {
        let hostname = entry1.key();
        let inner_map1 = entry1.value();
        let Some(inner_map2) = map2.get(hostname) else {
            return false;
        };
        let inner_keys1: HashSet<_> = inner_map1.iter().map(|e| e.key().clone()).collect();
        let inner_keys2: HashSet<_> = inner_map2.iter().map(|e| e.key().clone()).collect();
        if inner_keys1 != inner_keys2 {
            return false;
        }
        for path_entry in inner_map1.iter() {
            let path = path_entry.key();
            let (vec1, _counter1) = path_entry.value();
            let Some(entry2) = inner_map2.get(path) else {
                return false; // Path exists in map1 but not in map2
            };
            let (vec2, _counter2) = entry2.value();
            let set1: HashSet<_> = vec1.iter().collect();
            let set2: HashSet<_> = vec2.iter().collect();
            if set1 != set2 {
                return false;
            }
        }
    }
    true
}

pub fn merge_headers(target: &DashMap<Arc<str>, Vec<(Arc<str>, Arc<str>)>>, source: &DashMap<Arc<str>, Vec<(Arc<str>, Arc<str>)>>) {
    for entry in source.iter() {
        let global_key = entry.key().clone();
        let global_values = entry.value().clone();
        let mut target_entry = target.entry(global_key).or_insert_with(Vec::new);
        target_entry.extend(global_values);
    }
}

pub fn clone_idmap_into(original: &UpstreamsDashMap, cloned: &UpstreamsIdMap) {
    cloned.clear();
    for outer_entry in original.iter() {
        let inner_map = outer_entry.value();
        let new_inner_map = DashMap::new();
        for inner_entry in inner_map.iter() {
            let path = inner_entry.key();
            let (vec, _) = inner_entry.value();
            let new_vec = vec.clone();
            for x in vec.iter() {
                let mut id = String::new();
                write!(&mut id, "{}:{}:{}", x.address, x.port, x.is_ssl).unwrap();
                let mut hasher = Sha256::new();
                hasher.update(id.clone().into_bytes());
                let hash = hasher.finalize();
                let hex_hash = base16ct::lower::encode_string(&hash);
                let hh = hex_hash[0..50].to_string();
                let to_add = InnerMap {
                    address: "127.0.0.1".parse().unwrap(),
                    port: 0,
                    is_ssl: false,
                    is_http2: false,
                    to_https: false,
                    rate_limit: None,
                    healthcheck: None,
                };
                cloned.insert(id, to_add);
                cloned.insert(hh, x.to_owned());
            }
            new_inner_map.insert(path.clone(), new_vec);
        }
    }
}

pub fn listdir(dir: String) -> Vec<tls::CertificateConfig> {
    let mut f = HashMap::new();
    let mut certificate_configs: Vec<tls::CertificateConfig> = vec![];
    let paths = fs::read_dir(dir).unwrap();
    for path in paths {
        let path_str = path.unwrap().path().to_str().unwrap().to_owned();
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
    for (_, v) in f.iter() {
        let y = CertificateConfig {
            cert_path: v[0].clone(),
            key_path: v[1].clone(),
        };
        certificate_configs.push(y);
    }
    certificate_configs
}

pub fn watch_folder(path: String, sender: Sender<Vec<CertificateConfig>>) -> notify::Result<()> {
    let (tx, rx) = channel();
    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;
    watcher.watch(path.as_ref(), RecursiveMode::Recursive)?;
    info!("Watching for certificates in : {}", path);
    let certificate_configs = listdir(path.clone());
    sender.send(certificate_configs)?;
    let mut start = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Ok(event)) => match &event.kind {
                EventKind::Modify(ModifyKind::Data(_)) | EventKind::Create(_) | EventKind::Remove(_) => {
                    if start.elapsed() > Duration::from_secs(1) {
                        start = Instant::now();
                        let certificate_configs = listdir(path.clone());
                        sender.send(certificate_configs)?;
                        info!("Certificate changed: {:?}, {:?}", event.kind, event.paths);
                    }
                }
                _ => {}
            },
            Ok(Err(e)) => error!("Watch error: {:?}", e),
            Err(_) => {}
        }
    }
}

pub fn drop_priv(user: String, group: String, http_addr: String, tls_addr: Option<String>) {
    thread::sleep(time::Duration::from_millis(10));
    loop {
        thread::sleep(time::Duration::from_millis(10));
        if is_port_reachable(http_addr.clone()) {
            break;
        }
    }
    if let Some(tls_addr) = tls_addr {
        loop {
            thread::sleep(time::Duration::from_millis(10));
            if is_port_reachable(tls_addr.clone()) {
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
    let port = SocketAddr::from_str(addr).map(|sa| sa.port()).unwrap();
    match port < 1024 {
        true => {
            let meta = std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap();
            if meta != 0 {
                error!("Running on privileged port requires to start as ROOT");
                process::exit(1)
            }
        }
        false => {}
    }
}
