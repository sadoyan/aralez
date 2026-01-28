use crate::utils::structs::InnerMap;
use crate::web::proxyhttp::LB;
use async_trait::async_trait;
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetHostsReturHeaders {
    pub client_headers: Option<Vec<(Arc<str>, Arc<str>)>>,
    pub server_headers: Option<Vec<(Arc<str>, Arc<str>)>>,
}

#[async_trait]
pub trait GetHost {
    fn get_host(&self, peer: &str, path: &str, backend_id: Option<&str>) -> Option<Arc<InnerMap>>;

    fn get_header(&self, peer: &str, path: &str) -> Option<GetHostsReturHeaders>;
    // fn get_upstreams(&self) -> Arc<UpstreamsDashMap>;
}
#[async_trait]
impl GetHost for LB {
    // fn get_upstreams(&self) -> Arc<UpstreamsDashMap> {
    //     self.ump_full.clone()
    // }
    fn get_host(&self, peer: &str, path: &str, backend_id: Option<&str>) -> Option<Arc<InnerMap>> {
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
        let client_entry = self.client_headers.get(peer);
        let server_entry = self.server_headers.get(peer);
        if client_entry.is_none() && server_entry.is_none() {
            return None;
        }
        let mut current_path = path;
        let mut clnt_match = None;
        if let Some(client_entry) = client_entry {
            loop {
                if let Some(entry) = client_entry.get(current_path) {
                    if !entry.value().is_empty() {
                        clnt_match = Some(entry.value().clone());
                        break;
                    }
                }
                if current_path == "/" {
                    break;
                }
                if let Some(pos) = current_path.rfind('/') {
                    current_path = if pos == 0 { "/" } else { &current_path[..pos] };
                } else {
                    break;
                }
            }
        }
        current_path = path;
        let mut serv_match = None;
        if let Some(server_entry) = server_entry {
            loop {
                if let Some(entry) = server_entry.get(current_path) {
                    if !entry.value().is_empty() {
                        serv_match = Some(entry.value().clone());
                        break;
                    }
                }
                if current_path == "/" {
                    if let Some(entry) = server_entry.get("/") {
                        if !entry.value().is_empty() {
                            serv_match = Some(entry.value().clone());
                            break;
                        }
                    }
                    break;
                }
                if let Some(pos) = current_path.rfind('/') {
                    current_path = if pos == 0 { "/" } else { &current_path[..pos] };
                } else {
                    break;
                }
            }
        }
        let result = GetHostsReturHeaders {
            client_headers: clnt_match,
            server_headers: serv_match,
        };

        if result.client_headers.is_some() || result.server_headers.is_some() {
            Some(result)
        } else {
            None
        }
    }
}
