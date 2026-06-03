use crate::tls::acme::order::refresh_order;
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
            .0
            .expect("Failed to load configuration");

        match config.typecfg.as_str() {
            "file" => {
                info!("Running File discovery, requested type is: {}", config.typecfg);
                tx.send(config).await.unwrap();
                let file_load = FromFileProvider {
                    path: self.config.upstreams_conf.clone(),
                };
                // let _ = tokio::spawn(async move { file_load.start(tx).await });
                drop(tokio::spawn(async move { file_load.start(tx).await }));
            }
            "kubernetes" => {
                info!("Running Kubernetes discovery, requested type is: {}", config.typecfg);
                let cf = Arc::from(config);
                let kuber_load = KubernetesProvider { config: cf.clone() };
                drop(tokio::spawn(async move { kuber_load.start(tx).await }));
            }
            "consul" => {
                info!("Running Consul discovery, requested type is: {}", config.typecfg);
                let cf = Arc::from(config);
                let consul_load = ConsulProvider { config: cf.clone() };
                drop(tokio::spawn(async move { consul_load.start(tx).await }));
            }
            _ => {
                error!("Unknown discovery type: {}", config.typecfg);
            }
        }

        let confdir = self.config.proxy_configs.clone().unwrap_or_else(|| "/tmp".to_string()) + "/autoconfigs";
        let certdir = self.config.proxy_configs.clone().unwrap_or_else(|| "/tmp".to_string()) + "/certificates";

        let api_load = APIUpstreamProvider {
            address: self.config.config_address.clone(),
            masterkey: self.config.master_key.clone(),
            config_api_enabled: self.config.config_api_enabled,
            upstreams_file: self.config.upstreams_conf.clone(),
            config_dir: confdir.clone(),
            certs_dir: certdir.clone(),
            file_server_address: self.config.file_server_address.clone(),
            file_server_folder: self.config.file_server_folder.clone(),
            current_upstreams: self.ump_upst.clone(),
            full_upstreams: self.ump_full.clone(),
        };
        drop(tokio::spawn(async move { api_load.start(tx_api).await }));

        let uu = self.ump_upst.clone();
        let ff = self.ump_full.clone();
        let im = self.ump_byid.clone();
        let (hc_method, hc_interval) = (self.config.hc_method.clone(), self.config.hc_interval);
        drop(tokio::spawn(async move {
            healthcheck::hc2(uu, ff, im, (&*hc_method.to_string(), hc_interval.to_string().parse().unwrap())).await
        }));
        drop(tokio::spawn(async move { refresh_order(certdir, confdir).await }));

        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    break;
                }
                val = rx.next() => {
                    if let Some(ss) = val {
                        clone_dashmap_into(&ss.upstreams, &self.ump_full);
                        clone_dashmap_into(&ss.upstreams, &self.ump_upst);
                        clone_idmap_into(&ss.upstreams, &self.ump_byid);
                        let current = self.extraparams.load_full();
                        let mut new = (*current).clone();
                        new.to_https = ss.extraparams.to_https;
                        new.sticky_sessions = ss.extraparams.sticky_sessions;
                        new.authentication = ss.extraparams.authentication.clone();
                        new.rate_limit = ss.extraparams.rate_limit;
                        new.x4xx_limit = ss.extraparams.x4xx_limit;
                        self.extraparams.store(Arc::new(new));
                        self.client_headers.clear();
                        self.server_headers.clear();
                        for entry in ss.upstreams.iter() {
                            let global_key = entry.key().clone();
                            let client_global_values = DashMap::new();
                            let server_global_values = DashMap::new();

                            let mut client_target_entry = ss.client_headers.entry(global_key.clone()).or_insert_with(DashMap::new);
                            client_target_entry.extend(client_global_values);
                            let mut server_target_entry = ss.server_headers.entry(global_key).or_insert_with(DashMap::new);
                            server_target_entry.extend(server_global_values);
                            self.server_headers.insert(server_target_entry.key().to_owned(), server_target_entry.value().to_owned());
                        }
                        for path in ss.client_headers.iter() {
                            let path_key = path.key().clone();
                            let path_headers = path.value().clone();
                            self.client_headers.insert(path_key.clone(), path_headers);
                            if let Some(global_headers) = ss.client_headers.get("GLOBAL_CLIENT_HEADERS") {
                                if let Some(existing_headers) = self.client_headers.get_mut(&path_key) {
                                    merge_headers(&existing_headers, &global_headers);
                                }
                            }
                        }
                        for path in ss.server_headers.iter() {
                            let path_key = path.key().clone();
                            let path_headers = path.value().clone();
                            self.server_headers.insert(path_key.clone(), path_headers);
                            if let Some(global_headers) = ss.server_headers.get("GLOBAL_SERVER_HEADERS") {
                                if let Some(existing_headers) = self.server_headers.get_mut(&path_key) {
                                    merge_headers(&existing_headers, &global_headers);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
