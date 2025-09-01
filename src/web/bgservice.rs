use crate::utils::discovery::{APIUpstreamProvider, ConsulProvider, Discovery, FromFileProvider, KubernetesProvider};
use crate::utils::parceyaml::load_configuration;
use crate::utils::structs::Configuration;
use crate::utils::tools::*;
use crate::utils::*;
use crate::web::proxyhttp::LB;
use async_trait::async_trait;
use dashmap::DashMap;
use futures::channel::mpsc;
use futures::{SinkExt, StreamExt};
use log::{error, info};
use pingora_core::server::ShutdownWatch;
use pingora_core::services::background::BackgroundService;
use std::sync::Arc;

#[async_trait]
impl BackgroundService for LB {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        info!("Starting background service"); // tx: Sender<Configuration>
        let (mut tx, mut rx) = mpsc::channel::<Configuration>(1);
        let tx_api = tx.clone();
        let config = load_configuration(self.config.upstreams_conf.clone().as_str(), "filepath")
            .await
            .expect("Failed to load configuration");

        match config.typecfg.as_str() {
            "file" => {
                info!("Running File discovery, requested type is: {}", config.typecfg);
                tx.send(config).await.unwrap();
                let file_load = FromFileProvider {
                    path: self.config.upstreams_conf.clone(),
                };
                let _ = tokio::spawn(async move { file_load.start(tx).await });
            }
            "kubernetes" => {
                info!("Running Kubernetes discovery, requested type is: {}", config.typecfg);
                let cf = Arc::from(config);
                let kuber_load = KubernetesProvider { config: cf.clone() };
                let _ = tokio::spawn(async move { kuber_load.start(tx).await });
            }
            "consul" => {
                info!("Running Consul discovery, requested type is: {}", config.typecfg);
                let cf = Arc::from(config);
                let consul_load = ConsulProvider { config: cf.clone() };
                let _ = tokio::spawn(async move { consul_load.start(tx).await });
            }
            _ => {
                error!("Unknown discovery type: {}", config.typecfg);
            }
        }

        let api_load = APIUpstreamProvider {
            address: self.config.config_address.clone(),
            masterkey: self.config.master_key.clone(),
            config_api_enabled: self.config.config_api_enabled.clone(),
            tls_address: self.config.config_tls_address.clone(),
            tls_certificate: self.config.config_tls_certificate.clone(),
            tls_key_file: self.config.config_tls_key_file.clone(),
            file_server_address: self.config.file_server_address.clone(),
            file_server_folder: self.config.file_server_folder.clone(),
        };
        // let tx_api = tx.clone();
        let _ = tokio::spawn(async move { api_load.start(tx_api).await });

        let uu = self.ump_upst.clone();
        let ff = self.ump_full.clone();
        let im = self.ump_byid.clone();
        let (hc_method, hc_interval) = (self.config.hc_method.clone(), self.config.hc_interval);
        let _ = tokio::spawn(async move { healthcheck::hc2(uu, ff, im, (&*hc_method.to_string(), hc_interval.to_string().parse().unwrap())).await });

        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    break;
                }
                val = rx.next() => {
                    match val {
                        Some(ss) => {
                            clone_dashmap_into(&ss.upstreams, &self.ump_full);
                            clone_dashmap_into(&ss.upstreams, &self.ump_upst);
                            let current = self.extraparams.load_full();
                            let mut new = (*current).clone();
                            new.sticky_sessions = ss.extraparams.sticky_sessions;
                            new.to_https = ss.extraparams.to_https;
                            new.authentication = ss.extraparams.authentication.clone();
                            new.rate_limit = ss.extraparams.rate_limit;
                            self.extraparams.store(Arc::new(new));
                            self.headers.clear();

                            for entry in ss.upstreams.iter() {
                                let global_key = entry.key().clone();
                                let global_values = DashMap::new();
                                let mut target_entry = ss.headers.entry(global_key).or_insert_with(DashMap::new);
                                target_entry.extend(global_values);
                                self.headers.insert(target_entry.key().to_owned(), target_entry.value().to_owned());
                            }

                            for path in ss.headers.iter() {
                                let path_key = path.key().clone();
                                let path_headers = path.value().clone();
                                self.headers.insert(path_key.clone(), path_headers);
                                if let Some(global_headers) = ss.headers.get("GLOBAL_HEADERS") {
                                    if let Some(existing_headers) = self.headers.get_mut(&path_key) {
                                        merge_headers(&existing_headers, &global_headers);
                                    }
                                }
                            }
                            // info!("Upstreams list is changed, updating to:");
                            // print_upstreams(&self.ump_full);
                        }
                        None => {}
                    }
                }
            }
        }
    }
}
