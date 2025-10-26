use crate::utils::filewatch;
use crate::utils::kuberconsul::{ConsulDiscovery, KubernetesDiscovery, ServiceDiscovery};
use crate::utils::structs::Configuration;
use crate::web::webserver;
use async_trait::async_trait;
use futures::channel::mpsc::Sender;
use std::sync::Arc;

pub struct APIUpstreamProvider {
    pub config_api_enabled: bool,
    pub address: String,
    pub masterkey: String,
    pub tls_address: Option<String>,
    pub tls_certificate: Option<String>,
    pub tls_key_file: Option<String>,
    pub file_server_address: Option<String>,
    pub file_server_folder: Option<String>,
}

#[async_trait]
impl Discovery for APIUpstreamProvider {
    async fn start(&self, toreturn: Sender<Configuration>) {
        webserver::run_server(self, toreturn).await;
    }
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
impl Discovery for FromFileProvider {
    async fn start(&self, tx: Sender<Configuration>) {
        tokio::spawn(filewatch::start(self.path.clone(), tx.clone()));
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
