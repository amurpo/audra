use std::path::Path;
use anyhow::Result;
use lofty::prelude::*;
use lofty::probe::Probe;
use walkdir::WalkDir;
use crate::library::Track;

const AUDIO_EXTS: &[&str] = &["mp3", "flac", "ogg", "opus", "m4a", "wav", "aac"];


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
            let p = std::env::temp_dir().join(format!(
                "audra_scan_{}_{}",
                std::process::id(),
                n
            ));
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

    #[test]
    fn scan_folder_on_empty_dir_returns_nothing() {
        let dir = TmpDir::new();
        assert!(scan_folder(dir.path().to_str().unwrap()).is_empty());
    }

    #[test]
    fn scan_folder_ignores_non_audio_files() {
        let dir = TmpDir::new();
        std::fs::write(dir.path().join("notes.txt"), b"hello").unwrap();
        std::fs::write(dir.path().join("cover.jpg"), b"\xff\xd8\xff").unwrap();
        assert!(scan_folder(dir.path().to_str().unwrap()).is_empty());
    }

    #[test]
    fn scan_folder_reads_audio_recursively_and_falls_back_to_filename() {
        let dir = TmpDir::new();
        let sub = dir.path().join("nested");
        std::fs::create_dir_all(&sub).unwrap();
        write_wav(&dir.path().join("song.wav"));
        write_wav(&sub.join("Deep Cut.wav"));
        std::fs::write(dir.path().join("readme.md"), b"x").unwrap();

        let mut tracks = scan_folder(dir.path().to_str().unwrap());
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
    }
}
