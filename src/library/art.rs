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
