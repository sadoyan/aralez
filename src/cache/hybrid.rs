// This file is written with heavy utilization of AI .
use crate::utils::metrics::{CACHE_DISK_BYTES_FLUSH, CACHE_DISK_DELETED, CACHE_DISK_FILES_FLUSH, CACHE_DISK_WRITE_ERRORS, CACHE_DISK_WRITTEN};
use async_trait::async_trait;
use bytes::Bytes;
use log::{error, info, warn};
use moka::Expiry;
use moka::future::Cache;
use moka::notification::RemovalCause;
use pingora::Result;
use pingora_cache::storage::{HandleHit, HandleMiss, HitHandler, MissFinishType, MissHandler, Storage};
use pingora_cache::trace::SpanHandle;
use pingora_cache::{CacheKey, CacheMeta, PurgeOutcome, PurgeTarget, PurgeType};
use std::any::Any;
use std::borrow::Cow;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{Semaphore, mpsc};

static SPILLED_FILES: AtomicU64 = AtomicU64::new(0);
static SPILLED_BYTES: AtomicU64 = AtomicU64::new(0);
static SPILL_SKIPPED: AtomicU64 = AtomicU64::new(0); // already on disk
static SPILL_FAILED: AtomicU64 = AtomicU64::new(0);

// ---- tuning -------------------------------------------------------------
const SPILL_MIN_BYTES: usize = 1024; // smaller bodies stay RAM-only (saves inodes)
const SPILL_MIN_TTL_SECS: u64 = 30; // don't write entries about to expire
const PROMOTE_MAX: usize = 1024 * 1024; // disk hits above this are served but not copied into RAM
const SPILL_QUEUE: usize = 256; // bounded: when full, spills are dropped, never awaited
const WRITERS: usize = 4; // concurrent disk writes

const MAGIC: &[u8; 8] = b"ARLZDK01";
const HDR: usize = 32; // magic + expires_at + 4 x u32 lengths

type SpillJob = (Arc<str>, Arc<CacheItem>);

pub static MOKA_CACHE: OnceLock<Cache<Arc<str>, Arc<CacheItem>>> = OnceLock::new();
pub static DISK: OnceLock<DiskTier> = OnceLock::new();
static TMP_ID: AtomicU64 = AtomicU64::new(0);

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

// ---- item ---------------------------------------------------------------
pub struct CacheItem {
    pub expires_at: u64, // unix secs: the single source of truth for both tiers
    pub internal_bytes: Bytes,
    pub header_bytes: Bytes,
    pub body_bytes: Bytes,
}

impl CacheItem {
    pub fn new(internal: Bytes, header: Bytes, body: Bytes, ttl_secs: u64) -> Self {
        Self {
            expires_at: unix_now() + ttl_secs,
            internal_bytes: internal,
            header_bytes: header,
            body_bytes: body,
        }
    }
    fn remaining(&self) -> Duration {
        Duration::from_secs(self.expires_at.saturating_sub(unix_now()))
    }
    fn weight(&self) -> u32 {
        (self.internal_bytes.len() + self.header_bytes.len() + self.body_bytes.len() + 64).min(u32::MAX as usize) as u32
    }
}

struct L1Expiry;
impl Expiry<Arc<str>, Arc<CacheItem>> for L1Expiry {
    fn expire_after_create(&self, _k: &Arc<str>, v: &Arc<CacheItem>, _now: Instant) -> Option<Duration> {
        Some(v.remaining())
    }
    fn expire_after_update(&self, _k: &Arc<str>, v: &Arc<CacheItem>, _now: Instant, _d: Option<Duration>) -> Option<Duration> {
        Some(v.remaining())
    }
}

// ---- disk index ---------------------------------------------------------
#[derive(Clone, Copy)]
struct DiskEntry {
    size: u32,
    expires_at: u64,
}

struct DiskExpiry;
impl Expiry<u128, DiskEntry> for DiskExpiry {
    fn expire_after_create(&self, _k: &u128, v: &DiskEntry, _now: Instant) -> Option<Duration> {
        Some(Duration::from_secs(v.expires_at.saturating_sub(unix_now())))
    }
}

fn hash(key: &str) -> u128 {
    xxhash_rust::xxh3::xxh3_128(key.as_bytes()) // frozen spec: stable across restarts
}

fn path_for(dir: &Path, h: u128) -> PathBuf {
    dir.join(format!("{:02x}", (h >> 120) as u8)).join(format!("{h:032x}"))
}

