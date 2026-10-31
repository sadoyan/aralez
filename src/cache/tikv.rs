use async_trait::async_trait;
use bytes::Bytes;
use log::warn;
use pingora_cache::storage::{HandleHit, HandleMiss, MissFinishType};
use pingora_cache::trace::SpanHandle;
use pingora_cache::{CacheKey, CacheMeta, HitHandler, MissHandler, PurgeOutcome, PurgeTarget, PurgeType, Storage};
use std::any::Any;
use std::borrow::Cow;
use tikv_client::{Error as TiKVError, RawClient};

#[derive(Clone)]
pub struct TiKVCache {
    pub client: RawClient,
    pub ttl: u64,
}

pub struct TiKVHitHandler {
    body: Option<Bytes>,
}
pub struct TiKVMissHandler {
    client: RawClient,
    body_key: String,
    body_buffer: Vec<u8>,
    ttl: u64,
}
impl TiKVCache {
    pub async fn new<S: Into<String>>(endpoints: Vec<S>, ttl: u64) -> Result<Self, TiKVError> {
        let pd_endpoints: Vec<String> = endpoints.into_iter().map(Into::into).collect();
        let client = RawClient::new(pd_endpoints).await?;

        Ok(Self { client, ttl })
    }

    pub fn make_key(key: &CacheKey) -> Cow<'_, str> {
        match key.primary_key_str() {
            Some(s) => Cow::Borrowed(s),
            None => Cow::Owned(key.primary_key().iter().map(|b| format!("{:02x}", b)).collect()),
        }
    }
}

#[async_trait]
impl HandleHit for TiKVHitHandler {
    async fn read_body(&mut self) -> pingora::Result<Option<Bytes>> {
        Ok(self.body.take())
    }

    async fn finish(self: Box<Self>, _storage: &'static (dyn Storage + Sync), _key: &CacheKey, _trace: &SpanHandle) -> pingora::Result<()> {
        Ok(())
    }

    fn can_seek(&self) -> bool {
        false
    }

    fn seek(&mut self, _start: usize, _end: Option<usize>) -> pingora::Result<()> {
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
impl HandleMiss for TiKVMissHandler {
    async fn write_body(&mut self, data: Bytes, _eof: bool) -> pingora::Result<()> {
        self.body_buffer.extend_from_slice(&data);
        Ok(())
    }

    async fn finish(self: Box<Self>) -> pingora::Result<MissFinishType> {
        let len = self.body_buffer.len();
        if let Err(e) = self.client.put_with_ttl(self.body_key, self.body_buffer, self.ttl).await {
            warn!("Failed to write cache entry to TiKV: {e}");
        }
        Ok(MissFinishType::Appended(len, None))
    }

    fn entry_id(&self) -> Option<pingora_cache::eviction::CacheEntryId> {
        None
    }
}

#[async_trait]
impl Storage for TiKVCache {
    async fn lookup(&'static self, key: &CacheKey, _trace: &SpanHandle) -> pingora::Result<Option<(CacheMeta, HitHandler)>> {
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

        let conn = (&self.client).clone();

        // Perform a single batch_get round-trip for all 3 keys.
        // We pass clones into batch_get, leaving original keys owned for potential cleanup.
        let keys_to_fetch = vec![internal_key.clone(), header_key.clone(), body_key.clone()];
        let results = match conn.batch_get(keys_to_fetch.clone()).await {
            Ok(res) => res,
            Err(_) => return Ok(None),
        };

        // Pre-convert expected keys to tikv_client::Key for matching
        let expected_internal: tikv_client::Key = internal_key.clone().into();
        let expected_header: tikv_client::Key = header_key.clone().into();
        let expected_body: tikv_client::Key = body_key.clone().into();

        let mut internal_bytes = None;
        let mut header_bytes = None;
        let mut body_bytes = None;

        for kv in results {
            let k = kv.key();
            if k == &expected_internal {
                internal_bytes = Some(kv.into_value());
            } else if k == &expected_header {
                header_bytes = Some(kv.into_value());
            } else if k == &expected_body {
                body_bytes = Some(kv.into_value());
            }
        }

        match (internal_bytes, header_bytes, body_bytes) {
            (Some(internal_vec), Some(header_vec), Some(body_vec)) => {
                match CacheMeta::deserialize(&internal_vec, &header_vec) {
                    Ok(meta) => {
                        let handler: HitHandler = Box::new(TiKVHitHandler {
                            body: Some(Bytes::from(body_vec)),
                        });
                        Ok(Some((meta, handler)))
                    }
                    Err(_) => {
                        // Original keys are still owned here, no extra clones needed for batch_delete!
                        let _ = conn.batch_delete(vec![internal_key, header_key, body_key]).await;
                        Ok(None)
                    }
                }
            }
            _ => Ok(None),
        }
    }

    async fn get_miss_handler(&'static self, key: &CacheKey, meta: &CacheMeta, _trace: &SpanHandle) -> pingora::Result<MissHandler> {
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
        let conn = (&self.client).clone();

        if let Err(e) = conn.put_with_ttl(internal_key, internal_bytes, ttl).await {
            log::error!("Failed to cache body to TiKV: {}", e);
        }

        if let Err(e) = conn.put_with_ttl(header_key, header_bytes, ttl).await {
            log::error!("Failed to cache headers to TiKV: {}", e);
        }
        Ok(Box::new(TiKVMissHandler {
            client: self.client.clone(),
            body_key,
            body_buffer: Vec::with_capacity(4096),
            ttl,
        }))
    }

    async fn purge(&'static self, _target: PurgeTarget<'_>, _purge_type: PurgeType, _trace: &SpanHandle) -> pingora::Result<PurgeOutcome> {
        Ok(PurgeOutcome::NotFound)
    }

    async fn update_meta(&'static self, key: &CacheKey, meta: &CacheMeta, _trace: &SpanHandle) -> pingora::Result<bool> {
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
                let conn = (&self.client).clone();
                let _ = conn.put_with_ttl(internal_key, internal_bytes, ttl).await;
                let _ = conn.put_with_ttl(header_key, header_bytes, ttl).await;
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
