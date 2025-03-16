use crate::utils::tools::*;
use dashmap::DashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

pub async fn hc2(upslist: Arc<UpstreamsDashMap>, fullist: Arc<UpstreamsDashMap>) {
    let mut period = interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            _ = period.tick() => {
                let totest : UpstreamsDashMap = DashMap::new();
                let fclone : UpstreamsDashMap = clone_dashmap(&fullist);
                for val in fclone.iter() {
                    let host = val.key();
                    let inner = DashMap::new();
                    for path_entry in val.value().iter() {
                        // let inner = DashMap::new();
                        let path = path_entry.key();
                        let mut innervec= Vec::new();
                        for k in path_entry.value().0.iter().enumerate() {
                            let (ip, port, ssl, _proto) = k.1;
                            let mut _pref = "";
                            match ssl {
                                true => _pref = "https://",
                                false => _pref = "http://",
                            }
                            let link = format!("{}{}:{}{}", _pref, ip, port, path);
                            let resp = http_request(link.as_str(), "HEAD", "").await;
                            match resp {
                                true => {
                                    innervec.push(k.1.clone());
                                }
                                false => {
                                    println!("Dead Upstream {}, Link: {}",k.0, link);
                                }
                            }
                        }
                        inner.insert(path.clone().to_owned(), (innervec, AtomicUsize::new(0)));
                    }
                    totest.insert(host.clone(), inner);
                }
                if ! compare_dashmaps(&totest, &upslist){
                    print_upstreams(&totest);
                    clone_dashmap_into(&totest, &upslist);
                }
            }
        }
    }
}

#[allow(dead_code)]
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
        "HEAD" => {
            let response = client.head(url).timeout(to).send().await;
            match response {
                Ok(r) => 100 <= r.status().as_u16() && r.status().as_u16() < 500,
                Err(_) => false,
            }
        }
        _ => false,
    }
}
