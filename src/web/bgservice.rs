use crate::utils::discovery::{APIUpstreamProvider, ConsulProvider, Discovery, FromFileProvider};
use crate::utils::parceyaml::Configuration;
use crate::utils::tools::*;
use crate::utils::*;
use crate::web::proxyhttp::LB;
use async_trait::async_trait;
use dashmap::DashMap;
use futures::channel::mpsc;
use futures::StreamExt;
use log::{error, info};
use pingora_core::server::ShutdownWatch;
use pingora_core::services::background::BackgroundService;

#[async_trait]
impl BackgroundService for LB {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        info!("Starting background service");
        let (tx, mut rx) = mpsc::channel::<Configuration>(0);

        let from_file = self.config.get("upstreams_conf");
        match from_file {
            Some(from_file) => {
                let tx_file = tx.clone();
                let tx_consul = tx.clone();

                let file_load = FromFileProvider { path: from_file.to_string() };
                let consul_load = ConsulProvider { path: from_file.to_string() };

                let _ = tokio::spawn(async move { file_load.start(tx_file).await });
                let _ = tokio::spawn(async move { consul_load.start(tx_consul).await });
            }
            None => {
                error!("Can't read config file");
            }
        }
        let config_address = self.config.get("config_address");
        let masterkey = self.config.get("master_key").unwrap();
        match config_address {
            Some(config_address) => {
                let api_load = APIUpstreamProvider {
                    address: config_address.to_string(),
                    masterkey: masterkey.value().to_string(),
                };
                let tx_api = tx.clone();
                let _ = tokio::spawn(async move { api_load.start(tx_api).await });
            }
            None => {
                error!("Can't read config file");
            }
        }

        let uu = self.ump_upst.clone();
        let ff = self.ump_full.clone();
        let im = self.ump_byid.clone();
        let (hc_method, hc_interval) = (self.config.get("hc_method").unwrap().clone(), self.config.get("hc_interval").unwrap().clone());
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
                            self.proxyconf.clear();
                            match ss.globals {
                                Some(globals) => {
                                    for (k,v) in globals {
                                        self.proxyconf.insert(k, v);
                                    }
                                }
                                None => {}
                            }
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
                            info!("Upstreams list is changed, updating to:");
                            print_upstreams(&self.ump_full);
                        }
                        None => {}
                    }
                }
            }
        }
    }
}
