//! Pure, audio-free deduplication pipeline.
//!
//! Raw tags in the DB are the source of truth and are never mutated here; this
//! module only computes a *canonical* (artist, album) grouping for the Albums
//! and Artists views. `Track.artist` (the real performer) is left untouched so
//! the scrobbler and the song list keep the original tag.
//!
//! Two levels, derived from the on-disk layout under the configured music
//! folder:
//!   1. Artist  = first path component under the music folder (stable even when
//!      the artist tag is in a foreign script or inconsistently spelled).
//!   2. Album   = the artist's sub-folder, with multi-disc folders folded into
//!      one release and genuinely mixed folders subdivided by album tag.
//!
//! When a track is not under the music folder (or no folder is configured) it
//! falls back to pure tag grouping, i.e. the historical behaviour.

use crate::library::{Album, Track};
use std::collections::HashMap;
use std::path::{Component, Path};

/// Folding key used only for equality checks; never shown to the user.
/// Lowercase, trimmed, punctuation removed, whitespace collapsed. Diacritics
/// are intentionally left as-is: the real library shows no accent-only
/// collisions, and folding them would need a unicode dependency we avoid.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.trim().chars() {
        if ch.is_alphanumeric() {
            for lc in ch.to_lowercase() {
                out.push(lc);
            }
            prev_space = false;
        } else if ch.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
            }
            prev_space = true;
        }
        // any other punctuation is dropped
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Strip a trailing/embedded disc marker so `… Disco 1`, `… (CD 2)`,
/// `… [disc1]`, `… cd 3` all fold to the same album base. Returns the input
/// untouched when no marker is found.
pub fn strip_disc_marker(name: &str) -> String {
    let lower = name.to_lowercase();
    let bytes = lower.as_bytes();
    let mut best: Option<usize> = None;
    for kw in ["disco", "disc", "cd"] {
        let mut from = 0;
        while let Some(rel) = lower[from..].find(kw) {
            let start = from + rel;
            // Word boundary on the left.
            let left_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
            let mut i = start + kw.len();
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'.') {
                i += 1;
            }
            let digits_start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if left_ok && i > digits_start {
                best = Some(match best {
                    Some(b) => b.min(start),
                    None => start,
                });
            }
            from = start + kw.len();
        }
    }
    match best {
        Some(cut) => {
            // Also drop opening bracket/paren and separators just before it.
            let mut end = cut;
            let raw = name.as_bytes();
            while end > 0 && matches!(raw[end - 1], b' ' | b'-' | b'_' | b'(' | b'[' | b'{' | b'.')
            {
                end -= 1;
            }
            name[..end].trim().to_string()
        }
        None => name.trim().to_string(),
    }
}

/// Fold a single path component to a filesystem-equality key.
///
/// On Windows the filesystem is case-insensitive and the same location has
/// several spellings: `C:` vs `c:`, and the verbatim/extended-length form
/// `\\?\C:\…`. The saved `music_folder` (built from the scan path) and the
/// scanned track paths can therefore differ only in case or prefix decoration;
/// comparing `std::path::Component` verbatim would then silently disable the
/// whole folder-aware pipeline and fall back to raw-tag grouping. So we strip
/// the `\\?\` decoration and lowercase. Unix filesystems are case-sensitive
/// (`Beatles` and `beatles` are distinct folders), so there we compare as-is.
fn fs_fold(s: &str) -> String {
    if cfg!(windows) {
        s.strip_prefix(r"\\?\").unwrap_or(s).to_lowercase()
    } else {
        s.to_string()
    }
}

fn comp_eq(a: &Component, b: &Component) -> bool {
    fs_fold(&a.as_os_str().to_string_lossy()) == fs_fold(&b.as_os_str().to_string_lossy())
}

/// Path components of `path`'s parent, relative to `music_folder`. `None` when
/// no folder is configured or the track lives outside it.
fn rel_dirs(path: &str, music_folder: Option<&str>) -> Option<Vec<String>> {
    let folder = music_folder?;
    let base: Vec<Component> = Path::new(folder).components().collect();
    let full = Path::new(path);
    let parent = full.parent()?;
    let parts: Vec<Component> = parent.components().collect();
    if parts.len() < base.len()
        || !parts[..base.len()]
            .iter()
            .zip(base.iter())
            .all(|(p, b)| comp_eq(p, b))
    {
        return None;
    }
    Some(
        parts[base.len()..]
            .iter()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(s.to_string_lossy().to_string()),
                _ => None,
            })
            .collect(),
    )
}

