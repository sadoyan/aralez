use crate::utils::parceyaml::load_configuration;
use crate::utils::structs::Configuration;
use futures::channel::mpsc::Sender;
use futures::SinkExt;
use log::error;
use notify::event::ModifyKind;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use pingora::prelude::sleep;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::task;

pub async fn start(fp: String, mut toreturn: Sender<Configuration>) {
    sleep(Duration::from_millis(50)).await; // For having nice logs :-)
    let file_path = fp.as_str();
    let parent_dir = Path::new(file_path).parent().unwrap();
    let (local_tx, mut local_rx) = tokio::sync::mpsc::channel::<notify::Result<Event>>(1);

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
                            // info!("Config File changed :=> {:?}", e);
                            let snd = load_configuration(file_path, "filepath").await;
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
