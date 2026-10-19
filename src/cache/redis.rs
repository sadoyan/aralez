use async_trait::async_trait;
use bytes::Bytes;
use log::warn;
use pingora::Result;
use pingora_cache::storage::{HandleHit, HandleMiss, HitHandler, MissFinishType, MissHandler, Storage};
use pingora_cache::trace::SpanHandle;
use pingora_cache::{CacheKey, CacheMeta, PurgeOutcome, PurgeTarget, PurgeType};
use redis::aio::ConnectionManager;
use redis::{AsyncCommands, Client, RedisResult};
use std::any::Any;
use std::borrow::Cow;
use std::sync::Arc;

#[derive(Clone)]
pub struct RedisCache {
    pub client: Arc<ConnectionManager>,
    pub ttl: u64,
}

pub struct RedisHitHandler {
    body: Option<Bytes>,
}

pub struct RedisMissHandler {
    client: Arc<ConnectionManager>,
    body_key: String,
    body_buffer: Vec<u8>,
    ttl: u64,
}

impl RedisCache {
    pub async fn new(url: &str, ttl: u64) -> RedisResult<Self> {
        let client = Client::open(url)?;
        let manager = ConnectionManager::new(client).await?;
        Ok(Self { client: Arc::new(manager), ttl })
    }
    fn make_key(key: &CacheKey) -> Cow<'_, str> {
        match key.primary_key_str() {
            Some(s) => Cow::Borrowed(s),
            None => Cow::Owned(key.primary_key().iter().map(|b| format!("{:02x}", b)).collect()),
        }
    }
}

#[async_trait]
impl HandleHit for RedisHitHandler {
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
impl HandleMiss for RedisMissHandler {
    async fn write_body(&mut self, data: Bytes, _eof: bool) -> Result<()> {
        self.body_buffer.extend_from_slice(&data);
        Ok(())
    }

    async fn finish(self: Box<Self>) -> Result<MissFinishType> {
        let len = self.body_buffer.len();
        let mut conn = (*self.client).clone();
        if let Err(e) = conn.set_ex::<_, _, ()>(&self.body_key, &self.body_buffer, self.ttl).await {
            warn!("Failed to write cache entry to Redis: {e}");
        }
        Ok(MissFinishType::Appended(len, None))
    }

    fn entry_id(&self) -> Option<pingora_cache::eviction::CacheEntryId> {
        None
    }
}

#[async_trait]
impl Storage for RedisCache {
    async fn lookup(&'static self, key: &CacheKey, _trace: &SpanHandle) -> Result<Option<(CacheMeta, HitHandler)>> {
        let base = Self::make_key(key);

        // Pre-allocate exact capacities to prevent reallocations
        let mut meta_key = String::with_capacity(base.len() + 5);
        meta_key.push_str(&base);
        meta_key.push_str(":meta");

        let mut body_key = String::with_capacity(base.len() + 5);
        body_key.push_str(&base);
        body_key.push_str(":body");

        let mut internal_key = String::with_capacity(meta_key.len() + 9);
        internal_key.push_str(&meta_key);
        internal_key.push_str(":internal");

        let mut header_key = String::with_capacity(meta_key.len() + 7);
        header_key.push_str(&meta_key);
        header_key.push_str(":header");

        let mut conn = (*self.client).clone();

        let internal: Option<Vec<u8>> = conn.get(&internal_key).await.ok().flatten();
        let header: Option<Vec<u8>> = conn.get(&header_key).await.ok().flatten();
        let body: Option<Vec<u8>> = conn.get(&body_key).await.ok().flatten();

        match (internal, header, body) {
            (Some(internal_bytes), Some(header_bytes), Some(body_bytes)) => match CacheMeta::deserialize(&internal_bytes, &header_bytes) {
                Ok(meta) => {
                    let handler: HitHandler = Box::new(RedisHitHandler {
                        body: Some(Bytes::from(body_bytes)),
                    });
                    Ok(Some((meta, handler)))
                }
                Err(_) => {
                    let _: RedisResult<()> = conn.del(&[&internal_key, &header_key, &body_key]).await;
                    Ok(None)
                }
            },
            _ => Ok(None),
        }
    }

    async fn get_miss_handler(&'static self, key: &CacheKey, meta: &CacheMeta, _trace: &SpanHandle) -> Result<MissHandler> {
        let base = Self::make_key(key);

        let mut meta_key = String::with_capacity(base.len() + 5);
        meta_key.push_str(&base);
        meta_key.push_str(":meta");

        let mut body_key = String::with_capacity(base.len() + 5);
        body_key.push_str(&base);
        body_key.push_str(":body");

        let mut internal_key = String::with_capacity(meta_key.len() + 9);
        internal_key.push_str(&meta_key);
        internal_key.push_str(":internal");

        let mut header_key = String::with_capacity(meta_key.len() + 7);
        header_key.push_str(&meta_key);
        header_key.push_str(":header");

        let ttl = self.ttl;
        let (internal_bytes, header_bytes) = meta.serialize()?;
        let mut conn = (*self.client).clone();

        let _: RedisResult<()> = conn.set_ex(&internal_key, &internal_bytes, ttl).await;
        let _: RedisResult<()> = conn.set_ex(&header_key, &header_bytes, ttl).await;

        Ok(Box::new(RedisMissHandler {
            client: self.client.clone(),
            body_key,
            body_buffer: Vec::with_capacity(4096),
            ttl,
        }))
    }

    async fn purge(&'static self, _target: PurgeTarget<'_>, _purge_type: PurgeType, _trace: &SpanHandle) -> Result<PurgeOutcome> {
        Ok(PurgeOutcome::NotFound)
    }

    async fn update_meta(&'static self, key: &CacheKey, meta: &CacheMeta, _trace: &SpanHandle) -> Result<bool> {
        let base = Self::make_key(key);

        let mut meta_key = String::with_capacity(base.len() + 5);
        meta_key.push_str(&base);
        meta_key.push_str(":meta");

        let mut internal_key = String::with_capacity(meta_key.len() + 9);
        internal_key.push_str(&meta_key);
        internal_key.push_str(":internal");

        let mut header_key = String::with_capacity(meta_key.len() + 7);
        header_key.push_str(&meta_key);
        header_key.push_str(":header");

        let ttl = self.ttl;
        match meta.serialize() {
            Ok((internal_bytes, header_bytes)) => {
                let mut conn = (*self.client).clone();
                let _: RedisResult<()> = conn.set_ex(&internal_key, &internal_bytes, ttl).await;
                let _: RedisResult<()> = conn.set_ex(&header_key, &header_bytes, ttl).await;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    fn support_streaming_partial_write(&self) -> bool {
        false
    }

    fn as_any(&self) -> &(dyn Any + Send + Sync) {
        self
    }
}
