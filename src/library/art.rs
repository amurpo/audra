use std::path::Path;
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::picture::PictureType;

/// Nombres de archivo candidatos para arte de carpeta, en orden de preferencia.
const FOLDER_CANDIDATES: &[&str] = &[
    "cover.jpg", "cover.png", "cover.jpeg",
    "folder.jpg", "folder.png",
    "album.jpg", "album.png",
    "front.jpg", "front.png",
    "artwork.jpg", "artwork.png",
];

pub fn read_cover_art(path: &str) -> Option<Vec<u8>> {
    // 1. Intentar arte embebida en el archivo de audio
    if let Some(bytes) = read_embedded(path) {
        log::debug!("cover art: embebida encontrada en {path}");
        return Some(bytes);
    }

    // 2. Buscar imagen en la misma carpeta del archivo
    if let Some(bytes) = read_folder_art(path) {
        log::debug!("cover art: encontrada en carpeta para {path}");
        return Some(bytes);
    }

    log::debug!("cover art: no encontrada para {path}");
    None
}

fn read_embedded(path: &str) -> Option<Vec<u8>> {
    let tagged = Probe::open(path).ok()?.guess_file_type().ok()?.read().ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    let pictures = tag.pictures();

    pictures.iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first())
        .map(|p| p.data().to_vec())
}

fn read_folder_art(audio_path: &str) -> Option<Vec<u8>> {
    let dir = Path::new(audio_path).parent()?;

    // Primero buscar nombres canónicos exactos
    for name in FOLDER_CANDIDATES {
        let candidate = dir.join(name);
        if candidate.exists() {
            return std::fs::read(&candidate).ok();
        }
    }

    // Como último recurso, cualquier .jpg o .png en la carpeta
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            if matches!(ext.to_lowercase().as_str(), "jpg" | "jpeg" | "png") {
                return std::fs::read(&p).ok();
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct TmpDir(std::path::PathBuf);

    impl TmpDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let p = std::env::temp_dir()
                .join(format!("audra_art_{}_{}", std::process::id(), n));
            std::fs::create_dir_all(&p).unwrap();
            TmpDir(p)
        }
        /// Path of an audio file inside the dir (the file itself need not exist
        /// for folder-art lookup, which only inspects the parent directory).
        fn audio(&self) -> String {
            self.0.join("track.mp3").to_string_lossy().to_string()
        }
        fn put(&self, name: &str, bytes: &[u8]) {
            std::fs::write(self.0.join(name), bytes).unwrap();
        }
    }

    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn no_art_anywhere_returns_none() {
        let dir = TmpDir::new();
        assert_eq!(read_cover_art(&dir.audio()), None);
    }

    #[test]
    fn picks_up_a_canonical_folder_image() {
        let dir = TmpDir::new();
        dir.put("cover.png", b"PNGDATA");
        assert_eq!(read_cover_art(&dir.audio()), Some(b"PNGDATA".to_vec()));
    }

    #[test]
    fn canonical_candidates_win_over_arbitrary_images() {
        let dir = TmpDir::new();
        dir.put("zzz_random.jpg", b"RANDOM");
        dir.put("cover.jpg", b"CANONICAL");
        assert_eq!(read_cover_art(&dir.audio()), Some(b"CANONICAL".to_vec()));
    }

    #[test]
    fn cover_jpg_is_preferred_over_folder_jpg() {
        let dir = TmpDir::new();
        dir.put("folder.jpg", b"FOLDER");
        dir.put("cover.jpg", b"COVER");
        // cover.jpg comes before folder.jpg in FOLDER_CANDIDATES.
        assert_eq!(read_cover_art(&dir.audio()), Some(b"COVER".to_vec()));
    }

    #[test]
    fn falls_back_to_any_image_extension() {
        let dir = TmpDir::new();
        dir.put("art_scan.jpeg", b"JPEGDATA");
        assert_eq!(read_cover_art(&dir.audio()), Some(b"JPEGDATA".to_vec()));
    }
}
