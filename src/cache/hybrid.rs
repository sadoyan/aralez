use async_trait::async_trait;
use bytes::Bytes;
use foyer::{BlockEngineConfig, DeviceBuilder, FsDeviceBuilder, HybridCache, HybridCacheBuilder, HybridCachePolicy, PsyncIoEngineConfig, RecoverMode};
use log::info;
use pingora::Result;
use pingora_cache::storage::{HandleHit, HandleMiss, HitHandler, MissFinishType, MissHandler, Storage};
use pingora_cache::trace::SpanHandle;
use pingora_cache::{CacheKey, CacheMeta, PurgeOutcome, PurgeTarget, PurgeType};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub static HYBRID_CACHE: OnceLock<HybridCache<u64, CacheItem>> = OnceLock::new();

#[derive(Serialize, Deserialize, Clone)]
pub struct CacheItem {
    pub expires_at: u64,
    pub internal_bytes: Vec<u8>,
    pub header_bytes: Vec<u8>,
    pub body_bytes: Vec<u8>,
}

impl CacheItem {
    pub fn new(internal_bytes: Vec<u8>, header_bytes: Vec<u8>, body_bytes: Vec<u8>, ttl_seconds: u64) -> Self {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        Self {
            expires_at: now + ttl_seconds,
            internal_bytes,
            header_bytes,
            body_bytes,
        }
    }

    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        now > self.expires_at
    }
}

pub async fn foy_init(m: usize, f: usize, dir: &str, flush: u64) -> anyhow::Result<()> {
    let memsize = m * 1024 * 1024;
    let filesize = f * 1024 * 1024;

    let device = FsDeviceBuilder::new(dir).with_capacity(filesize).build()?;
    let cache: HybridCache<u64, CacheItem> = HybridCacheBuilder::new()
        .with_policy(HybridCachePolicy::WriteOnEviction)
        .memory(memsize)
        .storage()
        .with_io_engine_config(PsyncIoEngineConfig::new())
        .with_engine_config(BlockEngineConfig::new(device))
        .with_recover_mode(RecoverMode::Strict)
        .build()
        .await?;

    let _ = HYBRID_CACHE.set(cache);

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(flush));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Some(cache) = HYBRID_CACHE.get() {
                        info!("Flushing {} cache entries to disk", cache.memory().entries());
                        let _ = cache.flush_if(|_, _| true).await;
                    }
                }
            }
        }
    });

    info!("Hybrid cache initialized with memory: {}mb, path: {}, size: {}mb", m, dir, f);
    Ok(())
}

#[derive(Clone)]
pub struct FoyerCache {
    pub ttl: u64,
}

pub struct FoyerHitHandler {
    body: Option<Bytes>,
}

pub struct FoyerMissHandler {
    key: u64,
    internal_bytes: Vec<u8>,
    header_bytes: Vec<u8>,
    body_buffer: Vec<u8>,
    ttl: u64,
}

impl FoyerCache {
    pub fn new(ttl: u64) -> Self {
        Self { ttl }
    }

    fn hash_key(key: &CacheKey) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();

        if let Some(s) = key.primary_key_str() {
            s.hash(&mut hasher);
        } else {
            key.primary_key().hash(&mut hasher);
        }
        hasher.finish()
    }
}

#[async_trait]
impl HandleHit for FoyerHitHandler {
    async fn read_body(&mut self) -> Result<Option<Bytes>> {
        Ok(self.body.take())
    }

    async fn finish(self: Box<Self>, _storage: &'static (dyn Storage + Sync), _key: &CacheKey, _trace: &SpanHandle) -> Result<()> {
        Ok(())
    }

    fn can_seek(&self) -> bool {
        false
    }

    fn seek(&mut self, _start: usize, _end: Option<usize>) -> Result<()> {
        Ok(())
    }

    fn should_count_access(&self) -> bool {
        true
    }

    fn get_eviction_weight(&self) -> usize {
        self.body.as_ref().map(|b| b.len()).unwrap_or(0)
    }

    fn entry_id(&self) -> Option<pingora_cache::eviction::CacheEntryId> {
        None
    }

    fn as_any(&self) -> &(dyn Any + Send + Sync) {
        self
    }

    fn as_any_mut(&mut self) -> &mut (dyn Any + Send + Sync) {
        self
    }
}

#[async_trait]
impl HandleMiss for FoyerMissHandler {
    async fn write_body(&mut self, data: Bytes, _eof: bool) -> Result<()> {
        self.body_buffer.extend_from_slice(&data);
        Ok(())
    }

    async fn finish(self: Box<Self>) -> Result<MissFinishType> {
        let len = self.body_buffer.len();
        let item = CacheItem::new(self.internal_bytes, self.header_bytes, self.body_buffer, self.ttl);
        if let Some(cache) = HYBRID_CACHE.get() {
            cache.insert(self.key, item);
        }
        Ok(MissFinishType::Appended(len, None))
    }

    fn entry_id(&self) -> Option<pingora_cache::eviction::CacheEntryId> {
        None
    }
}

#[async_trait]
impl Storage for FoyerCache {
    async fn lookup(&'static self, key: &CacheKey, _trace: &SpanHandle) -> Result<Option<(CacheMeta, HitHandler)>> {
        let k = Self::hash_key(key);

        if let Some(cache) = HYBRID_CACHE.get() {
            if let Ok(Some(entry)) = cache.get(&k).await {
                let item = entry.value();
                if item.is_expired() {
                    return Ok(None);
                }
                match CacheMeta::deserialize(&item.internal_bytes, &item.header_bytes) {
                    Ok(meta) => {
                        let handler: HitHandler = Box::new(FoyerHitHandler {
                            body: Some(Bytes::from(item.body_bytes.clone())),
                        });
                        return Ok(Some((meta, handler)));
                    }
                    Err(_) => {
                        return Ok(None);
                    }
                }
            }
        }
        Ok(None)
    }

    async fn get_miss_handler(&'static self, key: &CacheKey, meta: &CacheMeta, _trace: &SpanHandle) -> Result<MissHandler> {
        let k = Self::hash_key(key);
        let (internal_bytes, header_bytes) = meta.serialize()?;

        Ok(Box::new(FoyerMissHandler {
            key: k,
            internal_bytes,
            header_bytes,
            body_buffer: Vec::with_capacity(4096),
            ttl: self.ttl,
        }))
    }

    async fn purge(&'static self, _target: PurgeTarget<'_>, _purge_type: PurgeType, _trace: &SpanHandle) -> Result<PurgeOutcome> {
        Ok(PurgeOutcome::NotFound)
    }

    async fn update_meta(&'static self, key: &CacheKey, meta: &CacheMeta, _trace: &SpanHandle) -> Result<bool> {
        let k = Self::hash_key(key);
        if let Some(cache) = HYBRID_CACHE.get() {
            // Fix: Match the Result containing an Option just like lookup
            if let Ok(Some(entry)) = cache.get(&k).await {
                let existing = entry.value().clone();
                if let Ok((internal_bytes, header_bytes)) = meta.serialize() {
                    let updated = CacheItem {
                        expires_at: existing.expires_at,
                        internal_bytes,
                        header_bytes,
                        body_bytes: existing.body_bytes,
                    };
                    cache.insert(k, updated);
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn support_streaming_partial_write(&self) -> bool {
        false
    }

    fn as_any(&self) -> &(dyn Any + Send + Sync) {
        self
    }
}
