use crate::cache::redis::RedisCache;
use crate::utils::lazylock::{CACHE_TTL, EVICTION};
use crate::utils::types::AppConfig;
use log::{info, warn};
use pingora_cache::eviction::simple_lru::Manager;
use pingora_cache::{MemCache, Storage};
use std::sync::{Arc, OnceLock};

pub static GLOBAL_STORAGE: OnceLock<Box<dyn Storage + Send + Sync>> = OnceLock::new();

pub async fn init_cache(config: Arc<AppConfig>) {
    let cache_storage = config.cache_storage.clone().unwrap_or_else(|| {
        info!("No cache_storage configured, defaulting to memory");
        "memory".to_string()
    });
    let cache_type = cache_storage.splitn(2, ':').next().unwrap_or("memory");
    let ttl = config.cache_ttl.unwrap_or(60);

    match cache_type {
        "memory" => {
            CACHE_TTL.set(ttl).ok();
            if let Some(size) = config.cache_size_mb {
                let cache_size = size * 1024 * 1024;
                drop(EVICTION.set(Manager::new(cache_size)));
            }
            GLOBAL_STORAGE.set(Box::new(MemCache::new())).ok();
            info!("Cache initialized successfully. Storage: {}", cache_storage);
        }
        "redis" => match RedisCache::new(&cache_storage, ttl).await {
            Ok(cache) => {
                CACHE_TTL.set(ttl).ok();
                GLOBAL_STORAGE.set(Box::new(cache)).ok();
                info!("Cache initialized successfully. Storage: {}", cache_storage);
            }
            Err(e) => log::error!("Cache storage initialization failed: {}", e),
        },
        _ => {
            warn!("Unsupported storage, disabling caching: {}", cache_storage);
        }
    }
}
