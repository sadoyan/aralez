use crate::core::webserver;
use crate::ingress::consul::ConsulDiscovery;
use crate::ingress::kuberconsul::ServiceDiscovery;
use crate::ingress::kubernetes::KubernetesDiscovery;
use crate::utils::types::{Configuration, UpstreamsDashMap};
use crate::utils::watch;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

pub struct APIUpstreamProvider {
    pub config_api_enabled: bool,
    pub address: String,
    pub masterkey: Option<String>,
    pub certs_dir: String,
    pub config_dir: String,
    pub upstreams_file: String,
    pub file_server_address: Option<String>,
    pub file_server_folder: Option<String>,
    pub current_upstreams: Arc<UpstreamsDashMap>,
    pub full_upstreams: Arc<UpstreamsDashMap>,
}

pub struct FromFileProvider {
    pub path: String,
}

pub struct ConsulProvider {
    pub config: Arc<Configuration>,
}

pub struct KubernetesProvider {
    pub config: Arc<Configuration>,
}

#[async_trait]
pub trait Discovery {
    async fn start(&self, tx: Sender<Configuration>);
}

#[async_trait]
impl Discovery for APIUpstreamProvider {
    async fn start(&self, toreturn: Sender<Configuration>) {
        webserver::run_server(self, toreturn, self.current_upstreams.clone(), self.full_upstreams.clone()).await;
    }
}

#[async_trait]
impl Discovery for FromFileProvider {
    async fn start(&self, tx: Sender<Configuration>) {
        tokio::spawn(watch::file_watch(self.path.clone(), tx));
    }
}

#[async_trait]
impl Discovery for ConsulProvider {
    async fn start(&self, tx: Sender<Configuration>) {
        tokio::spawn(ConsulDiscovery.fetch_upstreams(self.config.clone(), tx));
    }
}

#[async_trait]
impl Discovery for KubernetesProvider {
    async fn start(&self, tx: Sender<Configuration>) {
        tokio::spawn(KubernetesDiscovery.fetch_upstreams(self.config.clone(), tx));
    }
}
