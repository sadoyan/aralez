use crate::logging::core::StructuredSystemLog;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::LazyLock;
use tokio::sync::mpsc;

#[async_trait]
pub trait WriteLog: Send + Sync {
    async fn run(&self, rx: mpsc::Receiver<StructuredSystemLog>);
}

pub struct LogBackendPlugin {
    pub name: &'static str,
    pub factory: fn() -> Box<dyn WriteLog>,
}

inventory::collect!(LogBackendPlugin);

static BACKENDS: LazyLock<HashMap<&'static str, Box<dyn WriteLog>>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for plugin in inventory::iter::<LogBackendPlugin> {
        map.insert(plugin.name, (plugin.factory)());
    }
    map
});

pub fn start_logging_backend(backend: &str, rx: mpsc::Receiver<StructuredSystemLog>) {
    if let Some(logger) = BACKENDS.get(backend) {
        let logger_ref = logger.as_ref();
        tokio::spawn(async move {
            logger_ref.run(rx).await;
        });
    } else {
        log::warn!("Unsupported logging mechanism: {}", backend);
    }
}
