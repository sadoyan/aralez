use crate::utils::httpclient;
use crate::utils::kuberconsul::{list_to_upstreams, ServiceDiscovery};
use crate::utils::parceyaml::build_headers;
use crate::utils::structs::{Configuration, GlobalServiceMapping, UpstreamsDashMap};
use async_trait::async_trait;
use dashmap::DashMap;
use pingora::prelude::sleep;
use rand::RngExt;
use serde::Deserialize;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Sender;

#[derive(Debug, Deserialize)]
pub struct ConsulService {
    #[serde(rename = "Address")]
    pub address: String,
    #[serde(rename = "ServicePort")]
    pub port: u16,
}
pub struct ConsulDiscovery;

#[async_trait]
impl ServiceDiscovery for ConsulDiscovery {
    async fn fetch_upstreams(&self, config: Arc<Configuration>, toreturn: Sender<Configuration>) {
        loop {
            let upstreams = UpstreamsDashMap::new();

            if let Some(consul) = config.consul.clone() {
                let servers = consul.servers.unwrap_or_else(|| {
                    vec![format!(
                        "{}:{}",
                        env::var("CONSUL_SERVICE_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
                        env::var("CONSUL_SERVICE_PORT").unwrap_or_else(|_| "0".to_string())
                    )]
                });
                let end = servers.len().saturating_sub(1);
                let num = if end > 0 { rand::rng().random_range(0..end) } else { 0 };
                let consul_data = servers.get(num).unwrap().to_string();
                let ss = format!("{}/v1/catalog/service/", consul_data);
                let dc = format!("{}/v1/catalog/services?filter=%22aralez.service=yes%22+in+ServiceTags", consul_data);

                if let Some(infolist) = httpclient::for_consul_list(dc.as_str(), consul.token.clone()).await {
                    for (k, v) in infolist {
                        let client_header_map: DashMap<Arc<str>, Vec<(String, Arc<str>)>> = DashMap::new();
                        let mut client_header_list = Vec::new();
                        build_headers(&v.client_headers, config.as_ref(), &mut client_header_list);

                        let sever_header_map: DashMap<Arc<str>, Vec<(String, Arc<str>)>> = DashMap::new();
                        let mut server_header_list = Vec::new();
                        build_headers(&v.server_headers, config.as_ref(), &mut server_header_list);

                        if !client_header_list.is_empty() {
                            let path_key = v.path.clone();
                            client_header_map.insert(Arc::from(path_key), client_header_list);
                            config.client_headers.insert(Arc::from(v.host.clone()), client_header_map);
                        }
                        if !server_header_list.is_empty() {
                            let path_key = v.path.clone();
                            sever_header_map.insert(Arc::from(path_key), server_header_list);
                            config.server_headers.insert(Arc::from(v.host.clone()), sever_header_map);
                        }
                        let pref = format!("{}{}", ss, k);
                        let gsm = &GlobalServiceMapping {
                            upstream: k.clone(),
                            hostname: v.host.clone(),
                            path: Some(v.path),
                            to_https: v.to_https,
                            redirect_to: v.redirect,
                            authorization: v.auth,
                            rate_limit: v.rate,
                            x4xx_limit: v.xrate,
                            client_headers: v.client_headers,
                            server_headers: v.server_headers,
                        };

                        let list = httpclient::for_consul(pref.as_str(), consul.token.clone(), gsm).await;
                        list_to_upstreams(list, &upstreams, gsm);
                    }
                }
            }

            if let Some(lt) = crate::utils::kuberconsul::clone_compare(&upstreams, &config).await {
                let _ = toreturn.send(lt).await;
            }

            sleep(Duration::from_secs(5)).await;
        }
    }
}
