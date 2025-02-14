use dashmap::DashMap;
use futures::channel::mpsc::Sender;
use futures::SinkExt;
use std::fs;
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use notify::event::ModifyKind;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use tokio::sync::mpsc;
use tokio::task;

pub struct DSC;
#[async_trait]
pub trait Discovery {
    async fn discover(&self, tx: Sender<DashMap<String, (Vec<(String, u16)>, AtomicUsize)>>);
}

#[async_trait]
impl Discovery for DSC {
    async fn discover(&self, tx: Sender<DashMap<String, (Vec<(String, u16)>, AtomicUsize)>>) {
        let file_path = "etc/upstreams.conf";
        tokio::spawn(watch_file(file_path, tx));
    }
}

// pub async fn dsc(tx: Sender<DashMap<String, (Vec<(String, u16)>, AtomicUsize)>>) {
//     let file_path = "etc/upstreams.conf";
//     tokio::spawn(watch_file(file_path, tx));
// }

pub async fn watch_file(file_path: &str, mut toreturn: Sender<DashMap<String, (Vec<(String, u16)>, AtomicUsize)>>) {
    let parent_dir = Path::new(file_path).parent().unwrap(); // Watch directory, not file
    let (tx, mut rx) = mpsc::channel::<notify::Result<Event>>(10);

    println!("Watching for changes in {:?}", parent_dir);
    let paths = fs::read_dir(parent_dir).unwrap();
    for path in paths {
        println!("  {}", path.unwrap().path().display())
    }

    let snd = read_upstreams_from_file(file_path);
    let _ = toreturn.send(snd).await.unwrap();

    let _watcher_handle = task::spawn_blocking({
        let parent_dir = parent_dir.to_path_buf(); // Move directory path into the closure
        move || {
            let mut watcher = RecommendedWatcher::new(
                move |res| {
                    let _ = tx.blocking_send(res);
                },
                Config::default(),
            )
            .unwrap();
            watcher.watch(&parent_dir, RecursiveMode::Recursive).unwrap();

            loop {
                std::thread::sleep(Duration::from_secs(50));
            }
        }
    });
    let mut start = Instant::now();
    while let Some(event) = rx.recv().await {
        match event {
            Ok(e) => match e.kind {
                EventKind::Modify(ModifyKind::Data(_)) | EventKind::Create(..) | EventKind::Remove(..) => {
                    if e.paths[0].to_str().unwrap().ends_with("conf") {
                        if start.elapsed() > Duration::from_secs(10) {
                            start = Instant::now();
                            println!("Config File changed :=> {:?}", e);
                            let snd = read_upstreams_from_file(file_path);
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
fn read_upstreams_from_file(path: &str) -> DashMap<String, (Vec<(String, u16)>, AtomicUsize)> {
    let upstreams = DashMap::new();
    let contents = match fs::read_to_string(path) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error reading file: {:?}", e);
            return upstreams;
        }
    };

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
