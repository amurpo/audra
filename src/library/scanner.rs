use crate::library::Track;
use anyhow::Result;
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::ItemKey;
use std::collections::HashMap;
use std::path::Path;
use walkdir::WalkDir;

// Must match the features enabled on `rodio` in Cargo.toml. Listing an
// extension here without the matching decoder makes the file appear in the
// library and then fail at play time.
const AUDIO_EXTS: &[&str] = &["mp3", "flac", "ogg", "wav"];

/// Outcome of one folder scan.
///
/// `tracks` holds only new files and files whose mtime changed since the last
/// scan — the ones whose tags were actually (re-)read. `found_paths` lists
/// every audio file seen, including unchanged ones, so stale-row cleanup
/// (`remove_missing_from_folder`) still sees the full picture.
pub struct ScanResult {
    pub tracks: Vec<Track>,
    pub found_paths: Vec<String>,
}

/// Scan `folder` for audio files. `known_mtimes` (path → mtime as stored in
/// the DB) makes the scan incremental: a file whose on-disk mtime matches its
/// stored one is counted as found but its tags are not re-read. Pass an empty
/// map to force a full read of every file.
pub fn scan_folder(folder: &str, known_mtimes: &HashMap<String, i64>) -> ScanResult {
    let mut tracks = Vec::new();
    let mut found_paths = Vec::new();

    let entries = WalkDir::new(folder)
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
        });

    for entry in entries {
        let path_str = entry.path().to_string_lossy().to_string();
        let mtime = file_mtime(&entry);
        found_paths.push(path_str.clone());
        if let (Some(mt), Some(known)) = (mtime, known_mtimes.get(&path_str)) {
            if mt == *known {
                continue; // unchanged since last scan: skip the tag read
            }
        }
        if let Ok(mut track) = read_track(entry.path()) {
            track.mtime = mtime;
            tracks.push(track);
        }
    }

    ScanResult {
        tracks,
        found_paths,
    }
}

/// Modification time as Unix seconds, or `None` when the metadata is
/// unavailable (the file is then always re-read — the safe fallback).
fn file_mtime(entry: &walkdir::DirEntry) -> Option<i64> {
    entry
        .metadata()
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
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
    let disc_num = tag.and_then(|t| t.disk()).map(|n| n as i64);
    let album_artist = tag.and_then(|t| t.get_string(&ItemKey::AlbumArtist).map(|s| s.to_string()));

    let duration_secs = tagged.properties().duration().as_secs() as i64;

    Ok(Track {
        id: None,
        path: path.to_string_lossy().to_string(),
        title,
        artist,
        album,
        track_num,
        duration_secs: Some(duration_secs),
        disc_num,
        album_artist,
        // Filled by `scan_folder` from the directory entry; reading it here
        // would cost an extra stat per file.
        mtime: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Unique scratch directory under the OS temp dir, cleaned up on drop.
    struct TmpDir(std::path::PathBuf);

    impl TmpDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let p = std::env::temp_dir().join(format!("audra_scan_{}_{}", std::process::id(), n));
            std::fs::create_dir_all(&p).unwrap();
            TmpDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Minimal valid 16-bit PCM mono WAV that lofty can probe.
    fn write_wav(path: &Path) {
        let sample_rate: u32 = 8000;
        let samples: u32 = 800; // 0.1 s of silence
        let data_len = samples * 2;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
        wav.extend_from_slice(&2u16.to_le_bytes()); // block align
        wav.extend_from_slice(&16u16.to_le_bytes()); // bits/sample
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.extend(std::iter::repeat_n(0u8, data_len as usize));
        std::fs::write(path, wav).unwrap();
    }

    /// Full (non-incremental) scan: no known mtimes.
    fn scan_all(folder: &str) -> ScanResult {
        scan_folder(folder, &HashMap::new())
    }

    #[test]
    fn scan_folder_on_empty_dir_returns_nothing() {
        let dir = TmpDir::new();
        let result = scan_all(dir.path().to_str().unwrap());
        assert!(result.tracks.is_empty());
        assert!(result.found_paths.is_empty());
    }

    #[test]
    fn scan_folder_ignores_non_audio_files() {
        let dir = TmpDir::new();
        std::fs::write(dir.path().join("notes.txt"), b"hello").unwrap();
        std::fs::write(dir.path().join("cover.jpg"), b"\xff\xd8\xff").unwrap();
        let result = scan_all(dir.path().to_str().unwrap());
        assert!(result.tracks.is_empty());
        assert!(result.found_paths.is_empty());
    }

    #[test]
    fn scan_folder_reads_audio_recursively_and_falls_back_to_filename() {
        let dir = TmpDir::new();
        let sub = dir.path().join("nested");
        std::fs::create_dir_all(&sub).unwrap();
        write_wav(&dir.path().join("song.wav"));
        write_wav(&sub.join("Deep Cut.wav"));
        std::fs::write(dir.path().join("readme.md"), b"x").unwrap();

        let mut tracks = scan_all(dir.path().to_str().unwrap()).tracks;
        tracks.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(tracks.len(), 2, "both nested and top-level WAVs found");
        // No tags => title falls back to the file stem.
        let titles: Vec<_> = tracks
            .iter()
            .map(|t| t.title.clone().unwrap_or_default())
            .collect();
        assert!(titles.contains(&"song".to_string()));
        assert!(titles.contains(&"Deep Cut".to_string()));
        assert!(tracks.iter().all(|t| t.duration_secs.is_some()));
        // Every scanned track carries the mtime the incremental rescan needs.
        assert!(tracks.iter().all(|t| t.mtime.is_some()));
    }

    #[test]
    fn rescan_skips_unchanged_files_but_still_reports_them_found() {
        let dir = TmpDir::new();
        write_wav(&dir.path().join("a.wav"));
        write_wav(&dir.path().join("b.wav"));

        let first = scan_all(dir.path().to_str().unwrap());
        assert_eq!(first.tracks.len(), 2);

        // Simulate the DB state after the first scan.
        let known: HashMap<String, i64> = first
            .tracks
            .iter()
            .filter_map(|t| t.mtime.map(|m| (t.path.clone(), m)))
            .collect();
        assert_eq!(known.len(), 2);

        let second = scan_folder(dir.path().to_str().unwrap(), &known);
        assert!(second.tracks.is_empty(), "unchanged files must be skipped");
        assert_eq!(
            second.found_paths.len(),
            2,
            "skipped files still count as found (stale-row cleanup needs them)"
        );

        // A changed mtime forces a re-read of just that file.
        let mut stale = known.clone();
        if let Some(m) = stale.get_mut(&first.tracks[0].path) {
            *m -= 10;
        }
        let third = scan_folder(dir.path().to_str().unwrap(), &stale);
        assert_eq!(third.tracks.len(), 1);
        assert_eq!(third.tracks[0].path, first.tracks[0].path);
    }
}