pub struct DiskTier {
    dir: PathBuf,
    index: Cache<u128, DiskEntry>,
    writers: Arc<Semaphore>,
    spill_tx: mpsc::Sender<SpillJob>,
}

impl DiskTier {
    /// Returns the object if it is on disk, intact and unexpired.
    async fn load(&self, key: &str) -> Option<Arc<CacheItem>> {
        let h = hash(key);
        if !self.index.contains_key(&h) {
            return None; // definite miss, no syscall
        }
        let item = tokio::fs::read(path_for(&self.dir, h)).await.ok().and_then(|buf| parse(buf, key));
        match item {
            Some(i) if i.expires_at > unix_now() => Some(Arc::new(i)),
            _ => {
                self.index.invalidate(&h).await; // the eviction listener deletes the file
                None
            }
        }
    }

    /// Drop the disk copy, used when RAM gets a newer version of the object.
    async fn forget(&self, key: &str) {
        let h = hash(key);
        if self.index.contains_key(&h) {
            self.index.invalidate(&h).await;
        }
    }
}

fn write_item(dir: &Path, h: u128, key: &str, item: &CacheItem) -> io::Result<()> {
    let tmp = dir.join("tmp").join(format!("{h:032x}.{}", TMP_ID.fetch_add(1, Ordering::Relaxed)));
    let res = (|| {
        let mut w = BufWriter::with_capacity(64 * 1024, File::create(&tmp)?);
        w.write_all(MAGIC)?;
        w.write_all(&item.expires_at.to_le_bytes())?;
        for l in [key.len(), item.internal_bytes.len(), item.header_bytes.len(), item.body_bytes.len()] {
            w.write_all(&(l as u32).to_le_bytes())?;
        }
        w.write_all(key.as_bytes())?;
        w.write_all(&item.internal_bytes)?;
        w.write_all(&item.header_bytes)?;
        w.write_all(&item.body_bytes)?; // large writes bypass the buffer, no extra copy
        w.flush()
    })()
    .and_then(|_| {
        CACHE_DISK_WRITTEN.inc();
        fs::rename(&tmp, path_for(dir, h))
    });
    if res.is_err() {
        CACHE_DISK_WRITE_ERRORS.inc();
        let _ = fs::remove_file(&tmp);
    }
    res
}

/// One read buffer, sliced without copying. Verifies lengths and the stored key.
fn parse(buf: Vec<u8>, key: &str) -> Option<CacheItem> {
    if buf.len() < HDR || &buf[..8] != MAGIC {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().unwrap()) as usize;
    let expires_at = u64::from_le_bytes(buf[8..16].try_into().unwrap());
    let (kl, il, hl, bl) = (u32_at(16), u32_at(20), u32_at(24), u32_at(28));
    if HDR + kl + il + hl + bl != buf.len() {
        return None; // truncated or corrupt
    }
    let b = Bytes::from(buf);
    if &b[HDR..HDR + kl] != key.as_bytes() {
        return None; // hash collision or corrupt
    }
    let (i0, h0, b0) = (HDR + kl, HDR + kl + il, HDR + kl + il + hl);
    Some(CacheItem {
        expires_at,
        internal_bytes: b.slice(i0..h0),
        header_bytes: b.slice(h0..b0),
        body_bytes: b.slice(b0..),
    })
}

/// Startup: create the directories, delete tmp leftovers and expired or corrupt files, rebuild the index.
fn scan_blocking(dir: &Path) -> io::Result<Vec<(u128, DiskEntry)>> {
    let _ = fs::remove_dir_all(dir.join("tmp"));
    fs::create_dir_all(dir.join("tmp"))?;
    let now = unix_now();
    let mut out = Vec::new();
    for sub in 0..=255u8 {
        let d = dir.join(format!("{sub:02x}"));
        fs::create_dir_all(&d)?;
        for e in fs::read_dir(&d)?.flatten() {
            let p = e.path();
            let entry = (|| {
                let h = u128::from_str_radix(e.file_name().to_str()?, 16).ok()?;
                let mut f = File::open(&p).ok()?;
                let mut hdr = [0u8; HDR];
                f.read_exact(&mut hdr).ok()?;
                if &hdr[..8] != MAGIC {
                    return None;
                }
                let expires_at = u64::from_le_bytes(hdr[8..16].try_into().ok()?);
                if expires_at <= now {
                    return None;
                }
                let size = f.metadata().ok()?.len().min(u32::MAX as u64) as u32;
                Some((h, DiskEntry { size, expires_at }))
            })();
            match entry {
                Some(x) => out.push(x),
                None => {
                    CACHE_DISK_DELETED.inc();
                    let _ = fs::remove_file(&p);
                }
            }
        }
    }
    Ok(out)
}

