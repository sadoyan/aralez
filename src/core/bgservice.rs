use crate::cache::types::init_cache;
use crate::core::proxyhttp::LB;
use crate::ingress::{APIUpstreamProvider, ConsulProvider, Discovery, FromFileProvider, KubernetesProvider};
use crate::logging::core::{init_access_log, init_access_logging, log_builder};
use crate::tls::acme::order::refresh_order;
use crate::utils::metrics::calc_cache_metrics;
use crate::utils::parceyaml::load_configuration;
use crate::utils::tools::*;
use crate::utils::types::{Configuration, UpstreamsDashMap};
use crate::utils::*;
use async_trait::async_trait;
use log::{error, info};
use pingora_core::server::ShutdownWatch;
use pingora_core::services::background::BackgroundService;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

#[async_trait]
impl BackgroundService for LB {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        log_builder(&self.config, &self.config.log_file).await;

        let al = &self.config.access_log.clone().unwrap_or("none".to_string());
        let _ = init_access_log(al.as_str()).await;

        info!("Starting background service");
        let (tx, mut rx) = mpsc::channel::<Configuration>(1);
        let tx_api = tx.clone();
        let config = load_configuration(self.config.upstreams_conf.clone().as_str(), "filepath")
            .await
            .0
            .die("Failed to load configuration");

        match config.typecfg.as_str() {
            "file" => {
                info!("Running File discovery, requested type is: {}", config.typecfg);
                tx.send(config).await.die("Running File discovery");
                let file_load = FromFileProvider {
                    path: self.config.upstreams_conf.clone(),
                };
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

        let initial_config_content = std::fs::read_to_string(&self.config.upstreams_conf).unwrap_or_default();
        let parsed = crate::utils::parceyaml::load_configuration(&initial_config_content, "content").await;
        if let Some(serverlist) = parsed.0 {
            let cloned_upstreams = deep_clone_upstreams(&serverlist.upstreams);
            let cloned_fpstreams = deep_clone_upstreams(&serverlist.upstreams);
            self.ump_upst.store(Arc::new(cloned_upstreams));
            self.ump_full.store(Arc::new(cloned_fpstreams));
        }

        let api_load = APIUpstreamProvider {
            address: self.config.config_address.clone(),
            masterkey: self.config.master_key.clone(),
            config_api_enabled: self.config.config_api_enabled,
            upstreams_file: self.config.upstreams_conf.clone(),
            config_dir: confdir.clone(),
            certs_dir: certdir.clone(),
            file_server_address: self.config.file_server_address.clone(),
            file_server_folder: self.config.file_server_folder.clone(),

            // Pass the ArcSwap containers directly
            current_upstreams: self.ump_upst.clone(),
            full_upstreams: self.ump_full.clone(),

            acme_dns_provider: self.config.acme_dns_provider.clone(),
        };

        tokio::spawn(async move { api_load.start(tx_api).await });

        let (hc_method, hc_interval) = (self.config.hc_method.clone(), self.config.hc_interval);
        let im = self.ump_byid.clone();
        let uu = self.ump_upst.clone();
        let ff = self.ump_full.clone();
        let method_str = hc_method.to_string();
        let interval: u64 = hc_interval.to_string().parse().die("Unable to parse HC interval");
        tokio::spawn(async move { healthcheck::hc2(uu, ff, im, (&method_str, interval)).await });

        tokio::spawn(async move { calc_cache_metrics().await });
        let acme_provider = self.config.acme_dns_provider.clone();
        tokio::spawn(async move { refresh_order(certdir, confdir, acme_provider).await });

        init_cache(self.config.clone()).await;
        init_access_logging(self.config.access_log.clone()).await;

        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    break;
                }
                val = rx.recv() => {
                    let start = SystemTime::now().duration_since(UNIX_EPOCH).die(format!("ON {} {}", file!(), line!()).as_str());
                    if let Some(ss) = val {
                        let mut new_ump_full = HashMap::new();
                        let mut new_ump_upst = HashMap::new();
                        let mut new_ump_byid = HashMap::new();
                        clone_dashmap_into(&ss.upstreams, &mut new_ump_full);
                        clone_dashmap_into(&ss.upstreams, &mut new_ump_upst);
                        clone_idmap_into(&ss.upstreams, &mut new_ump_byid);
                        self.ump_full.store(Arc::new(new_ump_full));
                        self.ump_upst.store(Arc::new(new_ump_upst));
                        self.ump_byid.store(Arc::new(new_ump_byid));

                        // 2. Update extra configuration parameters
                        let current = self.extraparams.load_full();
                        let mut new = (*current).clone();
                        new.to_https = ss.extraparams.to_https;
                        new.sticky_sessions = ss.extraparams.sticky_sessions;
                        new.authentication = ss.extraparams.authentication.clone();
                        new.rate_limit = ss.extraparams.rate_limit;
                        new.x4xx_limit = ss.extraparams.x4xx_limit;
                        self.extraparams.store(Arc::new(new));
                        let mut new_client_headers = HashMap::new();
                        for (path_key, path_headers) in &ss.client_headers {
                            new_client_headers.insert(path_key.clone(), path_headers.clone());
                        }
                        if let Some(global_headers) = ss.client_headers.get("GLOBAL_CLIENT_HEADERS") {
                            for existing_headers in new_client_headers.values_mut() {
                                merge_headers(existing_headers, global_headers);
                            }
                        }
                        self.client_headers.store(Arc::new(new_client_headers));
                        let mut new_server_headers = HashMap::new();
                        for (path_key, path_headers) in &ss.server_headers {
                            new_server_headers.insert(path_key.clone(), path_headers.clone());
                        }
                        if let Some(global_headers) = ss.server_headers.get("GLOBAL_SERVER_HEADERS") {
                            for existing_headers in new_server_headers.values_mut() {
                                merge_headers(existing_headers, global_headers);
                            }
                        }
                        self.server_headers.store(Arc::new(new_server_headers));
                    }
                    let end = SystemTime::now().duration_since(UNIX_EPOCH).die(format!("ON {} {}", file!(), line!()).as_str());
                    let startup = end - start;
                    info!("Loading upstreams took: {}ms", startup.as_millis());
                }
            }
        }
    }
}

use std::sync::atomic::Ordering;

fn deep_clone_upstreams(original: &UpstreamsDashMap) -> UpstreamsDashMap {
    let mut new_map = HashMap::new();

    for (domain, paths) in original {
        let mut new_paths = HashMap::new();
        for (path, (backends, atomic_counter)) in paths {
            // Load the current value from the old atomic counter
            let current_val = atomic_counter.load(Ordering::Relaxed);

            // Create a fresh atomic counter with the loaded value
            let new_atomic = std::sync::atomic::AtomicUsize::new(current_val);

            // Backends are Vec<Arc<InnerMap>>, so they can be cloned safely
            new_paths.insert(path.clone(), (backends.clone(), new_atomic));
        }
        new_map.insert(domain.clone(), new_paths);
    }

    new_map
}
