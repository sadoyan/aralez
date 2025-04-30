use crate::web::proxyhttp::LB;
use async_trait::async_trait;
use std::sync::atomic::Ordering;

#[async_trait]
pub trait GetHost {
    fn get_host(&self, peer: &str, path: &str, backend_id: Option<&str>) -> Option<(String, u16, bool)>;
    fn get_header(&self, peer: &str, path: &str) -> Option<Vec<(String, String)>>;
}
#[async_trait]
impl GetHost for LB {
    fn get_host(&self, peer: &str, path: &str, backend_id: Option<&str>) -> Option<(String, u16, bool)> {
        if let Some(b) = backend_id {
            if let Some(bb) = self.ump_byid.get(b) {
                // println!("BIB :===> {:?}", Some(bb.value()));
                return Some(bb.value().clone());
            }
        }

        let host_entry = self.ump_upst.get(peer)?;
        let mut current_path = path.to_string();
        let mut best_match: Option<(String, u16, bool)> = None;
        loop {
            if let Some(entry) = host_entry.get(&current_path) {
                let (servers, index) = entry.value();
                if !servers.is_empty() {
                    let idx = index.fetch_add(1, Ordering::Relaxed) % servers.len();
                    best_match = Some(servers[idx].clone());
                    break;
                }
            }
            if let Some(pos) = current_path.rfind('/') {
                current_path.truncate(pos);
            } else {
                break;
            }
        }
        if best_match.is_none() {
            if let Some(entry) = host_entry.get("/") {
                let (servers, index) = entry.value();
                if !servers.is_empty() {
                    let idx = index.fetch_add(1, Ordering::Relaxed) % servers.len();
                    best_match = Some(servers[idx].clone());
                }
            }
        }
        // println!("BMT :===> {:?}", best_match);
        best_match
    }
    fn get_header(&self, peer: &str, path: &str) -> Option<Vec<(String, String)>> {
        let host_entry = self.headers.get(peer)?;
        let mut current_path = path.to_string();
        let mut best_match: Option<Vec<(String, String)>> = None;

        loop {
            if let Some(entry) = host_entry.get(&current_path) {
                if !entry.value().is_empty() {
                    best_match = Some(entry.value().clone());
                    break;
                }
            }
            if let Some(pos) = current_path.rfind('/') {
                current_path.truncate(pos);
            } else {
                break;
            }
        }
        if best_match.is_none() {
            if let Some(entry) = host_entry.get("/") {
                if !entry.value().is_empty() {
                    best_match = Some(entry.value().clone());
                }
            }
        }
        best_match
    }
}