/// Most frequent original spelling among `values`, keyed by their normalized
/// form. Returns `None` when every value normalizes to empty.
fn dominant<'a, I: Iterator<Item = &'a str>>(values: I) -> Option<String> {
    let mut counts: HashMap<String, (usize, String)> = HashMap::new();
    for v in values {
        let n = normalize(v);
        if n.is_empty() {
            continue;
        }
        counts.entry(n).or_insert((0, v.to_string())).0 += 1;
    }
    counts
        .into_values()
        .max_by_key(|(c, _)| *c)
        .map(|(_, label)| label)
}

struct Derived {
    artist_key: String,
    artist_label: String,
    album_key: String,
    album_label: String,
}

fn derive(track: &Track, music_folder: Option<&str>) -> Derived {
    // album_artist is used for album-level grouping in tag mode but must not
    // override the performer (artist tag) for display: a track tagged
    // artist=INFIX / album_artist=千住明 should show as INFIX, not 千住明.
    let tag_artist = track
        .album_artist
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or(track.artist.as_deref())
        .map(str::to_string);

    let display_artist = track
        .artist
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .or_else(|| tag_artist.clone());

    match rel_dirs(&track.path, music_folder) {
        Some(dirs) if !dirs.is_empty() => {
            let folder_name = dirs[0].clone();
            let artist_label = display_artist.unwrap_or_else(|| folder_name.clone());
            let (album_label, _from_folder) = if dirs.len() >= 2 {
                (strip_disc_marker(&dirs[1]), true)
            } else {
                (
                    track
                        .album
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_default(),
                    false,
                )
            };
            Derived {
                artist_key: normalize(&folder_name),
                artist_label,
                album_key: normalize(&album_label),
                album_label,
            }
        }
        // Outside the music folder (or none configured): pure tag grouping,
        // i.e. the historical behaviour.
        _ => {
            let artist = track.display_artist();
            let album = track.display_album();
            Derived {
                artist_key: normalize(&artist),
                artist_label: display_artist.unwrap_or(artist),
                album_key: normalize(&album),
                album_label: album,
            }
        }
    }
}

