use dashmap::DashMap;
use rand::Rng;
use std::sync::Arc;

// type Db = Arc<RwLock<HashMap<Arc<str>, Arc<i32>>>>;
pub type Peers = Arc<DashMap<Arc<str>, Vec<Arc<str>>>>;

pub fn add_peers(peers: Peers, path: &str) {
    if let Some(mut peers_list) = peers.get_mut(&Arc::from(path)) {
        peers_list.push(Arc::from("http://192.168.1.1:8000"));
        peers_list.push(Arc::from("http://192.168.1.10:8000"));
    }
    println!("Adding peers {} -> {:?}", peers.get(path).unwrap().key(), peers.get(path).unwrap().value());
}
pub fn return_peer(peers: Peers, path: &str) -> Arc<str> {
    if let Some(peer_list) = peers.get(&Arc::from(path)) {
        let mut rng = rand::thread_rng();
        let r = rng.gen_range(0..peer_list.len());

        if let Some(selected_peer) = peer_list.get(r) {
            selected_peer.clone()
        } else {
            Arc::from("https://127.0.0.1:8443")
        }
    } else {
        Arc::from("https://127.0.0.1:8443")
    }
}
