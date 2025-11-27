use crate::utils::structs::InnerMap;
use crate::web::proxyhttp::LB;
use async_trait::async_trait;
use std::sync::atomic::Ordering;

#[derive(Debug, Clone)]
pub struct GetHostsReturHeaders {
    pub client_headers: Option<Vec<(String, String)>>,
    pub server_headers: Option<Vec<(String, String)>>,
}

#[async_trait]
pub trait GetHost {
    // fn get_host<'a>(&self, peer: &str, path: &str, backend_id: Option<&str>) -> Option<&'a InnerMap>;

    fn get_host(&self, peer: &str, path: &str, backend_id: Option<&str>) -> Option<InnerMap>;

    fn get_header(&self, peer: &str, path: &str) -> Option<GetHostsReturHeaders>;
}
#[async_trait]
impl GetHost for LB {
    fn get_host(&self, peer: &str, path: &str, backend_id: Option<&str>) -> Option<InnerMap> {
        if let Some(b) = backend_id {
            if let Some(bb) = self.ump_byid.get(b) {
                return Some(bb.value().clone());
            }
        }
        let host_entry = self.ump_upst.get(peer)?;
        let mut end = path.len();
        loop {
            let slice = &path[..end];
            if let Some(entry) = host_entry.get(slice) {
                let (servers, index) = entry.value();
                if !servers.is_empty() {
                    let idx = index.fetch_add(1, Ordering::Relaxed) % servers.len();
                    return Some(servers[idx].clone());
                }
            }
            if let Some(pos) = slice.rfind('/') {
                end = pos;
            } else {
                break;
            }
        }
        if let Some(entry) = host_entry.get("/") {
            let (servers, index) = entry.value();
            if !servers.is_empty() {
                let idx = index.fetch_add(1, Ordering::Relaxed) % servers.len();
                return Some(servers[idx].clone());
            }
        }
        None
    }

    fn get_header(&self, peer: &str, path: &str) -> Option<GetHostsReturHeaders> {
        let client_entry = self.client_headers.get(peer)?;
        let server_entry = self.server_headers.get(peer)?;
        let mut current_path = path;
        let mut clnt_match = None;
        loop {
            if let Some(entry) = client_entry.get(current_path) {
                if !entry.value().is_empty() {
                    clnt_match = Some(entry.value().clone());
                    break;
                }
            }
            if let Some(pos) = current_path.rfind('/') {
                current_path = if pos == 0 { "/" } else { &current_path[..pos] };
            } else {
                break;
            }
        }
        current_path = path;
        let mut serv_match = None;
        loop {
            if let Some(entry) = server_entry.get(current_path) {
                if !entry.value().is_empty() {
                    serv_match = Some(entry.value().clone());
                    break;
                }
            }
            if let Some(pos) = current_path.rfind('/') {
                current_path = if pos == 0 { "/" } else { &current_path[..pos] };
            } else {
                break;
            }
            if serv_match.is_none() {
                if let Some(entry) = server_entry.get("/") {
                    if !entry.value().is_empty() {
                        serv_match = Some(entry.value().clone());
                        break;
                    }
                }
            }
        }
        Some(GetHostsReturHeaders {
            client_headers: clnt_match,
            server_headers: serv_match,
        })
    }
}
