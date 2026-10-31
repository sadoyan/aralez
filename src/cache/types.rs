use crate::cache::hybrid::{FoyerCache, foy_init};
use crate::cache::redis::RedisCache;
use crate::cache::tikv::TiKVCache;
use crate::utils::lazylock::{CACHE_TTL, EVICTION};
use crate::utils::types::AppConfig;
use log::{error, info, warn};
use pingora_cache::eviction::simple_lru::Manager;
use pingora_cache::{MemCache, Storage};
use std::sync::{Arc, OnceLock};

pub static GLOBAL_STORAGE: OnceLock<Box<dyn Storage + Send + Sync>> = OnceLock::new();

pub async fn init_cache(config: Arc<AppConfig>) {
    if let Some(cache) = &config.cache {
        let storage = cache.storage.clone().unwrap_or_else(|| {
            info!("No cache_storage configured, defaulting to memory");
            "memory".to_string()
        });
        let cache_type = storage.splitn(2, ':').next().unwrap_or("memory");
        let ttl = cache.ttl.unwrap_or(60);

        match cache_type {
            "memory" => {
                CACHE_TTL.set(ttl).ok();
                let size = cache.mem_size_mb.unwrap_or(64);
                let cache_size = size * 1024 * 1024;
                let manager = Manager::new(cache_size);
                let _ = EVICTION.set(manager);
                GLOBAL_STORAGE.set(Box::new(MemCache::new())).ok();
                info!("Cache initialized successfully. Storage: {}", storage);
            }
            "redis" => match RedisCache::new(&storage, ttl).await {
                Ok(cache) => {
                    CACHE_TTL.set(ttl).ok();
                    GLOBAL_STORAGE.set(Box::new(cache)).ok();
                    info!("Cache initialized successfully. Storage: {}", storage);
                }
                Err(e) => log::error!("Cache storage initialization failed: {}", e),
            },
            "tikv" => {
                let stor = storage.replace("tikv://", "");
                let endpoints: Vec<String> = stor.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();

                match TiKVCache::new(endpoints, ttl).await {
                    Ok(cache) => {
                        CACHE_TTL.set(ttl).ok();
                        GLOBAL_STORAGE.set(Box::new(cache)).ok();
                        info!("Cache initialized successfully. Storage: {}", storage);
                    }
                    Err(e) => log::error!("Cache storage initialization failed: {}", e),
                }
            }
            "hybrid" => {
                if let Some(cache) = config.cache.clone() {
                    let mem_size = cache.mem_size_mb.unwrap_or(64);
                    let fs_size = cache.file_size_mb.unwrap_or(512);
                    let flush = cache.flush_interval.unwrap_or(60);
                    let folder = cache.file_path.clone().unwrap_or("/tmp/aralez_cache".to_string());
                    let _ = foy_init(mem_size, fs_size, folder.as_str(), flush).await;
                    CACHE_TTL.set(ttl).ok();
                    match GLOBAL_STORAGE.set(Box::new(FoyerCache::new(ttl))) {
                        Ok(_) => {}
                        Err(_) => error!("CRITICAL: GLOBAL_STORAGE was already set before hybrid init!"),
                    }
                };
            }
            _ => {
                warn!("Unsupported storage, disabling caching: {}", storage);
            }
        }
    } else {
        info!("Cache engine is not configured, disabling.");
    }
}
