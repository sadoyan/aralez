use dashmap::DashMap;
use std::sync::atomic::AtomicUsize;

pub fn discover() -> DashMap<String, (Vec<(String, u16)>, AtomicUsize)> {
    let upstreams: DashMap<String, (Vec<(String, u16)>, AtomicUsize)> = DashMap::new();
    let mut toreturn = vec![];
    toreturn.push(("192.168.1.1".to_string(), 8000.to_owned()));
    toreturn.push(("192.168.1.10".to_string(), 8000.to_owned()));
    toreturn.push(("127.0.0.1".to_string(), 8000.to_owned()));
    toreturn.push(("127.0.0.2".to_string(), 8000.to_owned()));
    toreturn.push(("127.0.0.3".to_string(), 8000.to_owned()));
    toreturn.push(("127.0.0.4".to_string(), 8000.to_owned()));
    toreturn.push(("127.0.0.5".to_string(), 8000.to_owned()));
    toreturn.push(("127.0.0.6".to_string(), 8000.to_owned()));
    upstreams.insert("myip.netangels.net".to_string(), (toreturn, AtomicUsize::new(0)));
    let mut toreturn = vec![];
    toreturn.push(("192.168.1.1".to_string(), 8000.to_owned()));
    toreturn.push(("192.168.1.10".to_string(), 8000.to_owned()));
    upstreams.insert("polo.netangels.net".to_string(), (toreturn, AtomicUsize::new(0)));
    let mut toreturn = vec![];
    toreturn.push(("192.168.1.20".to_string(), 8000.to_owned()));
    upstreams.insert("glop.netangels.net".to_string(), (toreturn, AtomicUsize::new(0)));
    upstreams
}
