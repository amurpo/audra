use std::sync::{Arc, Mutex};

pub enum WatcherEvent {
    Created(String),
    Removed(String),
}

pub fn start_folder_watcher(
    folder: &str,
    events: Arc<Mutex<Vec<WatcherEvent>>>,
) -> Option<notify::RecommendedWatcher> {
    use notify::{Watcher, RecursiveMode};
    const AUDIO_EXTS: &[&str] = &["mp3", "flac", "ogg", "opus", "m4a", "wav", "aac"];

    let result = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        for path in &event.paths {
            let ext_ok = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| AUDIO_EXTS.contains(&e.to_lowercase().as_str()))
                .unwrap_or(false);
            if !ext_ok {
                continue;
            }
            let path_str = path.to_string_lossy().to_string();
            let evt = match &event.kind {
                notify::EventKind::Create(_) => WatcherEvent::Created(path_str),
                notify::EventKind::Remove(_) => WatcherEvent::Removed(path_str),
                _ => continue,
            };
            events.lock().unwrap().push(evt);
        }
    });

    match result {
        Ok(mut watcher) => {
            if watcher
                .watch(std::path::Path::new(folder), RecursiveMode::Recursive)
                .is_ok()
            {
                Some(watcher)
            } else {
                log::warn!("watcher: no se pudo vigilar '{}'", folder);
                None
            }
        }
        Err(e) => {
            log::error!("watcher: error al crear: {}", e);
            None
        }
    }
}
