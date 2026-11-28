use crate::core::proxyhttp::LB;
use crate::utils::buildroutes::{collapse_slashes, find, norm_path};
use crate::utils::lazylock::{CLIENT_HEADERS, SERVER_HEADERS, UPSTREAMS};
use crate::utils::types::{HeaderEntries, InnerMap, Pool};
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
        let guard = UPSTREAMS.load();

        let router = guard.get(peer)?;
        let path = path.trim_end_matches('/');
        let path = if path.is_empty() { "/" } else { path };
        let p = collapse_slashes(path);
        let mut cur: Option<&Pool> = Some(router.at(p.as_ref()).ok()?.value);

        while let Some(pool) = cur {
            if let Some(b) = self.pick_backend(&pool.servers, &pool.index, backend_id) {
                return Some(b);
            }
            cur = pool.parent.as_deref();
        }
        None
    }
    fn get_header(&self, peer: &str, path: &str) -> Option<GetHostsReturHeaders> {
        let ce = CLIENT_HEADERS.load();
        let se = SERVER_HEADERS.load();
        let path = norm_path(path);

        let global_client = ce.get("GLOBAL_CLIENT_HEADERS").and_then(|r| find(r, path, |e: &HeaderEntries| !e.is_empty()));
        let host_client = ce.get(peer).and_then(|r| find(r, path, |e: &HeaderEntries| !e.is_empty()));

        let global_server = se.get("GLOBAL_SERVER_HEADERS").and_then(|r| find(r, path, |e: &HeaderEntries| !e.is_empty()));
        let host_server = se.get(peer).and_then(|r| find(r, path, |e: &HeaderEntries| !e.is_empty()));

        let client = match (global_client, host_client) {
            (None, None) => None,
            (Some(g), None) => Some(g.clone()),
            (None, Some(h)) => Some(h.clone()),
            (Some(g), Some(h)) => Some(g.iter().chain(h.iter()).cloned().collect()),
        };

        let server = match (global_server, host_server) {
            (None, None) => None,
            (Some(g), None) => Some(g.clone()),
            (None, Some(h)) => Some(h.clone()),
            (Some(g), Some(h)) => Some(g.iter().chain(h.iter()).cloned().collect()),
        };

        if client.is_none() && server.is_none() {
            return None;
        }
        Some(GetHostsReturHeaders {
            client_headers: client,
            server_headers: server,
        })
    }
}