/// Group tracks into canonical albums. `music_folder` enables the folder-aware
/// pipeline; `None` reproduces pure tag grouping.
pub fn group_albums(tracks: &[Track], music_folder: Option<&str>) -> Vec<Album> {
    // Stage 1: bucket by canonical artist.
    let mut by_artist: HashMap<String, Vec<(Derived, &Track)>> = HashMap::new();
    for t in tracks {
        let d = derive(t, music_folder);
        by_artist
            .entry(d.artist_key.clone())
            .or_default()
            .push((d, t));
    }

    let mut albums: Vec<Album> = Vec::new();

    for (_akey, mut rows) in by_artist {
        let artist_label = dominant(rows.iter().map(|(d, _)| d.artist_label.as_str()))
            .unwrap_or_else(|| rows[0].0.artist_label.clone());

        // "Various Artists" detection. Two shapes both qualify:
        //   1. No music folder at all (or track outside it): label comes from
        //      tags, and performers differ → tag grouping picked a wrong
        //      artist by accident.
        //   2. Tracks live directly under a single folder under the music
        //      folder (Compilations/track.mp3, no per-album subfolder) and
        //      performers differ → it's an OST or compilation, not a real
        //      single-artist release.
        let distinct_perf: std::collections::HashSet<String> = rows
            .iter()
            .map(|(_, t)| normalize(&t.display_artist()))
            .collect();
        // True when every track is either outside the music folder, or sits
        // directly under a single folder (no per-album subfolder).
        let single_level_or_loose = rows.iter().all(|(_, t)| {
            rel_dirs(&t.path, music_folder)
                .map(|d| d.len() <= 1)
                .unwrap_or(true)
        });
        let artist_label = if single_level_or_loose && distinct_perf.len() > 1 {
            crate::i18n::gettext("Various Artists")
        } else {
            artist_label
        };

        // Stage 2: bucket by canonical album within the artist. A folder with
        // several distinct album tags is a mixed folder and gets subdivided by
        // the album tag instead of collapsing into one release.
        rows.sort_by(|a, b| a.0.album_key.cmp(&b.0.album_key));
        // Pre-accumulate, per album key, the set of distinct non-empty album
        // tags. Done once here so the grouping loop stays O(1) per track
        // instead of re-scanning every row for each track — that quadratic
        // path bit hard on huge buckets (a flat "loose files" root, or a big
        // Various Artists bucket).
        let mut tags_by_album: HashMap<&str, std::collections::HashSet<String>> = HashMap::new();
        for (d, t) in &rows {
            if let Some(tag) = t.album.as_deref().map(normalize).filter(|s| !s.is_empty()) {
                tags_by_album
                    .entry(d.album_key.as_str())
                    .or_default()
                    .insert(tag);
            }
        }
        let mut groups: HashMap<String, Vec<(&Derived, &Track)>> = HashMap::new();
        for (d, t) in &rows {
            let tag_key = t.album.as_deref().map(normalize).filter(|s| !s.is_empty());
            // Subdivide only when the folder genuinely holds >1 album tag.
            let multi_tag = tags_by_album
                .get(d.album_key.as_str())
                .map(|s| s.len() > 1)
                .unwrap_or(false);
            let key = if multi_tag {
                tag_key.unwrap_or_else(|| d.album_key.clone())
            } else {
                d.album_key.clone()
            };
            groups.entry(key).or_default().push((d, t));
        }

        // Stage 3: tag-based merge pass.
        // Stage 2 keys groups by folder name. When a release spans multiple
        // differently-named folders (common in classical rips: one folder per
        // disc content, one more with the full album title), they stay as
        // separate groups even though every track shares the same album tag.
        // Merge them when: (a) multiple groups share the same dominant tag
        // (tag_norm), AND (b) at least one group's key already equals that
        // tag_norm — meaning a folder IS the canonical album title.
        // The "at least one folder matches" guard prevents merging folders
        // that share a generic tag (e.g. "Concertos") where the folder
        // structure is the intended grouping.
        {
            let mut by_tag_norm: HashMap<String, Vec<String>> = HashMap::new();
            for (key, members) in &groups {
                let tag_norm = dominant(members.iter().filter_map(|(_, t)| t.album.as_deref()))
                    .map(|s| normalize(&s))
                    .unwrap_or_default();
                if !tag_norm.is_empty() {
                    by_tag_norm.entry(tag_norm).or_default().push(key.clone());
                }
            }
            let mut to_merge: Vec<(String, String)> = Vec::new();
            for (tag_norm, keys) in &by_tag_norm {
                if keys.len() < 2 {
                    continue;
                }
                if keys.iter().any(|k| k == tag_norm) {
                    for k in keys {
                        if k != tag_norm {
                            to_merge.push((k.clone(), tag_norm.clone()));
                        }
                    }
                }
            }
            for (src, dst) in to_merge {
                if let Some(src_members) = groups.remove(&src) {
                    groups.entry(dst).or_default().extend(src_members);
                }
            }
        }

        for (_gkey, members) in groups {
            let mut tracks: Vec<Track> = members.iter().map(|(_, t)| (*t).clone()).collect();
            tracks.sort_by_key(|t| {
                (
                    t.disc_num.unwrap_or(1),
                    t.track_num.unwrap_or(999),
                    t.path.clone(),
                )
            });
            // Prefer a real album tag for display; fall back to the (disc
            // stripped) folder label.
            let name = dominant(members.iter().filter_map(|(_, t)| t.album.as_deref()))
                .or_else(|| {
                    members
                        .iter()
                        .map(|(d, _)| d.album_label.clone())
                        .find(|s| !s.trim().is_empty())
                })
                .unwrap_or_else(|| tracks[0].display_album());
            albums.push(Album {
                name,
                artist: artist_label.clone(),
                tracks,
                cover: None,
            });
        }
    }

    albums.sort_by(|a, b| a.name.cmp(&b.name).then(a.artist.cmp(&b.artist)));
    albums
}

