use dashmap::DashMap;
use std::any::type_name;
use std::sync::atomic::AtomicUsize;

#[allow(dead_code)]
pub fn print_upstreams(upstreams: &UpstresmDashMap) {
    for host_entry in upstreams.iter() {
        let hostname = host_entry.key();
        println!("Hostname: {}", hostname);

        for path_entry in host_entry.value().iter() {
            let path = path_entry.key();
            println!("  Path: {}", path);

            for (ip, port, ssl, proto) in path_entry.value().0.clone() {
                println!("   ===> IP: {}, Port: {}, SSL: {}, Proto: {}", ip, port, ssl, proto);
            }
        }
    }
}

pub type UpstresmDashMap = DashMap<String, DashMap<String, (Vec<(String, u16, bool, String)>, AtomicUsize)>>;
pub type UpstreamMap = DashMap<String, (Vec<(String, u16)>, AtomicUsize)>;

#[allow(dead_code)]
pub fn typeoff<T>(_: T) {
    let to = type_name::<T>();
    println!("{:?}", to);
}

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
