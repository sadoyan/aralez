use crate::utils::structs::{InnerMap, ServiceMapping, UpstreamsDashMap};
use dashmap::DashMap;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;

#[derive(Debug, serde::Deserialize)]
pub struct KubeEndpoints {
    pub subsets: Option<Vec<KubeSubset>>,
}
#[derive(Debug, serde::Deserialize)]
pub struct KubeSubset {
    pub addresses: Option<Vec<KubeAddress>>,
    pub ports: Option<Vec<KubePort>>,
}

#[derive(Debug, serde::Deserialize)]
pub struct KubeAddress {
    pub ip: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct KubePort {
    pub port: u16,
}

#[derive(Debug, Deserialize)]
pub struct ConsulService {
    #[serde(rename = "ServiceTaggedAddresses")]
    pub tagged_addresses: HashMap<String, ConsulTaggedAddress>,
}

#[derive(Debug, Deserialize)]
pub struct ConsulTaggedAddress {
    #[serde(rename = "Address")]
    pub address: String,
    #[serde(rename = "Port")]
    pub port: u16,
}
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
