/*
use crate::utils::structs::InnerMap;
use dashmap::DashMap;
use hickory_client::client::{Client, ClientHandle};
use hickory_client::proto::rr::{DNSClass, Name, RecordType};
use hickory_client::proto::runtime::TokioRuntimeProvider;
use hickory_client::proto::udp::UdpClientStream;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
use tokio::sync::Mutex;

type DnsError = Box<dyn std::error::Error + Send + Sync + 'static>;

pub struct DnsClientPool {
    clients: Vec<Mutex<DnsClient>>,
}

struct DnsClient {
    client: Client,
}

pub async fn start2(mut toreturn: Sender<Configuration>, config: Arc<Configuration>) {
    let k8s = config.kubernetes.clone();
    match k8s {
        Some(k8s) => {
            let dnserver = k8s.servers.unwrap_or(vec!["127.0.0.1:53".to_string()]);
            let headers = DashMap::new();
            let end = dnserver.len() - 1;
            let mut num = 0;
            if end > 0 {
                num = rand::rng().random_range(0..end);
            }
            let srv = dnserver.get(num).unwrap().to_string();
            let pool = DnsClientPool::new(5, srv.clone()).await;
            let u = UpstreamsDashMap::new();
            if let Some(whitelist) = k8s.services {
                loop {
                    let upstreams = UpstreamsDashMap::new();
                    for service in whitelist.iter() {
                        let ret = pool.query_srv(service.real.as_str(), srv.clone()).await;
                        match ret {
                            Ok(r) => {
                                upstreams.insert(service.proxy.clone(), r);
                            }
                            Err(e) => eprintln!("DNS query failed for {:?}: {:?}", service, e),
                        }
                    }
                    if !compare_dashmaps(&u, &upstreams) {
                        headers.clear();
                        for (k, v) in config.headers.clone() {
                            headers.insert(k.to_string(), v);
                        }

                        let mut tosend: Configuration = Configuration {
                            upstreams: Default::default(),
                            headers: Default::default(),
                            consul: None,
                            kubernetes: None,
                            typecfg: "".to_string(),
                            extraparams: config.extraparams.clone(),
                        };

                        clone_dashmap_into(&upstreams, &u);
                        clone_dashmap_into(&upstreams, &tosend.upstreams);
                        tosend.headers = headers.clone();
                        tosend.extraparams.authentication = config.extraparams.authentication.clone();
                        tosend.typecfg = config.typecfg.clone();
                        tosend.consul = config.consul.clone();
                        print_upstreams(&tosend.upstreams);
                        toreturn.send(tosend).await.unwrap();
                    }

                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
        None => {}
    }
}

impl DnsClient {
    pub async fn new(server: String) -> Result<Self, DnsError> {
        let server_details = server;
        let server: SocketAddr = server_details.parse().expect("Unable to parse socket address");
        let conn = UdpClientStream::builder(server, TokioRuntimeProvider::default()).build();
        let (client, bg) = Client::connect(conn).await.unwrap();
        tokio::spawn(bg);
        Ok(Self { client })
    }

    pub async fn query_srv(&mut self, name: &str) -> Result<DashMap<String, (Vec<InnerMap>, AtomicUsize)>, DnsError> {
        let upstreams: DashMap<String, (Vec<InnerMap>, AtomicUsize)> = DashMap::new();
        let mut values = Vec::new();
        match tokio::time::timeout(Duration::from_secs(5), self.client.query(Name::from_str(name)?, DNSClass::IN, RecordType::SRV)).await {
            Ok(Ok(response)) => {
                for answer in response.answers() {
                    if let hickory_client::proto::rr::RData::SRV(srv) = answer.data() {
                        let to_add = InnerMap {
                            address: srv.target().to_string(),
                            port: srv.port(),
                            is_ssl: false,
                            is_http2: false,
                            to_https: false,
                            rate_limit: None,
                        };
                        values.push(to_add);
                    }
                }
                upstreams.insert("/".to_string(), (values, AtomicUsize::new(0)));
                Ok(upstreams)
            }
            Ok(Err(e)) => Err(Box::new(e)),
            Err(_) => Err("DNS query timed out".into()),
        }
    }
}

impl DnsClientPool {
    pub async fn new(pool_size: usize, server: String) -> Self {
        let mut clients = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            if let Ok(client) = DnsClient::new(server.clone()).await {
                clients.push(Mutex::new(client));
            }
        }
        Self { clients }
    }

    pub async fn query_srv(&self, name: &str, server: String) -> Result<DashMap<String, (Vec<InnerMap>, AtomicUsize)>, DnsError> {
        // Try to get an available client
        for client_mutex in &self.clients {
            if let Ok(mut client) = client_mutex.try_lock() {
                let vay = client.query_srv(name).await;
                match vay {
                    Ok(_) => return vay,
                    Err(_) => {
                        // If query fails, drop this client and create a new one
                        *client = match DnsClient::new(server).await {
                            Ok(c) => c,
                            Err(e) => return Err(e),
                        };
                        // Retry with the new client
                        return client.query_srv(name).await;
                    }
                }
            }
        }

        // If all clients are busy, wait for the first one with a timeout
        match tokio::time::timeout(Duration::from_secs(2), self.clients[0].lock()).await {
            Ok(mut client) => client.query_srv(name).await,
            Err(_) => Err("All DNS clients are busy and timeout reached".into()),
        }
    }
}
*/
