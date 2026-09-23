use crate::logging::core::StructuredSystemLog;
use crate::logging::types::{LogBackendPlugin, WriteLog};
use async_trait::async_trait;

pub struct Example;
#[async_trait]
impl WriteLog for Example {
    async fn writelog(&self, msg: &StructuredSystemLog) {
        // Here comes the backend logic
        println!("Sending log Example : {:?}", msg);
    }
}

// Mandatory  with name: matching value of "log_structired" in main.yaml
inventory::submit! {
    LogBackendPlugin {
        name: "example",
        factory: || Box::new(Example),
    }
}