// ---- init ---------------------------------------------------------------
pub async fn moka_init(mem_mb: usize, disk_mb: usize, dir: PathBuf, flush: u64) -> anyhow::Result<()> {
    // Disk index: weighted by file size, deletes files when entries leave the index.
    let (del_tx, mut del_rx) = mpsc::unbounded_channel::<u128>();
    let index: Cache<u128, DiskEntry> = Cache::builder()
        .max_capacity(disk_mb as u64 * 1024 * 1024)
        .weigher(|_k: &u128, v: &DiskEntry| v.size)
        .expire_after(DiskExpiry)
        .eviction_listener(move |k: Arc<u128>, _v: DiskEntry, cause| {
            if !matches!(cause, RemovalCause::Replaced) {
                let _ = del_tx.send(*k);
            }
        })
        .build();

    let d = dir.clone();
    tokio::spawn(async move {
        while let Some(h) = del_rx.recv().await {
            CACHE_DISK_DELETED.inc();
            let _ = tokio::fs::remove_file(path_for(&d, h)).await;
        }
    });

    let every = Duration::from_secs(flush.max(1));
    tokio::spawn(async move {
        let mut t = tokio::time::interval(every);
        t.tick().await;
        loop {
            t.tick().await;
            persist_l1().await;
        }
    });

    // Rebuild the index from whatever survived the restart.
    let d = dir.clone();
    let found = tokio::task::spawn_blocking(move || scan_blocking(&d)).await??;
    let restored = found.len();
    for (h, e) in found {
        index.insert(h, e).await;
    }

    // Spill pipeline: L1 eviction listener -> bounded queue -> writer tasks.
    let (spill_tx, mut spill_rx) = mpsc::channel::<SpillJob>(SPILL_QUEUE);
    let writers = Arc::new(Semaphore::new(WRITERS));
    {
        let (dir, index, writers) = (dir.clone(), index.clone(), writers.clone());
        tokio::spawn(async move {
            while let Some((key, item)) = spill_rx.recv().await {
                let h = hash(&key);
                if index.contains_key(&h) {
                    SPILL_SKIPPED.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                let Ok(permit) = writers.clone().acquire_owned().await else { break };
                let (dir, index) = (dir.clone(), index.clone());
                tokio::spawn(async move {
                    let size = (HDR + key.len() + item.internal_bytes.len() + item.header_bytes.len() + item.body_bytes.len()).min(u32::MAX as usize) as u32;
                    let expires_at = item.expires_at;
                    let d = dir.clone();
                    match tokio::task::spawn_blocking(move || write_item(&d, h, &key, &item)).await {
                        Ok(Ok(())) => {
                            index.insert(h, DiskEntry { size, expires_at }).await;
                            SPILLED_FILES.fetch_add(1, Ordering::Relaxed);
                            SPILLED_BYTES.fetch_add(size as u64, Ordering::Relaxed);
                        }
                        Ok(Err(e)) => {
                            SPILL_FAILED.fetch_add(1, Ordering::Relaxed);
                            warn!("disk spill failed: {e}");
                        }
                        Err(e) => {
                            SPILL_FAILED.fetch_add(1, Ordering::Relaxed);
                            warn!("disk spill task failed: {e}");
                        }
                    }
                    drop(permit);
                });
            }
        });
    }

    // L1: only entries evicted for SIZE are spilled. Expired, replaced or purged entries are not.
    let tx = spill_tx.clone();
    let l1: Cache<Arc<str>, Arc<CacheItem>> = Cache::builder()
        .max_capacity(mem_mb as u64 * 1024 * 1024)
        .weigher(|_k: &Arc<str>, v: &Arc<CacheItem>| v.weight())
        .expire_after(L1Expiry)
        .eviction_listener(move |k: Arc<Arc<str>>, v: Arc<CacheItem>, cause| {
            if matches!(cause, RemovalCause::Size) && v.body_bytes.len() >= SPILL_MIN_BYTES && v.remaining() > Duration::from_secs(SPILL_MIN_TTL_SECS) {
                let _ = tx.try_send(((*k).clone(), v)); // never block, a full queue just drops the spill
            }
        })
        .build();

    let _ = DISK.set(DiskTier {
        dir: dir.clone(),
        index,
        writers,
        spill_tx,
    });
    let _ = MOKA_CACHE.set(l1);
    info!("Hybrid cache ready: mem {mem_mb}mb, disk {disk_mb}mb at {}, restored {restored} files", dir.display());
    Ok(())
}

pub async fn persist_l1() {
    let (Some(cache), Some(disk)) = (MOKA_CACHE.get(), DISK.get()) else {
        warn!("Cache: cache not initialized, nothing to do");
        return;
    };
    let started = Instant::now();
    let (files0, bytes0) = (SPILLED_FILES.load(Ordering::Relaxed), SPILLED_BYTES.load(Ordering::Relaxed));
    let (skipped0, failed0) = (SPILL_SKIPPED.load(Ordering::Relaxed), SPILL_FAILED.load(Ordering::Relaxed));

    let (mut scanned, mut queued, mut queued_bytes) = (0u64, 0u64, 0u64);
    for (k, v) in cache.iter() {
        scanned += 1;
        if v.body_bytes.len() >= SPILL_MIN_BYTES && v.remaining() > Duration::from_secs(SPILL_MIN_TTL_SECS) {
            let n = v.body_bytes.len() as u64;
            if disk.spill_tx.send(((*k).clone(), v)).await.is_ok() {
                queued += 1;
                queued_bytes += n;
            }
        }
    }
    info!(
        "Cache: scanned {scanned} entries, queued {queued} ({:.1} MB of bodies), waiting for writers",
        queued_bytes as f64 / 1e6
    );

    while disk.spill_tx.capacity() < disk.spill_tx.max_capacity() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let _ = disk.writers.acquire_many(WRITERS as u32).await;

    let files = SPILLED_FILES.load(Ordering::Relaxed) - files0;
    let bytes = SPILLED_BYTES.load(Ordering::Relaxed) - bytes0;
    let skipped = SPILL_SKIPPED.load(Ordering::Relaxed) - skipped0;
    let failed = SPILL_FAILED.load(Ordering::Relaxed) - failed0;
    CACHE_DISK_BYTES_FLUSH.set(bytes as i64);
    CACHE_DISK_FILES_FLUSH.set(files as i64);
    info!(
        "Cache: Flushed {files} entries, {:.1} MB to disk in {:.2?} ({skipped} already on disk, {failed} failed)",
        bytes as f64 / 1e6,
        started.elapsed()
    );
}

// ---- storage ------------------------------------------------------------
#[derive(Clone)]
pub struct MokaCache {
    pub ttl: u64,
    pub max_body: usize,
}

pub struct MokaHitHandler {
    body: Option<Bytes>,
}

pub struct MokaMissHandler {
    key: Arc<str>,
    internal_bytes: Bytes,
    header_bytes: Bytes,
    body_buffer: Vec<u8>,
    ttl: u64,
    max_body: usize,
    overflow: bool,
}

fn key_str(key: &CacheKey) -> Cow<'_, str> {
    match key.primary_key_str() {
        Some(s) => Cow::Borrowed(s),
        // The old code mapped this case to "", which made all such keys share one entry.
        None => Cow::Owned(key.primary_key().iter().map(|b| format!("{b:02x}")).collect()),
    }
}

