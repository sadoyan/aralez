use crate::logging::core::StructuredSystemLog;
use std::collections::HashMap;
use std::sync::LazyLock;

pub trait WriteLog: Send + Sync {
    fn writelog(&self, msg: &StructuredSystemLog);
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

pub fn sendlog(backend: &str, msg: &StructuredSystemLog) {
    if let Some(logger) = BACKENDS.get(backend) {
        logger.writelog(msg);
    } else {
        log::warn!("Unsupported logging mechanism: {}", backend);
    }
}
