use crate::core::proxyhttp::LB;
use crate::utils::types::InnerMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone)]
pub struct GetHostsReturHeaders {
    pub client_headers: Option<Vec<(String, Arc<str>)>>,
    pub server_headers: Option<Vec<(String, Arc<str>)>>,
}

pub trait GetHost {
    fn find_sticky_backend(&self, servers: &[Arc<InnerMap>], backend_id: Option<&str>) -> Option<Arc<InnerMap>>;
    fn pick_backend(&self, servers: &[Arc<InnerMap>], index: &AtomicUsize, backend_id: Option<&str>) -> Option<Arc<InnerMap>>;
    fn get_host(&self, peer: &str, path: &str, backend_id: Option<&str>) -> Option<Arc<InnerMap>>;
    fn get_header(&self, peer: &str, path: &str) -> Option<GetHostsReturHeaders>;
}
impl GetHost for LB {
    fn find_sticky_backend(&self, servers: &[Arc<InnerMap>], backend_id: Option<&str>) -> Option<Arc<InnerMap>> {
        let b = backend_id?;
        let ump_byid = self.ump_byid.load();
        let target = ump_byid.get(b)?;
        servers.iter().find(|s| s.address == target.address && s.port == target.port).map(|s| s.clone())
    }
    fn pick_backend(&self, servers: &[Arc<InnerMap>], index: &AtomicUsize, backend_id: Option<&str>) -> Option<Arc<InnerMap>> {
        if servers.is_empty() {
            return None;
        }
        if let Some(target) = self.find_sticky_backend(servers, backend_id) {
            return Some(target);
        }
        let idx = index.fetch_add(1, Ordering::Relaxed) % servers.len();
        Some(servers[idx].clone())
    }
    fn get_host(&self, peer: &str, path: &str, backend_id: Option<&str>) -> Option<Arc<InnerMap>> {
        let guard = self.ump_upst.load();
        let host_entry = guard.get(peer)?;
        let mut end = path.len();
        loop {
            let slice = &path[..end];
            if let Some(entry) = host_entry.get(slice) {
                let (servers, index) = entry;
                if let Some(backend) = self.pick_backend(servers, index, backend_id) {
                    return Some(backend);
                }
            }
            if let Some(pos) = slice.rfind('/') {
                end = pos;
            } else {
                break;
            }
        }
        if let Some(entry) = host_entry.get("/") {
            let (servers, index) = entry;
            if let Some(backend) = self.pick_backend(servers, index, backend_id) {
                return Some(backend);
            }
        }
        None
    }

    fn get_header(&self, peer: &str, path: &str) -> Option<GetHostsReturHeaders> {
        let ce = self.client_headers.load();
        let client_entry = ce.get(peer);

        let se = self.server_headers.load();
        let server_entry = se.get(peer);

        if client_entry.is_none() && server_entry.is_none() {
            return None;
        }

        let mut current_path = path;
        let mut clnt_match = None;

        if let Some(client_map) = client_entry {
            loop {
                if let Some(entry) = client_map.get(current_path) {
                    if !entry.is_empty() {
                        clnt_match = Some(entry.clone());
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

        if let Some(server_map) = server_entry {
            loop {
                if let Some(entry) = server_map.get(current_path) {
                    if !entry.is_empty() {
                        serv_match = Some(entry.clone());
                        break;
                    }
                }
                if current_path == "/" {
                    if let Some(entry) = server_map.get("/") {
                        if !entry.is_empty() {
                            serv_match = Some(entry.clone());
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