fn hit(item: &CacheItem) -> Option<(CacheMeta, HitHandler)> {
    match CacheMeta::deserialize(&item.internal_bytes, &item.header_bytes) {
        Ok(meta) => {
            let h: HitHandler = Box::new(MokaHitHandler {
                body: Some(item.body_bytes.clone()),
            });
            Some((meta, h))
        }
        Err(e) => {
            error!("Failed to deserialize cache metadata: {e}");
            None
        }
    }
}

impl MokaCache {
    pub fn new(ttl: u64, max_body: usize) -> Self {
        Self { ttl, max_body }
    }

    /// Remove from both tiers (wire this into `purge` once you know which PurgeTarget you use).
    pub async fn remove(key: &str) {
        if let Some(c) = MOKA_CACHE.get() {
            c.invalidate(key).await;
        }
        if let Some(d) = DISK.get() {
            d.forget(key).await;
        }
    }
}

#[async_trait]
impl HandleHit for MokaHitHandler {
    async fn read_body(&mut self) -> Result<Option<Bytes>> {
        Ok(self.body.take())
    }
    async fn finish(self: Box<Self>, _s: &'static (dyn Storage + Sync), _k: &CacheKey, _t: &SpanHandle) -> Result<()> {
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
impl HandleMiss for MokaMissHandler {
    async fn write_body(&mut self, data: Bytes, _eof: bool) -> Result<()> {
        if self.overflow {
            return Ok(());
        }
        if self.body_buffer.len() + data.len() > self.max_body {
            self.overflow = true; // too big to cache: stop buffering and free the memory
            self.body_buffer = Vec::new();
            return Ok(());
        }
        self.body_buffer.extend_from_slice(&data);
        Ok(())
    }

    async fn finish(self: Box<Self>) -> Result<MissFinishType> {
        let len = self.body_buffer.len();
        if !self.overflow {
            if let Some(cache) = MOKA_CACHE.get() {
                if let Some(d) = DISK.get() {
                    d.forget(&self.key).await; // a fresh body supersedes any older disk copy
                }
                let item = Arc::new(CacheItem::new(self.internal_bytes, self.header_bytes, Bytes::from(self.body_buffer), self.ttl));
                cache.insert(self.key, item).await;
            }
        }
        Ok(MissFinishType::Appended(len, None))
    }

    fn entry_id(&self) -> Option<pingora_cache::eviction::CacheEntryId> {
        None
    }
}

#[async_trait]
impl Storage for MokaCache {
    async fn lookup(&'static self, key: &CacheKey, _trace: &SpanHandle) -> Result<Option<(CacheMeta, HitHandler)>> {
        let Some(cache) = MOKA_CACHE.get() else { return Ok(None) };
        let ks = key_str(key);

        if let Some(item) = cache.get(&*ks).await {
            return Ok(hit(&item));
        }

        // L1 miss: ask the disk tier (the index check makes a true miss cheap).
        let Some(disk) = DISK.get() else { return Ok(None) };
        let Some(item) = disk.load(&ks).await else { return Ok(None) };
        if item.body_bytes.len() <= PROMOTE_MAX {
            cache.insert(Arc::from(&*ks), item.clone()).await; // promote, keeps its original expiry
        }
        Ok(hit(&item))
    }

    async fn get_miss_handler(&'static self, key: &CacheKey, meta: &CacheMeta, _trace: &SpanHandle) -> Result<MissHandler> {
        let (internal, header) = meta.serialize()?;
        Ok(Box::new(MokaMissHandler {
            key: Arc::from(&*key_str(key)),
            internal_bytes: Bytes::from(internal),
            header_bytes: Bytes::from(header),
            body_buffer: Vec::with_capacity(4096),
            ttl: self.ttl, // for freshness-based TTL, compute it from `meta` here
            max_body: self.max_body,
            overflow: false,
        }))
    }

    async fn purge(&'static self, _target: PurgeTarget<'_>, _purge_type: PurgeType, _trace: &SpanHandle) -> Result<PurgeOutcome> {
        Ok(PurgeOutcome::NotFound)
    }

    async fn update_meta(&'static self, key: &CacheKey, meta: &CacheMeta, _trace: &SpanHandle) -> Result<bool> {
        let Some(cache) = MOKA_CACHE.get() else { return Ok(false) };
        let ks = key_str(key);

        let existing = match cache.get(&*ks).await {
            Some(e) => e,
            None => match DISK.get() {
                Some(d) => match d.load(&ks).await {
                    Some(e) => e,
                    None => return Ok(false),
                },
                None => return Ok(false),
            },
        };
        let Ok((internal, header)) = meta.serialize() else { return Ok(false) };

        let updated = Arc::new(CacheItem {
            expires_at: existing.expires_at,
            internal_bytes: Bytes::from(internal),
            header_bytes: Bytes::from(header),
            body_bytes: existing.body_bytes.clone(), // shared, not copied
        });
        if let Some(d) = DISK.get() {
            d.forget(&ks).await; // the disk copy has stale meta, so it will be rewritten on the next spill
        }
        cache.insert(Arc::from(&*ks), updated).await;
        Ok(true)
    }

    fn support_streaming_partial_write(&self) -> bool {
        false
    }

    fn as_any(&self) -> &(dyn Any + Send + Sync) {
        self
    }
}
