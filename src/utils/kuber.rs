// use crate::utils::dnsclient::DnsClientPool;
use crate::utils::structs::{Configuration, InnerMap, UpstreamsDashMap};
use crate::utils::tools::{clone_dashmap_into, compare_dashmaps, print_upstreams};
use dashmap::DashMap;
use futures::channel::mpsc::Sender;
use futures::SinkExt;
use pingora::prelude::sleep;
use rand::Rng;
use reqwest::Client;
use std::env;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

// static KUBERNETES_SERVICE_HOST: &str = "140.238.122.18:6443";
// static TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6InJXMzl2VFlHTDM4V0tBbWxNQnRhbnRVcDlvcS10MjRDVHNwc2p3d1ZXeDAifQ.eyJhdWQiOlsiaHR0cHM6Ly9rdWJlcm5ldGVzLmRlZmF1bHQuc3ZjLmNsdXN0ZXIubG9jYWwiXSwiZXhwIjoxNzg4MjY3OTgxLCJpYXQiOjE3NTY3MzE5ODEsImlzcyI6Imh0dHBzOi8va3ViZXJuZXRlcy5kZWZhdWx0LnN2Yy5jbHVzdGVyLmxvY2FsIiwianRpIjoiMDM4ZWVlODAtNGViZi00MjU5LWI0OTctYmYwNDgxMmYyNWJhIiwia3ViZXJuZXRlcy5pbyI6eyJuYW1lc3BhY2UiOiJzdGFnaW5nIiwibm9kZSI6eyJuYW1lIjoiMTcyLjE2LjAuMTU1IiwidWlkIjoiOGY2ZTk2ZjYtNDdjNS00YjZhLTkwMmUtYmZhZTUwNzcxMWFjIn0sInBvZCI6eyJuYW1lIjoiYXJhbGV6LTY3Njg5OGY5ZDUtaHpienEiLCJ1aWQiOiI2NjAyZDFjNy00ZWM2LTRiZDEtYTc3NS00NzI5OGYyMTc0N2QifSwic2VydmljZWFjY291bnQiOnsibmFtZSI6ImFyYWxlei1zYSIsInVpZCI6IjVjYmYxZTU2LTJjY2YtNDFlMS05OGU2LTc3NmY1ZWY1NGRkOCJ9LCJ3YXJuYWZ0ZXIiOjE3NTY3MzU1ODh9LCJuYmYiOjE3NTY3MzE5ODEsInN1YiI6InN5c3RlbTpzZXJ2aWNlYWNjb3VudDpzdGFnaW5nOmFyYWxlei1zYSJ9.fnlQoOMEztWx6aE4JNPCMVDVc4s8ygs1tvuU_4FoNTWnTipFivzV3duHAg5sHFnLexYEbeBzWqr1betk__ATfy5RB5UaDdg0kk2AdVjYhvUfW4FcIqPNzXBUd74BOUG8vhN6bhZTQC8Fh0eCLCn9XXwVGIO94LKi9LmLbFiDAhpQDGwbXYnI8kV4nNGRE3kf0fsb6SyHs_8bOGc-t2U6OPdAFqBsk4JliaqiXhJKsoc8JCfUkcxYkT7GqIZxFYpvgisbOdZL7_iVyLU1BiSPMHb0jFa4O60aZ8EzCR7Mio0F5A8eZjSCf_f90ecUCGFuW3eCTCDd02RutXeSyPqxhQ";

#[derive(Debug, serde::Deserialize)]
struct Endpoints {
    subsets: Option<Vec<Subset>>,
}

#[derive(Debug, serde::Deserialize)]
struct Subset {
    addresses: Option<Vec<Address>>,
    ports: Option<Vec<Port>>,
}

#[derive(Debug, serde::Deserialize)]
struct Address {
    ip: String,
}

#[derive(Debug, serde::Deserialize)]
struct Port {
    // name: String,
    port: u16,
}

pub async fn start(mut toreturn: Sender<Configuration>, config: Arc<Configuration>) {
    let upstreams = UpstreamsDashMap::new();
    let prev_upstreams = UpstreamsDashMap::new();
    loop {
        if let Some(kuber) = config.kubernetes.clone() {
            let path = kuber.tokenpath.unwrap_or("/var/run/secrets/kubernetes.io/serviceaccount/token".to_string());
            let token = read_token(path.as_str()).await;
            let servers = kuber.servers.unwrap_or(vec![format!(
                "{}:{}",
                env::var("KUBERNETES_SERVICE_HOST").unwrap_or("0.0.0.0".to_string()),
                env::var("KUBERNETES_SERVICE_PORT_HTTPS").unwrap_or("0".to_string())
            )]);
            let end = servers.len() - 1;
            let mut num = 0;
            if end > 0 {
                num = rand::rng().random_range(0..end);
            }
            let server = servers.get(num).unwrap().to_string();

            if let Some(svc) = kuber.services {
                for i in svc {
                    let url = format!("https://{}/api/v1/namespaces/staging/endpoints/{}", server, i.real);
                    let list = get_by_http(&*url, &*token).await;
                    if let Some(list) = list {
                        upstreams.insert(i.proxy.clone(), list);
                    }
                }
            }
        }

        if !compare_dashmaps(&upstreams, &prev_upstreams) {
            let tosend: Configuration = Configuration {
                upstreams: Default::default(),
                headers: config.headers.clone(),
                consul: config.consul.clone(),
                kubernetes: config.kubernetes.clone(),
                typecfg: config.typecfg.clone(),
                extraparams: config.extraparams.clone(),
            };

            clone_dashmap_into(&upstreams, &prev_upstreams);
            clone_dashmap_into(&upstreams, &tosend.upstreams);
            print_upstreams(&tosend.upstreams);
            toreturn.send(tosend).await.unwrap();
        }
        sleep(Duration::from_secs(5)).await;
    }
}

pub async fn get_by_http(url: &str, token: &str) -> Option<DashMap<String, (Vec<InnerMap>, AtomicUsize)>> {
    let client = Client::builder().timeout(Duration::from_secs(2)).danger_accept_invalid_certs(true).build().ok()?;

    let resp = client.get(url).bearer_auth(token).send().await.ok()?;

    if !resp.status().is_success() {
        eprintln!("Kubernetes API returned status: {}", resp.status());
        return None;
    }

    let endpoints: Endpoints = resp.json().await.ok()?;
    let upstreams: DashMap<String, (Vec<InnerMap>, AtomicUsize)> = DashMap::new();

    if let Some(subsets) = endpoints.subsets {
        for subset in subsets {
            if let (Some(addresses), Some(ports)) = (subset.addresses, subset.ports) {
                for addr in addresses {
                    let mut inner_vec = Vec::new();
                    for port in &ports {
                        let to_add = InnerMap {
                            address: addr.ip.clone(),
                            port: port.port.clone(),
                            is_ssl: false,
                            is_http2: false,
                            to_https: false,
                            rate_limit: None,
                        };
                        inner_vec.push(to_add);
                    }
                    upstreams.insert("/".to_string(), (inner_vec, AtomicUsize::new(0)));
                }
            }
        }
    }
    Some(upstreams)
}

async fn read_token(path: &str) -> String {
    let mut file = File::open(path).await.unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).await.unwrap();
    contents.trim().to_string()
}
