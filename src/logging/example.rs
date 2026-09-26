use crate::logging::core::StructuredSystemLog;
use crate::logging::types::{LogBackendPlugin, WriteLog};
use async_trait::async_trait;
use tokio::sync::mpsc;

pub struct Example;

#[async_trait]
impl WriteLog for Example {
    async fn run(&self, mut rx: mpsc::Receiver<StructuredSystemLog>) {
        while let Some(msg) = rx.recv().await {
            println!("Sending log Example : {:?}", msg);
        }
    }
}

inventory::submit! {
    LogBackendPlugin {
        name: "example",
        factory: || Box::new(Example),
    }
}
