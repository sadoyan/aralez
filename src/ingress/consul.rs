use crate::ingress::kuberconsul;
use crate::ingress::kuberconsul::{ServiceDiscovery, list_to_upstreams};
use crate::utils::httpclient;
use crate::utils::parceyaml::build_headers;
use crate::utils::tools::Die;
use crate::utils::types::{Configuration, GlobalServiceMapping, Headers, UpstreamsDashMap};
use async_trait::async_trait;
use pingora::prelude::sleep;
use rand::RngExt;
use serde::Deserialize;
use std::collections::HashMap;
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
            let mut upstreams = UpstreamsDashMap::new();

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
                let consul_data = servers.get(num).die("Reading consul data").to_string();
                let ss = format!("{}/v1/catalog/service/", consul_data);
                let dc = format!("{}/v1/catalog/services?filter=%22aralez.service=yes%22+in+ServiceTags", consul_data);

                let mut local_client_headers: Headers = HashMap::new();
                let mut local_server_headers: Headers = HashMap::new();

                if let Some(infolist) = httpclient::for_consul_list(dc.as_str(), consul.token.clone()).await {
                    for (k, v) in infolist {
                        let mut client_header_list = Vec::new();
                        build_headers(&v.client_headers, config.as_ref(), &mut client_header_list);

                        let mut server_header_list = Vec::new();
                        build_headers(&v.server_headers, config.as_ref(), &mut server_header_list);

                        let host_key: Arc<str> = Arc::from(v.host.clone());
                        let path_key: Arc<str> = Arc::from(v.path.clone());

                        // 2. Populate the local scratch maps instead of mutating config directly
                        if !client_header_list.is_empty() {
                            local_client_headers
                                .entry(host_key.clone())
                                .or_insert_with(HashMap::new)
                                .insert(path_key.clone(), client_header_list);
                        }
                        if !server_header_list.is_empty() {
                            local_server_headers
                                .entry(host_key.clone())
                                .or_insert_with(HashMap::new)
                                .insert(path_key.clone(), server_header_list);
                        }

                        let pref = format!("{}{}", ss, k);
                        let gsm = &GlobalServiceMapping {
                            upstream: k,
                            hostname: v.host,
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
                        list_to_upstreams(list, &mut upstreams, gsm);
                    }
                }
            }

            if let Some(lt) = kuberconsul::clone_compare(&upstreams, &config).await {
                let _ = toreturn.send(lt).await;
            }

            sleep(Duration::from_secs(5)).await;
        }
    }
}