/// Map every raw `(artist_tag, album_tag)` present in the library to its
/// canonical `(artist, album)`. Used to migrate cover/photo keys without
/// losing user picks.
pub fn canonical_key_map(
    tracks: &[Track],
    music_folder: Option<&str>,
) -> HashMap<(String, String), (String, String)> {
    let albums = group_albums(tracks, music_folder);
    // Reverse-index: which canonical album owns each track path.
    let mut path_to_canon: HashMap<&str, (String, String)> = HashMap::new();
    for a in &albums {
        for t in &a.tracks {
            path_to_canon.insert(t.path.as_str(), (a.artist.clone(), a.name.clone()));
        }
    }
    let mut map = HashMap::new();
    for t in tracks {
        if let Some(canon) = path_to_canon.get(t.path.as_str()) {
            let raw = (t.display_artist(), t.display_album());
            if raw != *canon {
                map.insert(raw, canon.clone());
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(path: &str, artist: &str, album: &str, num: i64) -> Track {
        Track {
            id: None,
            path: path.to_string(),
            title: Some(format!("{num}")),
            artist: Some(artist.to_string()),
            album: Some(album.to_string()),
            track_num: Some(num),
            duration_secs: Some(180),
            disc_num: None,
            album_artist: None,
        }
    }

    #[test]
    fn canonical_key_map_collapses_inconsistent_tags_to_one_canon() {
        let mf = Some("/Music");
        let tracks = vec![
            t(
                "/Music/Tokyo Phil/Gundam/1.mp3",
                "東京フィル",
                "gundam ost",
                1,
            ),
            t(
                "/Music/Tokyo Phil/Gundam/2.mp3",
                "Tokyo Phil.",
                "Gundam OST",
                2,
            ),
        ];
        let map = canonical_key_map(&tracks, mf);
        // Both raw (artist, album) pairs must resolve to the same canonical
        // pair so their covers converge instead of being lost.
        let canon: std::collections::HashSet<_> = map.values().collect();
        assert_eq!(canon.len(), 1);
    }

    #[test]
    fn normalize_folds_case_space_and_punctuation() {
        assert_eq!(
            normalize("Return Of The Killer A's"),
            "return of the killer as"
        );
        assert_eq!(normalize("Mellon Collie  "), "mellon collie");
        assert_eq!(normalize("III: Over The Under"), "iii over the under");
    }

    #[test]
    fn strip_disc_marker_handles_real_world_variants() {
        assert_eq!(
            strip_disc_marker("Made in Japan [1972] Disco 1"),
            "Made in Japan [1972]"
        );
        assert_eq!(strip_disc_marker("Mellon Collie CD 2"), "Mellon Collie");
        assert_eq!(strip_disc_marker("Whocares (Cd 1)"), "Whocares");
        assert_eq!(strip_disc_marker("Symphony [disc1]"), "Symphony");
        // No marker => untouched.
        assert_eq!(strip_disc_marker("Abbey Road"), "Abbey Road");
        // "cd" inside a word must not trigger.
        assert_eq!(strip_disc_marker("Mcdonald Songs"), "Mcdonald Songs");
    }

    #[test]
    fn no_music_folder_is_pure_tag_grouping() {
        let tracks = vec![
            t("/m/a.mp3", "A", "Beta", 2),
            t("/m/b.mp3", "A", "Beta", 1),
            t("/m/c.mp3", "B", "Greatest Hits", 1),
            t("/m/d.mp3", "A", "Greatest Hits", 1),
        ];
        let albums = group_albums(&tracks, None);
        assert_eq!(albums.len(), 3);
        let beta = albums.iter().find(|a| a.name == "Beta").unwrap();
        assert_eq!(beta.tracks.len(), 2);
        assert_eq!(beta.tracks[0].track_num, Some(1));
        // Same album name, different artist stays separate.
        assert_eq!(
            albums.iter().filter(|a| a.name == "Greatest Hits").count(),
            2
        );
    }

    #[test]
    fn folder_groups_tracks_with_inconsistent_tags_into_one_album() {
        let mf = Some("/Music");
        let tracks = vec![
            t(
                "/Music/Tokyo City Philharmonic Orchestra/Gundam/1.mp3",
                "東京シティフィルハーモニック管弦楽団",
                "Gundam",
                1,
            ),
            t(
                "/Music/Tokyo City Philharmonic Orchestra/Gundam/2.mp3",
                "Tokyo City Phil.",
                "Gundam",
                2,
            ),
        ];
        let albums = group_albums(&tracks, mf);
        // Folder acts as the grouping key so both tracks land in one album
        // regardless of the inconsistent artist tags.
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].tracks.len(), 2);
        // Artist label comes from the dominant tag, not the folder name.
        let artist = &albums[0].artist;
        assert!(
            artist == "東京シティフィルハーモニック管弦楽団" || artist == "Tokyo City Phil.",
            "unexpected artist label: {artist}"
        );
    }

    #[test]
    fn artist_label_comes_from_tag_not_abbreviated_folder() {
        let mf = Some("/Music");
        let tracks = vec![
            t(
                "/Music/wd/The Winery Dogs/1.mp3",
                "Winery Dogs",
                "The Winery Dogs",
                1,
            ),
            t(
                "/Music/wd/The Winery Dogs/2.mp3",
                "Winery Dogs",
                "The Winery Dogs",
                2,
            ),
        ];
        let albums = group_albums(&tracks, mf);
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].artist, "Winery Dogs");
    }

    #[test]
    fn multidisc_folders_fold_into_one_album() {
        let mf = Some("/Music");
        let tracks = vec![
            t(
                "/Music/Deep Purple/Made in Japan Disco 1/1.mp3",
                "Deep Purple",
                "Made in Japan",
                1,
            ),
            t(
                "/Music/Deep Purple/Made in Japan Disco 2/1.mp3",
                "Deep Purple",
                "Made in Japan",
                1,
            ),
        ];
        let albums = group_albums(&tracks, mf);
        assert_eq!(albums.len(), 1, "Disco 1/2 must fold");
        assert_eq!(albums[0].tracks.len(), 2);
    }

    #[test]
    fn different_albums_same_generic_tag_stay_separate() {
        // Beethoven case: same useless album tag, different folders => keep apart.
        let mf = Some("/Music");
        let tracks = vec![
            t(
                "/Music/Beethoven/Concerto Em Op64/1.mp3",
                "Beethoven",
                "Concertos",
                1,
            ),
            t(
                "/Music/Beethoven/Triple Concerto/1.mp3",
                "Beethoven",
                "Concertos",
                1,
            ),
        ];
        let albums = group_albums(&tracks, mf);
        assert_eq!(albums.len(), 2);
    }

    #[test]
    fn mixed_folder_is_subdivided_by_album_tag() {
        let mf = Some("/Music");
        let tracks = vec![
            t("/Music/OST/Expedition 33/1.mp3", "X", "Lumiere", 1),
            t("/Music/OST/Expedition 33/2.mp3", "Y", "Lune", 1),
            t("/Music/OST/Expedition 33/3.mp3", "X", "Lumiere", 2),
        ];
        let albums = group_albums(&tracks, mf);
        assert_eq!(albums.len(), 2);
        let names: std::collections::HashSet<_> = albums.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains("Lumiere") && names.contains("Lune"));
    }

    #[test]
    fn spelling_variants_in_same_folder_collapse() {
        let mf = Some("/Music");
        let tracks = vec![
            t(
                "/Music/Anthrax/Killers/1.mp3",
                "Anthrax",
                "Return Of The Killer A's",
                1,
            ),
            t(
                "/Music/Anthrax/Killers/2.mp3",
                "Anthrax",
                "Return of the Killer A's",
                2,
            ),
        ];
        let albums = group_albums(&tracks, mf);
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].tracks.len(), 2);
    }

    #[test]
    fn fs_fold_strips_verbatim_prefix_and_folds_case_on_windows() {
        // The rule documented in `fs_fold` itself: on Windows fold case and
        // strip the `\\?\` decoration; on Unix it's a no-op (case-sensitive).
        if cfg!(windows) {
            assert_eq!(fs_fold("C:"), fs_fold("c:"));
            assert_eq!(fs_fold(r"\\?\C:"), fs_fold("c:"));
            assert_eq!(fs_fold("Music"), fs_fold("music"));
            assert_eq!(fs_fold(r"\\?\C:\Music"), fs_fold(r"c:\music"));
            assert_ne!(fs_fold("Beatles"), fs_fold("Stones"));
        } else {
            // Unix is case-sensitive; folding must NOT collapse distinct
            // on-disk names.
            assert_ne!(fs_fold("Music"), fs_fold("music"));
            assert_eq!(fs_fold("Music"), "Music");
        }
    }

    #[cfg(windows)]
    #[test]
    fn rel_dirs_matches_across_case_and_verbatim_on_windows() {
        // Saved music_folder one spelling, scanned path another — must still
        // resolve to the same artist folder, not fall back to tag grouping.
        let mf = Some(r"C:\Users\X\Music");
        let dirs =
            super::rel_dirs(r"c:\users\x\music\Comes With the Fall\Album\1.mp3", mf).unwrap();
        assert_eq!(
            dirs.first().map(String::as_str),
            Some("Comes With the Fall")
        );
        let dirs_v =
            super::rel_dirs(r"\\?\C:\Users\X\Music\Comes With the Fall\Album\1.mp3", mf).unwrap();
        assert_eq!(
            dirs_v.first().map(String::as_str),
            Some("Comes With the Fall")
        );
    }

    #[test]
    fn album_artist_does_not_override_performer_label() {
        // INFIX tracks on a V Gundam OST have album_artist=千住明 (composer) but
        // artist=INFIX (performer). The artist view must show INFIX, not 千住明.
        let mf = Some("/Music");
        let mut track = t(
            "/Music/INFIX/V Gundam Score I/1.mp3",
            "INFIX",
            "V Gundam Score I",
            1,
        );
        track.album_artist = Some("千住明".to_string());
        let albums = group_albums(&[track], mf);
        assert_eq!(albums.len(), 1);
        assert_eq!(
            albums[0].artist, "INFIX",
            "album_artist must not override artist label"
        );
    }

    #[test]
    fn disc_num_orders_tracks_across_discs() {
        let mf = Some("/Music");
        let mut a = t("/Music/Band/Set Disc 2/1.mp3", "Band", "Set", 1);
        a.disc_num = Some(2);
        let mut b = t("/Music/Band/Set Disc 1/1.mp3", "Band", "Set", 1);
        b.disc_num = Some(1);
        let albums = group_albums(&[a, b], mf);
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].tracks[0].disc_num, Some(1));
        assert_eq!(albums[0].tracks[1].disc_num, Some(2));
    }

    #[test]
    fn differently_named_folders_with_same_specific_tag_merge_into_one_album() {
        // Classical rip pattern: one folder per disc content + one folder with
        // the full album title. All tracks share the same album tag, so they
        // should appear as one album (not two). The "at least one folder name
        // matches the tag" guard prevents generic-tag false merges.
        let mf = Some("/Music");
        let tracks = vec![
            t(
                "/Music/Beethoven/Concerto en Em Op64/1.mp3",
                "Beethoven",
                "Beethoven & Mendelssohn: Violin Concertos",
                1,
            ),
            t(
                "/Music/Beethoven/Beethoven & Mendelssohn Violin Concertos/1.mp3",
                "Beethoven",
                "Beethoven & Mendelssohn: Violin Concertos",
                1,
            ),
            t(
                "/Music/Beethoven/Beethoven & Mendelssohn Violin Concertos/2.mp3",
                "Beethoven",
                "Beethoven & Mendelssohn: Violin Concertos",
                2,
            ),
        ];
        let albums = group_albums(&tracks, mf);
        assert_eq!(albums.len(), 1, "should merge into one album");
        assert_eq!(albums[0].tracks.len(), 3);
    }

    #[test]
    fn single_level_folder_with_many_artists_is_various_artists() {
        // Rurouni Kenshin OST shape: every track sits directly under one
        // folder (no per-album subfolder) and the performer is different on
        // each track. The whole set must collapse into a single album
        // labelled "Various Artists", not silently adopt one of the twelve
        // performers as the bucket's artist.
        let mf = Some("/Music");
        let tracks = vec![
            t(
                "/Music/Rurouni Kenshin OST/01.mp3",
                "Judy and Mary",
                "Rurouni Kenshin OST",
                1,
            ),
            t(
                "/Music/Rurouni Kenshin OST/02.mp3",
                "The Yellow Monkey",
                "Rurouni Kenshin OST",
                2,
            ),
            t(
                "/Music/Rurouni Kenshin OST/03.mp3",
                "Curio",
                "Rurouni Kenshin OST",
                3,
            ),
            t(
                "/Music/Rurouni Kenshin OST/04.mp3",
                "T.M. Revolution",
                "Rurouni Kenshin OST",
                4,
            ),
        ];
        let albums = group_albums(&tracks, mf);
        assert_eq!(albums.len(), 1, "compilation folds into one album");
        assert_eq!(albums[0].artist, "Various Artists");
        assert_eq!(
            albums[0].tracks.len(),
            4,
            "every track must survive the compilation collapse"
        );
    }

    #[test]
    fn single_level_folder_with_one_artist_is_not_various() {
        // Same shape as the OST case, but every track is the same artist —
        // the all-tracks-same-folder guard must NOT trip Various Artists.
        let mf = Some("/Music");
        let tracks = vec![
            t(
                "/Music/Live Bootleg/01.mp3",
                "Soundgarden",
                "Live Bootleg",
                1,
            ),
            t(
                "/Music/Live Bootleg/02.mp3",
                "Soundgarden",
                "Live Bootleg",
                2,
            ),
        ];
        let albums = group_albums(&tracks, mf);
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].artist, "Soundgarden");
    }

    #[test]
    fn artist_subfolder_protects_album_from_various_misdetection() {
        // Artist/Album/track shape: even if for some reason multiple
        // performers showed up in the same album subfolder (collab record),
        // we must NOT relabel it Various — the folder structure says it is
        // a single release.
        let mf = Some("/Music");
        let tracks = vec![
            t(
                "/Music/Daft Punk/Random Access Memories/01.mp3",
                "Daft Punk",
                "RAM",
                1,
            ),
            t(
                "/Music/Daft Punk/Random Access Memories/02.mp3",
                "Daft Punk feat. Pharrell",
                "RAM",
                2,
            ),
        ];
        let albums = group_albums(&tracks, mf);
        assert_eq!(albums.len(), 1);
        assert_ne!(
            albums[0].artist, "Various Artists",
            "two-level folder must override per-track artist disagreement"
        );
    }
}
