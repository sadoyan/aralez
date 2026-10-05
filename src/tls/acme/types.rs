use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::LazyLock;

pub struct DnsBackendPlugin {
    pub name: &'static str,
    pub factory: fn() -> Box<dyn DnsProvider>,
}

inventory::collect!(DnsBackendPlugin);

static FACTORIES: LazyLock<HashMap<&'static str, fn() -> Box<dyn DnsProvider>>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for plugin in inventory::iter::<DnsBackendPlugin> {
        map.insert(plugin.name, plugin.factory);
    }
    map
});

#[async_trait]
pub trait DnsProvider: Send + Sync {
    async fn create_txt_record(&self, domain: &str, name: &str, value: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;

    async fn delete_txt_record(&self, record_id: &str, record_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

pub fn get_provider(provider: String) -> Box<dyn DnsProvider> {
    FACTORIES
        .get(provider.as_str())
        .map(|factory| factory())
        .unwrap_or_else(|| panic!("Unknown DNS provider '{}'. Available providers: {:?}", provider, FACTORIES.keys().collect::<Vec<_>>()))
}
