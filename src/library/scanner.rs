use std::path::Path;
use anyhow::Result;
use lofty::prelude::*;
use lofty::probe::Probe;
use walkdir::WalkDir;
use crate::library::Track;

const AUDIO_EXTS: &[&str] = &["mp3", "flac", "ogg", "opus", "m4a", "wav", "aac"];

pub fn scan_file(path: &str) -> Option<Track> {
    read_track(std::path::Path::new(path)).ok()
}

pub fn scan_folder(folder: &str) -> Vec<Track> {
    WalkDir::new(folder)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| AUDIO_EXTS.contains(&x.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .filter_map(|e| read_track(e.path()).ok())
        .collect()
}

fn read_track(path: &Path) -> Result<Track> {
    let tagged = Probe::open(path)?.guess_file_type()?.read()?;

    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

    let title = tag
        .and_then(|t| t.title().map(|s| s.to_string()))
        .or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        });

    let artist = tag.and_then(|t| t.artist().map(|s| s.to_string()));
    let album = tag.and_then(|t| t.album().map(|s| s.to_string()));
    let track_num = tag.and_then(|t| t.track()).map(|n| n as i64);

    let duration_secs = tagged
        .properties()
        .duration()
        .as_secs() as i64;

    Ok(Track {
        id: None,
        path: path.to_string_lossy().to_string(),
        title,
        artist,
        album,
        track_num,
        duration_secs: Some(duration_secs),
    })
}
