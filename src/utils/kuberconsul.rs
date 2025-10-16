use crate::utils::structs::{InnerMap, ServiceMapping, UpstreamsDashMap};
use dashmap::DashMap;
use std::sync::atomic::AtomicUsize;

pub fn list_to_upstreams(lt: Option<DashMap<String, (Vec<InnerMap>, AtomicUsize)>>, upstreams: &UpstreamsDashMap, i: &ServiceMapping) {
    if let Some(list) = lt {
        match upstreams.get(&i.hostname.clone()) {
            Some(upstr) => {
                for (k, v) in list {
                    upstr.value().insert(k, v);
                }
            }
            None => {
                upstreams.insert(i.hostname.clone(), list);
            }
        };
    }
}

pub fn match_path(conf: &ServiceMapping, upstreams: &DashMap<String, (Vec<InnerMap>, AtomicUsize)>, values: Vec<InnerMap>) {
    match conf.path {
        Some(ref p) => {
            upstreams.insert(p.to_string(), (values, AtomicUsize::new(0)));
        }
        None => {
            upstreams.insert("/".to_string(), (values, AtomicUsize::new(0)));
        }
    }
}
