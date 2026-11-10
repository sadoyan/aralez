use crate::utils::tools::{Die, compare_dashmaps, print_upstreams};
use crate::utils::types::{Configuration, GlobalServiceMapping, InnerMap, UpstreamsDashMap};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc::Sender;

#[allow(clippy::type_complexity)]
pub fn list_to_upstreams(lt: Option<HashMap<Arc<str>, (Vec<Arc<InnerMap>>, AtomicUsize)>>, upstreams: &mut UpstreamsDashMap, i: &GlobalServiceMapping) {
    if let Some(list) = lt {
        let key: Arc<str> = Arc::from(i.hostname.as_str());
        let entry = upstreams.entry(key).or_insert_with(HashMap::new);
        for (k, v) in list {
            entry.insert(k, v);
        }
    }
}

pub fn match_path(conf: &GlobalServiceMapping, upstreams: &mut HashMap<Arc<str>, (Vec<Arc<InnerMap>>, AtomicUsize)>, values: Vec<Arc<InnerMap>>) {
    let path = conf.path.as_deref().unwrap_or("/");
    upstreams.insert(Arc::from(path), (values, AtomicUsize::new(0)));
}

pub async fn read_token(path: &str) -> String {
    let mut file = File::open(path).await.die("Reading kubernetes token");
    let mut contents = String::new();
    file.read_to_string(&mut contents).await.die("Kubernetes token to string");
    contents.trim().to_string()
}

#[async_trait]
pub trait ServiceDiscovery {
    async fn fetch_upstreams(&self, config: Arc<Configuration>, toreturn: Sender<Configuration>);
}

pub async fn clone_compare(new_upstreams: &UpstreamsDashMap, config: &Configuration) -> Option<Configuration> {
    if !compare_dashmaps(&new_upstreams, &config.upstreams) {
        let mut cloned_upstreams = HashMap::new();
        for (host_key, inner_map) in new_upstreams {
            let mut cloned_inner = HashMap::new();
            for (path_key, (servers, atomic_val)) in inner_map {
                let current_val = atomic_val.load(Ordering::Relaxed);
                let new_atomic = AtomicUsize::new(current_val);
                cloned_inner.insert(path_key.clone(), (servers.clone(), new_atomic));
            }
            cloned_upstreams.insert(host_key.clone(), cloned_inner);
        }
        let tosend = Configuration {
            upstreams: cloned_upstreams,
            client_headers: config.client_headers.clone(),
            server_headers: config.server_headers.clone(),
            consul: config.consul.clone(),
            kubernetes: config.kubernetes.clone(),
            typecfg: config.typecfg.clone(),
            extraparams: config.extraparams.clone(),
        };
        print_upstreams(&tosend.upstreams, &tosend.extraparams);
        return Some(tosend);
    }
    None
}
