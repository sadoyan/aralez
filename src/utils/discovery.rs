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
    async fn start(&self, tx: Sender<(UpstreamsDashMap, Headers)>);
}

#[async_trait]
impl Discovery for APIUpstreamProvider {
    async fn start(&self, toreturn: Sender<(UpstreamsDashMap, Headers)>) {
        webserver::run_server(self.address.clone(), toreturn).await;
    }
}

#[async_trait]
impl Discovery for FromFileProvider {
    async fn start(&self, tx: Sender<(UpstreamsDashMap, Headers)>) {
        tokio::spawn(watch_file(self.path.clone(), tx.clone()));
    }
}
pub async fn watch_file(fp: String, mut toreturn: Sender<(UpstreamsDashMap, Headers)>) {
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
