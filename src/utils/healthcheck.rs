use dashmap::DashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::interval;

pub async fn hc(upslist: Arc<RwLock<DashMap<String, (Vec<(String, u16)>, AtomicUsize)>>>, fullist: Arc<RwLock<DashMap<String, (Vec<(String, u16)>, AtomicUsize)>>>) {
    let mut period = interval(Duration::from_secs(20));
    loop {
        tokio::select! {
            _ = period.tick() => {
                let ups = upslist.write().await;
                let full = fullist.write().await;
                for val in full.iter_mut() {
                    // making some dummy ligic
                    match val.key().to_string().as_str() {
                       "polo.netangels.net" => ups.remove("polo.netangels.net"),
                       "glop.netangels.net" => ups.remove("glop.netangels.net"),
                        _ => ups.remove(""),
                    };
                    // println!("Iter full: {} -> {:?}", val.key(), val.value());
                }

                println!("UPS: {:?}", ups);
                drop(ups);
                drop(full);
            }
        }
    }
}
