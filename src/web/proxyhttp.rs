use async_trait::async_trait;
use dashmap::DashMap;
use log::{info, warn};
use pingora::prelude::*;
use pingora_core::prelude::HttpPeer;
use pingora_http::{RequestHeader, ResponseHeader};
use pingora_proxy::{ProxyHttp, Session};
use std::any::type_name;
use std::sync::atomic::{AtomicUsize, Ordering};

#[allow(dead_code)]
pub fn typeoff<T>(_: T) {
    let to = type_name::<T>();
    println!("{:?}", to);
}

// pub struct LB(pub Arc<LoadBalancer<RoundRobin>>);
#[allow(dead_code)]
pub struct LB {
    // pub load_balancer: Arc<LoadBalancer<RoundRobin>>,
    // pub upstreams_map: Arc<HashMap<String, Vec<(String, u16)>>>,
    // pub upstreams_map: Arc<Mutex<HashMap<String, Vec<(String, u16)>>>>,
    // upstreams: DashMap<String, (Vec<(&'static str, u16)>, AtomicUsize)>,
    // pub upstreams_map: DashMap<String, Vec<(String, u16)>>,
    // pub upstreams_maps: DashMap<String, Arc<LoadBalancer<RoundRobin>>>,
    pub upstreams_map: DashMap<String, (Vec<(String, u16)>, AtomicUsize)>,
}

#[async_trait]
pub trait GetHost {
    async fn get_host(&self, peer: &str) -> Option<(String, u16)>;
    fn set_host(&mut self, peer: &str, host: &str, port: u16);
    fn discover_hosts(&mut self);
}
#[async_trait]
impl GetHost for LB {
    async fn get_host(&self, peer: &str) -> Option<(String, u16)> {
        // println!("{:?}", self.upstreams_map);
        // let entry = self.upstreams_map.get(peer)?;
        // let first = entry.value().first()?;
        // println!("{:?}", entry.value());
        // Some((first.0.clone(), first.1))

        let entry = self.upstreams_map.get(peer)?;
        let (servers, index) = entry.value();

        if servers.is_empty() {
            return None;
        }

        let idx = index.fetch_add(1, Ordering::Relaxed) % servers.len();
        println!("{} {:?} => len: {}, idx: {}", peer, servers[idx], servers.len(), idx);
        Some(servers[idx].clone())
    }

    fn set_host(&mut self, peer: &str, host: &str, port: u16) {
        // let new_value = vec![(host.to_string(), port)];
        // self.upstreams_map.insert(peer.to_string(), (new_value, AtomicUsize::new(0)));

        let exists = self.upstreams_map.get(peer);
        let mut toreturn = vec![];
        match exists {
            Some(e) => {
                let (ko, _) = e.value();
                let new_value = vec![(host.to_string(), port)];
                for (k, v) in ko.clone().iter() {
                    toreturn.push((k.to_string(), v.to_owned()));
                }
                toreturn.push(new_value[0].clone());
            }
            None => {
                toreturn.push((host.to_string(), port));
            }
        }

        println!(" ==> Updating peer list: name => {} | value => {:?}", peer.to_string(), toreturn);
        self.upstreams_map.insert(peer.to_string(), (toreturn, AtomicUsize::new(0)));

        // self.upstreams_map.insert(peer.to_string(), toreturn);

        // use std::time::Instant;
        // let now = Instant::now();
        // self.get_host(peer);
        // let elapsed = now.elapsed();
        // println!("Elapsed: {:.2?}", elapsed);
    }

    fn discover_hosts(&mut self) {
        self.set_host("myip.netangels.net", "192.168.1.1", 8000);
        self.set_host("myip.netangels.net", "127.0.0.1", 8000);
        self.set_host("myip.netangels.net", "127.0.0.2", 8000);
        self.set_host("polo.netangels.net", "192.168.1.1", 8000);
        self.set_host("polo.netangels.net", "192.168.1.10", 8000);
        self.set_host("glop.netangels.net", "192.168.1.20", 8000);
    }
}

#[async_trait]
impl ProxyHttp for LB {
    type CTX = ();
    fn new_ctx(&self) -> Self::CTX {}
    // async fn upstream_peer(&self, _session: &mut Session, _ctx: &mut ()) -> Result<Box<HttpPeer>> {
    //     let upstream = self.load_balancer.select(b"", 256).ok_or_else(|| Error::explain(HTTPStatus(503), "no upstreams"))?;
    //     let peer = HttpPeer::new(upstream.addr, false, "".to_string());
    //
    //     let host_name = _session.req_header().headers.get("host");
    //     let fo = self.get_host(host_name.unwrap().to_str().unwrap());
    //     println!("{:?}", fo);
    //
    //     Ok(Box::new(peer))
    // }

    async fn upstream_peer(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<Box<HttpPeer>> {
        let host_name = session.req_header().headers.get("host");
        let ddr = self.get_host(host_name.unwrap().to_str().unwrap());
        match ddr.await {
            Some((host, port)) => {
                let peer = Box::new(HttpPeer::new((host, port), false, "".to_string()));
                Ok(peer)
            }
            None => {
                println!("Returning default list => {:?}", ("127.0.0.1", 8000));
                let peer = Box::new(HttpPeer::new(("127.0.0.1", 8000), false, "".to_string()));
                Ok(peer)
            }
        }
    }

    /*

        async fn upstream_peer(&self, session: &mut Session, _ctx: &mut Self::CTX) -> Result<Box<HttpPeer>> {
            let host_name = session.req_header().headers.get("host").unwrap();
            let addr = self.get_host(host_name.to_str().unwrap()).unwrap();
            info!("connecting to {addr:?}");
            let peer = Box::new(HttpPeer::new(addr, false, "".to_string()));
            Ok(peer)
        }
    */
    async fn request_filter(&self, _session: &mut Session, _ctx: &mut Self::CTX) -> pingora_core::Result<bool>
    where
        Self::CTX: Send + Sync,
    {
        // if !_session.req_header().uri.path().starts_with("/ping") {
        if _session.req_header().uri.path().starts_with("/denied") {
            let _ = _session.respond_error(403).await;
            info!("Forbidded: {:?}, {}", _session.client_addr(), _session.req_header().uri.path().to_string());
            return Ok(true);
        };
        Ok(false)
    }
    async fn upstream_request_filter(&self, _session: &mut Session, _upstream_request: &mut RequestHeader, _ctx: &mut Self::CTX) -> pingora_core::Result<()>
    where
        Self::CTX: Send + Sync,
    {
        let clientip = _session.client_addr();
        match clientip {
            Some(ip) => {
                let inet = ip.as_inet();
                match inet {
                    Some(addr) => {
                        _upstream_request
                            .insert_header("X-Forwarded-For", addr.to_string().split(':').collect::<Vec<&str>>()[0])
                            .unwrap();
                    }
                    None => warn!("Malformed Client IP: {:?}", inet),
                }
            }
            None => {
                warn!("Cannot detect client IP");
            }
        }
        Ok(())
    }
    async fn response_filter(&self, _session: &mut Session, _upstream_response: &mut ResponseHeader, _ctx: &mut Self::CTX) -> pingora_core::Result<()>
    where
        Self::CTX: Send + Sync,
    {
        _upstream_response.insert_header("X-Proxied-From", "Fooooooooooooooo").unwrap();
        Ok(())
    }
    // async fn logging(&self, session: &mut Session, _e: Option<&pingora::Error>, ctx: &mut Self::CTX) {
    //     let response_code = session.response_written().map_or(0, |resp| resp.status.as_u16());
    //     info!("{}, response code: {response_code}", self.request_summary(session, ctx));
    // }
}
