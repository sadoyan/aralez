use crate::utils::tools::*;
use dashmap::DashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

pub async fn hc(upslist: Arc<UpstreamMap>, fullist: Arc<UpstreamMap>) {
    let mut period = interval(Duration::from_secs(2));

    loop {
        tokio::select! {
            _ = period.tick() => {
                // let before = Instant::now();
                let totest: UpstreamMap = DashMap::new();
                let fclone: UpstreamMap = DashMap::new();
                // println!("\nElapsed dash: {:.2?}", before.elapsed());
                // let before = Instant::now();
                {
                    for v in fullist.iter() {
                        fclone.insert(v.key().clone(), (v.value().0.clone(), AtomicUsize::new(0)));
                    }
                } // lock releases when scope ends
                // println!("Elapsed full: {:.2?}", before.elapsed());
                for val in fclone.iter() {
                    let mut newvec = vec![];
                    for hostport in val.value().0.clone(){
                        let hostpart = hostport.0.split('/').last().unwrap(); // For later use
                        let url = format!("http://{}:{}", hostpart, hostport.1);
                        let resp = http_request(url.as_str(), "GET", "").await;
                        match resp{
                            true => {
                                newvec.push((hostpart.to_string(), hostport.1));
                            },
                            false => {
                                println!("Dead upstream. Host: {}, Upstream: {}:{} ",val.key(), hostpart.to_string(), hostport.1 );
                            }
                        }
                    }
                    totest.insert(val.key().clone(), (newvec, AtomicUsize::new(0)));
                }
                // let before = Instant::now();
                {
                    if !crate::utils::compare::dm(&upslist, &totest) {
                        println!("Dashmaps not matched, synchronizing");
                        upslist.clear();
                        for (k, v) in totest { // loop takes the ownership
                            println!("Host: {}", k);
                            for vv in &v.0 {
                                println!("   :===> {:?}", vv);
                            }
                            upslist.insert(k, v);
                        }
                    }
                }
                // println!("Elapsed upsl: {:.2?}", before.elapsed());
            }
        }
    }
}

async fn http_request(url: &str, method: &str, payload: &str) -> bool {
    let client = reqwest::Client::new();
    let to = Duration::from_secs(1);
    match method {
        "POST" => {
            let response = client.post(url).body(payload.to_owned()).timeout(to).send().await;
            match response {
                Ok(r) => 100 <= r.status().as_u16() && r.status().as_u16() < 500,
                Err(_) => false,
            }
        }
        "GET" => {
            let response = client.get(url).timeout(to).send().await;
            match response {
                Ok(r) => {
                    // println!("Response: {} : {}", r.status(), r.url());
                    100 <= r.status().as_u16() && r.status().as_u16() < 500
                }
                Err(_) => {
                    // println!("Error: {}", url);
                    false
                }
            }
        }
        _ => false,
    }
}
