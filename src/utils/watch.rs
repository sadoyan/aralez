use crate::tls::CertificateConfig;
use crate::utils::parceyaml::load_configuration;
use crate::utils::tools::listdir;
use crate::utils::types::Configuration;
use log::{error, info};
use notify::event::ModifyKind;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use pingora::prelude::sleep;
use std::path::Path;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Sender;
use tokio::task;

pub async fn file_watch(fp: String, toreturn: Sender<Configuration>) {
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
                    if start.elapsed() > Duration::from_secs(2) {
                        start = Instant::now();
                        let snd = load_configuration(file_path, "filepath").await.0;
                        if let Some(snd) = snd {
                            toreturn.send(snd).await.unwrap();
                        }
                    }
                }
                _ => (),
            },
            Err(e) => error!("Watch error: {:?}", e),
        }
    }
}
pub fn folder_watch(path: String, sender: std::sync::mpsc::Sender<Vec<CertificateConfig>>) -> notify::Result<()> {
    let (tx, rx) = channel();
    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;
    watcher.watch(path.as_ref(), RecursiveMode::Recursive)?;
    info!("Watching for certificates in : {}", path);
    let certificate_configs = listdir(path.clone());
    sender.send(certificate_configs)?;
    let mut start = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Ok(event)) => match &event.kind {
                EventKind::Modify(ModifyKind::Data(_)) | EventKind::Create(_) | EventKind::Remove(_) => {
                    if start.elapsed() > Duration::from_secs(1) {
                        start = Instant::now();
                        let certificate_configs = listdir(path.clone());
                        sender.send(certificate_configs)?;
                        info!("Certificate changed: {:?}, {:?}", event.kind, event.paths);
                    }
                }
                _ => {}
            },
            Ok(Err(e)) => error!("Watch error: {:?}", e),
            Err(_) => {}
        }
    }
}
