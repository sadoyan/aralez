use dashmap::DashMap;
use futures::StreamExt;
use hyper::Uri;
use k8s_openapi::api::core::v1::{Node, Service};
use k8s_openapi::api::networking::v1::Ingress;
use kube::api::{Patch, PatchParams};
use kube::{
    api::Api,
    client::Client,
    config::{AuthInfo, Config},
    runtime::watcher::{self, Event},
};
use log::{error, info};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

pub struct AralezRouteUpdate {
    pub host: String,
    pub path: String,
    pub namespace: String,
    pub service_name: String,
    pub is_deleted: bool,
    pub rate_limit: Option<isize>,
    pub x4xx_limit: Option<u32>,
    pub client_headers: Option<Vec<String>>,
    pub server_headers: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct AralezAnnotation {
    rate_limit: Option<isize>,
    x4xx_limit: Option<u32>,
    namespace: String,
    client_headers: Option<Vec<String>>,
    server_headers: Option<Vec<String>>,
}

lazy_static::lazy_static! {
    static ref PATCHED_INGRESSES: Arc<DashMap<String, String>> = Arc::new(DashMap::new());
}
pub async fn start_ingress_watcher_with_config(
    api_server: &str,
    token: &str,
    tx: mpsc::Sender<AralezRouteUpdate>,
    target_class: &'static str,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = create_custom_k8s_client(api_server, token, true).await?;
    let ingresses: Api<Ingress> = Api::all(client.clone());

    let watcher_config = watcher::Config::default().any_semantic();
    let mut stream = watcher::watcher(ingresses, watcher_config).boxed();

    while let Some(event) = stream.next().await {
        match event {
            Ok(Event::Apply(ing)) => {
                process_ingress_change(&client, &ing, target_class, false, &tx).await;
            }
            Ok(Event::Delete(ing)) => {
                process_ingress_change(&client, &ing, target_class, true, &tx).await;
            }
            Ok(Event::InitApply(ing)) => {
                process_ingress_change(&client, &ing, target_class, false, &tx).await;
            }
            Ok(Event::InitDone) => {
                info!("Ingress watcher initial sync complete");
            }
            Err(err) => {
                error!("Kubernetes Ingress watch error: {:?}", err);
            }
            _ => {}
        }
    }
    Ok(())
}

async fn process_ingress_change(client: &Client, ing: &Ingress, target_class: &str, is_deleted: bool, tx: &mpsc::Sender<AralezRouteUpdate>) {
    let spec = match &ing.spec {
        Some(s) => s,
        None => return,
    };
    let class_name = spec.ingress_class_name.as_deref().unwrap_or_default();
    if class_name != target_class {
        return;
    }

    if !is_deleted {
        if let (Some(name), Some(ns)) = (ing.metadata.name.as_deref(), ing.metadata.namespace.as_deref()) {
            let key = format!("{}/{}", ns, name);
            let external_ip = resolve_external_ip(client).await;
            let status_already_correct = ing
                .status
                .as_ref()
                .and_then(|s| s.load_balancer.as_ref())
                .and_then(|lb| lb.ingress.as_ref())
                .map(|list| list.iter().any(|i| i.ip.as_deref() == Some(&external_ip)))
                .unwrap_or(false);

            if status_already_correct {
                PATCHED_INGRESSES.insert(key, external_ip);
            } else {
                PATCHED_INGRESSES.insert(key.clone(), external_ip.clone());
                let client_clone = client.clone();
                let name_clone = name.to_string();
                let ns_clone = ns.to_string();
                let key_clone = key.clone();
                tokio::spawn(async move {
                    if let Err(e) = patch_ingress_status_address(&client_clone, &ns_clone, &name_clone, &external_ip).await {
                        error!("Failed to patch ingress status address {}/{}: {}", &ns_clone, &name_clone, e);
                        PATCHED_INGRESSES.remove(&key_clone);
                    }
                });
            }
        }
    } else {
        if let (Some(name), Some(ns)) = (ing.metadata.name.as_deref(), ing.metadata.namespace.as_deref()) {
            PATCHED_INGRESSES.remove(&format!("{}/{}", ns, name));
        }
    }

    if let Some(rules) = &spec.rules {
        for rule in rules {
            let host = rule.host.clone().unwrap_or_else(|| "*".to_string());

            if let Some(http) = &rule.http {
                for path_entry in &http.paths {
                    let path = path_entry.path.clone().unwrap_or_else(|| "/".to_string());
                    if let Some(service) = &path_entry.backend.service {
                        let service_name = service.name.clone();
                        let annot = parse_ingress_config(ing);
                        let update = AralezRouteUpdate {
                            host: host.clone(),
                            path,
                            service_name,
                            is_deleted,
                            rate_limit: annot.rate_limit,
                            x4xx_limit: annot.x4xx_limit,
                            client_headers: annot.client_headers,
                            server_headers: annot.server_headers,
                            namespace: annot.namespace,
                        };
                        if let Err(e) = tx.send(update).await {
                            error!("Failed to send route update to channel: {:?}", e);
                        }
                    }
                }
            }
        }
    }
}

pub async fn create_custom_k8s_client(api_server_url: &str, bearer_token: &str, insecure_tls: bool) -> Result<Client, Box<dyn std::error::Error>> {
    let full_url = if !api_server_url.starts_with("http://") && !api_server_url.starts_with("https://") {
        format!("https://{}", api_server_url)
    } else {
        api_server_url.to_string()
    };
    let uri: Uri = full_url.parse()?;
    let mut config = Config::new(uri);
    config.auth_info = AuthInfo {
        token: Some(bearer_token.trim().to_string().into()),
        ..Default::default()
    };
    config.connect_timeout = Some(Duration::from_secs(10));
    config.read_timeout = None;
    if insecure_tls {
        config.accept_invalid_certs = true;
    }
    let client = Client::try_from(config)?;
    Ok(client)
}

fn parse_ingress_config(ing: &Ingress) -> AralezAnnotation {
    let namespace = ing.metadata.namespace.clone().unwrap_or_else(|| "default".to_string());
    let annotations = ing.metadata.annotations.as_ref();

    let client_headers = annotations.and_then(|a| a.get("aralez.rs/client_headers")).and_then(|ch| serde_json::from_str(ch).ok());

    let server_headers = annotations.and_then(|a| a.get("aralez.rs/server_headers")).and_then(|sh| serde_json::from_str(sh).ok());

    AralezAnnotation {
        rate_limit: annotations.and_then(|a| a.get("aralez.rs/rate_limit")).and_then(|v| v.parse().ok()),
        x4xx_limit: annotations.and_then(|a| a.get("aralez.rs/x4xx_limit")).and_then(|v| v.parse().ok()),
        client_headers,
        server_headers,
        namespace,
    }
}

pub async fn patch_ingress_status_address(client: &Client, namespace: &str, name: &str, external_ip: &str) -> Result<(), kube::Error> {
    let ingresses: Api<Ingress> = Api::namespaced(client.clone(), namespace);
    let patch = json!({
        "status": {
            "loadBalancer": {
                "ingress": [
                    {
                        "ip": external_ip
                    }
                ]
            }
        }
    });
    let params = PatchParams::default();

    match ingresses.patch_status(name, &params, &Patch::Strategic(patch)).await {
        Ok(_) => {
            info!("Successfully patched Ingress {}/{} ADDRESS to {}", namespace, name, external_ip);
            Ok(())
        }
        Err(e) => {
            error!("Failed to patch status for Ingress {}/{}: {:?}", namespace, name, e);
            Err(e)
        }
    }
}

pub async fn discover_node_ip(client: &Client) -> Option<String> {
    let nodes: Api<Node> = Api::all(client.clone());
    if let Ok(node_list) = nodes.list(&Default::default()).await {
        for node in node_list {
            if let Some(status) = node.status {
                if let Some(addresses) = status.addresses {
                    for addr in &addresses {
                        if addr.type_ == "ExternalIP" {
                            return Some(addr.address.clone());
                        }
                    }
                    for addr in &addresses {
                        if addr.type_ == "InternalIP" {
                            return Some(addr.address.clone());
                        }
                    }
                }
            }
        }
    }
    None
}
pub async fn discover_controller_ip(client: &Client, namespace: &str, service_name: &str) -> Option<String> {
    let services: Api<Service> = Api::namespaced(client.clone(), namespace);
    if let Ok(svc) = services.get(service_name).await {
        if let Some(status) = svc.status {
            if let Some(lb) = status.load_balancer {
                if let Some(ingress_list) = lb.ingress {
                    for ing in ingress_list {
                        if let Some(ip) = ing.ip {
                            return Some(ip);
                        }
                        if let Some(hostname) = ing.hostname {
                            return Some(hostname);
                        }
                    }
                }
            }
        }
    }
    None
}

pub async fn resolve_external_ip(client: &Client) -> String {
    if let Ok(ip) = std::env::var("ARALEZ_EXTERNAL_IP") {
        let trimmed = ip.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let svc_name = std::env::var("ARALEZ_SERVICE_NAME").unwrap_or_else(|_| "aralez-service".to_string());
    let svc_ns = std::env::var("ARALEZ_SERVICE_NAMESPACE").unwrap_or_else(|_| "aralez".to_string());

    if let Some(ip) = discover_controller_ip(client, &svc_ns, &svc_name).await {
        return ip;
    }
    if let Some(ip) = discover_node_ip(client).await {
        return ip;
    }

    "127.0.0.1".to_string()
}
