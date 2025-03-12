use dashmap::DashMap;
use futures::channel::mpsc::Sender;
use futures::SinkExt;
use std::fs;
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, Instant};

use crate::utils::tools::*;
use crate::web::webserver;
use async_trait::async_trait;
use notify::event::ModifyKind;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use tokio::task;

pub struct FromFileProvider {
    pub path: String,
}
pub struct APIUpstreamProvider;

#[async_trait]
pub trait Discovery {
    async fn run(&self, tx: Sender<UpstreamMap>);
}

#[async_trait]
impl Discovery for APIUpstreamProvider {
    async fn run(&self, toreturn: Sender<UpstreamMap>) {
        webserver::run_server(toreturn).await;
    }
}

#[async_trait]
impl Discovery for FromFileProvider {
    async fn run(&self, tx: Sender<UpstreamMap>) {
        tokio::spawn(watch_file(self.path.clone(), tx.clone()));
    }
}
pub async fn watch_file(fp: String, mut toreturn: Sender<UpstreamMap>) {
    let file_path = fp.as_str();
    let parent_dir = Path::new(file_path).parent().unwrap(); // Watch directory, not file
    let (local_tx, mut local_rx) = tokio::sync::mpsc::channel::<notify::Result<Event>>(1);

    println!("Watching for changes in {:?}", parent_dir);
    let paths = fs::read_dir(parent_dir).unwrap();
    for path in paths {
        println!("  {}", path.unwrap().path().display())
    }

    let snd = build_upstreams(file_path, "filepath");
    let _ = toreturn.send(snd).await.unwrap();

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
                    if e.paths[0].to_str().unwrap().ends_with("conf") {
                        // if start.elapsed() > Duration::from_secs(10) {
                        if start.elapsed() > Duration::from_secs(2) {
                            start = Instant::now();
                            println!("Config File changed :=> {:?}", e);

                            let upstreams = build_upstreams2("etc/upstreams-long.conf", "filepath");

                            print_upstreams(&upstreams);

                            let host_entry = upstreams.get("myip.netangels.net").unwrap();
                            let path_entry = host_entry.get("/").unwrap();
                            for p in path_entry.value().0.clone() {
                                println!("  {:?}", p);
                            }

                            println!("\n\n");

                            let snd = build_upstreams(file_path, "filepath");
                            let _ = toreturn.send(snd).await.unwrap();
                        }
                    }
                }
                _ => (),
            },
            Err(e) => println!("Watch error: {:?}", e),
        }
    }
}
pub fn build_upstreams(d: &str, kind: &str) -> UpstreamMap {
    let upstreams = DashMap::new();
    let mut contents = d.to_string();
    match kind {
        "filepath" => {
            println!("Reading upstreams from {}", d);
            let _ = match fs::read_to_string(d) {
                Ok(data) => contents = data,
                Err(e) => {
                    eprintln!("Error reading file: {:?}", e);
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
        upstreams
            .entry(hostname.to_string()) // Step 1: Find or create entry
            .or_insert_with(|| (Vec::new(), AtomicUsize::new(0))) // Step 2: Insert if missing
            .0 // Step 3: Access the Vec<(String, u16)>
            .push((ip.to_string(), port)); // Step 4: Append new data
    }

    upstreams
}

pub fn build_upstreams2(d: &str, kind: &str) -> UpstresmDashMap {
    let upstreams: UpstresmDashMap = DashMap::new();
    let hopar: UpstresmDashMap = DashMap::new();
    let mut contents = d.to_string();
    match kind {
        "filepath" => {
            println!("Reading upstreams from {}", d);
            let _ = match fs::read_to_string(d) {
                Ok(data) => contents = data,
                Err(e) => {
                    eprintln!("Error reading file: {:?}", e);
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
        let entry = hopar.entry(hostname.to_string()).or_insert_with(DashMap::new);
        entry
            .entry(path.to_string())
            .or_insert_with(|| (Vec::new(), AtomicUsize::new(0)))
            .0
            .push((ip.to_string(), port, ssl, proto.to_string()));
    }
    // println!("\n\nResult ===> {} <===\n\n", dam(&hopar, &upstreams));
    // println!("{:?}", hopar);
    upstreams
}
