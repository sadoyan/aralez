use crate::utils::structs::{Configuration, GlobalServiceMapping, InnerMap, UpstreamsDashMap};
use crate::utils::tools::{clone_dashmap_into, compare_dashmaps, print_upstreams};
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc::Sender;

#[allow(clippy::type_complexity)]
pub fn list_to_upstreams(lt: Option<DashMap<Arc<str>, (Vec<Arc<InnerMap>>, AtomicUsize)>>, upstreams: &UpstreamsDashMap, i: &GlobalServiceMapping) {
    if let Some(list) = lt {
        let key: Arc<str> = Arc::from(i.hostname.as_str());
        match upstreams.get(&key) {
            Some(upstr) => {
                for (k, v) in list {
                    upstr.value().insert(k, v);
                }
            }
            None => {
                upstreams.insert(key, list);
            }
        };
    }
}

pub fn match_path(conf: &GlobalServiceMapping, upstreams: &DashMap<Arc<str>, (Vec<Arc<InnerMap>>, AtomicUsize)>, values: Vec<Arc<InnerMap>>) {
    let path = conf.path.as_deref().unwrap_or("/");
    upstreams.insert(Arc::from(path), (values, AtomicUsize::new(0)));
}

pub async fn read_token(path: &str) -> String {
    let mut file = File::open(path).await.unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).await.unwrap();
    contents.trim().to_string()
}

#[async_trait]
pub trait ServiceDiscovery {
    async fn fetch_upstreams(&self, config: Arc<Configuration>, toreturn: Sender<Configuration>);
}

pub async fn clone_compare(upstreams: &UpstreamsDashMap, config: &Arc<Configuration>) -> Option<Configuration> {
    if !compare_dashmaps(upstreams, &config.upstreams) {
        let tosend = Configuration {
            upstreams: Default::default(),
            client_headers: config.client_headers.clone(),
            server_headers: config.server_headers.clone(),
            consul: config.consul.clone(),
            kubernetes: config.kubernetes.clone(),
            typecfg: config.typecfg.clone(),
            extraparams: config.extraparams.clone(),
        };
        clone_dashmap_into(upstreams, &config.upstreams);
        clone_dashmap_into(upstreams, &tosend.upstreams);

        print_upstreams(&tosend.upstreams, &tosend.extraparams);
        return Some(tosend);
    }
    None
}
