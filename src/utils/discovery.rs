use crate::utils::filewatch;
use crate::utils::tools::*;
use crate::web::webserver;
use async_trait::async_trait;
use futures::channel::mpsc::Sender;

pub struct FromFileProvider {
    pub path: String,
}
pub struct APIUpstreamProvider {
    pub address: String,
}

#[async_trait]
pub trait Discovery {
    async fn start(&self, tx: Sender<(UpstreamsDashMap, Headers)>);
}

#[async_trait]
impl Discovery for APIUpstreamProvider {
    async fn start(&self, toreturn: Sender<(UpstreamsDashMap, Headers)>) {
        webserver::run_server(self.address.clone(), toreturn).await;
    }
}

#[async_trait]
impl Discovery for FromFileProvider {
    async fn start(&self, tx: Sender<(UpstreamsDashMap, Headers)>) {
        tokio::spawn(filewatch::start(self.path.clone(), tx.clone()));
    }
}
