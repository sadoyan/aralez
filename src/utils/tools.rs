use crate::utils::structs::{UpstreamsDashMap, UpstreamsIdMap};
use dashmap::DashMap;
use sha2::{Digest, Sha256};
use std::any::type_name;
use std::collections::HashSet;
use std::fmt::Write;
use std::sync::atomic::AtomicUsize;

#[allow(dead_code)]
pub fn print_upstreams(upstreams: &UpstreamsDashMap) {
    for host_entry in upstreams.iter() {
        let hostname = host_entry.key();
        println!("Hostname: {}", hostname);

        for path_entry in host_entry.value().iter() {
            let path = path_entry.key();
            println!(" Path: {}", path);

            for (ip, port, ssl, vers) in path_entry.value().0.clone() {
                println!("    ===> IP: {}, Port: {}, SSL: {}, H2: {}", ip, port, ssl, vers);
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

pub fn merge_headers(target: &DashMap<String, Vec<(String, String)>>, source: &DashMap<String, Vec<(String, String)>>) {
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
                write!(&mut id, "{}:{}:{}", x.0, x.1, x.2).unwrap();
                let mut hasher = Sha256::new();
                hasher.update(id.clone().into_bytes());
                let hash = hasher.finalize();
                let hex_hash = base16ct::lower::encode_string(&hash);
                let hh = hex_hash[0..50].to_string();
                cloned.insert(id, (hh.clone(), 0000, false, false));
                cloned.insert(hh, x.to_owned());
            }
            new_inner_map.insert(path.clone(), new_vec);
        }
    }
}
