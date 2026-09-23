use crate::logging::core::StructuredSystemLog;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::LazyLock;

#[async_trait]
pub trait WriteLog: Send + Sync {
    async fn writelog(&self, msg: &StructuredSystemLog);
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

pub async fn sendlog(backend: &str, msg: &StructuredSystemLog) {
    if let Some(logger) = BACKENDS.get(backend) {
        logger.writelog(msg).await;
    } else {
        log::warn!("Unsupported logging mechanism: {}", backend);
    }
}
