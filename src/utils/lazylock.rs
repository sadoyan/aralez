use dashmap::DashMap;
use moka::sync::Cache;
use pingora_limits::rate::Rate;
use std::net::IpAddr;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

pub static REVERSE_STORE: LazyLock<DashMap<String, String>> = LazyLock::new(DashMap::new);
pub static RATE_LIMITER: LazyLock<Rate> = LazyLock::new(|| Rate::new(Duration::from_secs(1)));
pub static REQUESTS_4XX: LazyLock<Cache<IpAddr, u32>> = LazyLock::new(|| Cache::builder().time_to_live(Duration::from_secs(1)).build());
pub static LOCALHOST: LazyLock<Arc<str>> = LazyLock::new(|| Arc::from("localhost"));
