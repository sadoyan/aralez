use crate::utils::parceyaml::load_yaml_to_dashmap;
use crate::utils::tools::*;
use crate::web::webserver;
use async_trait::async_trait;
use futures::channel::mpsc::Sender;
use futures::SinkExt;
use log::{error, info};
use notify::event::ModifyKind;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use pingora::prelude::sleep;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::task;

pub struct FromFileProvider {
    pub path: String,
}
pub struct APIUpstreamProvider {
    pub address: String,
}

#[async_trait]
pub trait Discovery {
    async fn start(&self, tx: Sender<UpstreamsDashMap>);
}

#[async_trait]
impl Discovery for APIUpstreamProvider {
    async fn start(&self, toreturn: Sender<UpstreamsDashMap>) {
        webserver::run_server(self.address.clone(), toreturn).await;
    }
}

#[async_trait]
impl Discovery for FromFileProvider {
    async fn start(&self, tx: Sender<UpstreamsDashMap>) {
        tokio::spawn(watch_file(self.path.clone(), tx.clone()));
    }
}
pub async fn watch_file(fp: String, mut toreturn: Sender<UpstreamsDashMap>) {
    sleep(Duration::from_millis(50)).await; // For having nice logs :-)
    let file_path = fp.as_str();
    let parent_dir = Path::new(file_path).parent().unwrap();
    let (local_tx, mut local_rx) = tokio::sync::mpsc::channel::<notify::Result<Event>>(1);
    info!("Watching for changes in {:?}", parent_dir);
    let snd = load_yaml_to_dashmap(file_path, "filepath");
    match snd {
        Some(snd) => {
            toreturn.send(snd).await.unwrap();
        }
        None => {}
    }

    let _watcher_handle = task::spawn_blocking({
        let parent_dir = parent_dir.to_path_buf(); // Move directory path into the closure
        move || {
            let mut watcher = RecommendedWatcher::new(
                move |res| {
                    let _ = local_tx.blocking_send(res);
                },
                Config::default(),
            )
            .unwrap();
            watcher.watch(&parent_dir, RecursiveMode::Recursive).unwrap();
            let (_rtx, mut rrx) = tokio::sync::mpsc::channel::<bool>(1);
            let _ = rrx.blocking_recv();
        }
    });
    let mut start = Instant::now();

    while let Some(event) = local_rx.recv().await {
        match event {
            Ok(e) => match e.kind {
                EventKind::Modify(ModifyKind::Data(_)) | EventKind::Create(..) | EventKind::Remove(..) => {
                    if e.paths[0].to_str().unwrap().ends_with("yaml") {
                        if start.elapsed() > Duration::from_secs(2) {
                            start = Instant::now();
                            info!("Config File changed :=> {:?}", e);
                            let snd = load_yaml_to_dashmap(file_path, "filepath");
                            match snd {
                                Some(snd) => {
                                    toreturn.send(snd).await.unwrap();
                                }
                                None => {}
                            }
                        }
                    }
                }
                _ => (),
            },
            Err(e) => error!("Watch error: {:?}", e),
        }
    }
}

/*
#[allow(dead_code)]
pub fn build_upstreams(d: &str, kind: &str) -> UpstreamsDashMap {
    let upstreams: UpstreamsDashMap = DashMap::new();
    let mut contents = d.to_string();
    match kind {
        "filepath" => {
            let _ = match fs::read_to_string(d) {
                Ok(data) => {
                    println!("Reading upstreams from {}", d);
                    contents = data
                }
                Err(e) => {
                    error!("Reading upstreams file: {:?}", e);
                    return upstreams;
                }
            };
        }
        "content" => {
            println!("Reading upstreams from API post body");
        }
        _ => println!("*******************> nothing <*******************"),
    }
    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
        let mut parts = line.split_whitespace();

        let Some(hostname) = parts.next() else {
            continue;
        };

        let Some(ssl) = string_to_bool(parts.next()) else {
            continue;
        };

        let Some(proto) = parts.next() else {
            continue;
        };
        let Some(path) = parts.next() else {
            continue;
        };
        let Some(address) = parts.next() else {
            continue;
        };

        let mut addr_parts = address.split(':');
        let Some(ip) = addr_parts.next() else {
            continue;
        };
        let Some(port_str) = addr_parts.next() else {
            continue;
        };

        let Ok(port) = port_str.parse::<u16>() else {
            continue;
        };

        let entry = upstreams.entry(hostname.to_string()).or_insert_with(DashMap::new);
        entry
            .entry(path.to_string())
            .or_insert_with(|| (Vec::new(), AtomicUsize::new(0)))
            .0
            .push((ip.to_string(), port, ssl, proto.to_string()));
    }
    upstreams
}
*/
