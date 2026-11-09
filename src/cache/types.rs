use crate::cache::hybrid::{MokaCache, moka_init};
use crate::cache::redis::RedisCache;
use crate::cache::tikv::TiKVCache;
use crate::utils::lazylock::{CACHE_TTL, EVICTION};
use crate::utils::types::AppConfig;
use log::{error, info, warn};
use pingora_cache::eviction::simple_lru::Manager;
use pingora_cache::{MemCache, Storage};
use std::path::PathBuf;
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

                info!("Cache initialized successfully. Storage: {}, Size: {}mb", storage, size);
            }

            "redis" => match RedisCache::new(&storage, ttl).await {
                Ok(cache) => {
                    CACHE_TTL.set(ttl).ok();
                    GLOBAL_STORAGE.set(Box::new(cache)).ok();

                    info!("Cache initialized successfully. Storage: {}", storage);
                }
                Err(e) => error!("Cache storage initialization failed: {}", e),
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
                    Err(e) => error!("Cache storage initialization failed: {}", e),
                }
            }

            "hybrid" => {
                let mem_size = cache.mem_size_mb.unwrap_or(64);
                let fs_size = cache.file_size_mb.unwrap_or(512);
                let folder: PathBuf = cache.file_path.as_ref().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp/aralez_cache"));
                let max_body = cache.item_max_size.unwrap_or(5) * 1024 * 1024;
                let flush = cache.flush_interval.unwrap_or(60);

                if let Err(e) = moka_init(mem_size, fs_size, folder.clone(), flush).await {
                    error!("CRITICAL: cache init failed for {}: {e}", folder.display());
                    return;
                }
                match GLOBAL_STORAGE.set(Box::new(MokaCache::new(ttl, max_body))) {
                    Ok(_) => info!("Cache initialized. Storage: {}, mem: {}mb, disk: {}mb at {}", storage, mem_size, fs_size, folder.display(),),
                    Err(_) => error!("CRITICAL: GLOBAL_STORAGE already initialized"),
                }
            }
            _ => {
                warn!("Unsupported storage, disabling caching: {}", storage);
            }
        }
    } else {
        info!("Cache engine is not configured, disabling.");
    }
}
