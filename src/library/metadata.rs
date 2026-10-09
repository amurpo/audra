use std::path::PathBuf;

fn cache_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("audra")
        .join("covers")
}

/// Remove the on-disk downloaded cover cache directory entirely.
pub fn clear_cover_cache() {
    let _ = std::fs::remove_dir_all(cache_dir());
}

/// One pickable cover image, already downloaded, tagged with its origin.
pub struct CoverCandidate {
    pub source: String,
    pub data: Vec<u8>,
}

/// Stream album-cover candidates from every online source to `sink`, each the
/// moment it finishes downloading. The three sources (MusicBrainz/CAA,
/// TheAudioDB, iTunes) run in parallel, one thread each, so the picker no longer
/// waits for the slow iTunes search before showing MusicBrainz art — total
/// latency drops from their sum to their max. Only MusicBrainz is rate-limited,
/// and its global gate serialises that host across threads; the other hosts
/// tolerate the concurrent request. Candidates therefore arrive in completion
/// order, not source order. `sink` is shared across the worker threads, so it
/// must be `Sync`; the embedded-art candidate is emitted by the caller, which
/// owns the track path. Network-bound: must run off the UI thread.
pub fn fetch_album_cover_candidates(
    artist: &str,
    album: &str,
    sink: &(dyn Fn(CoverCandidate) + Sync),
) {
    let Some(client) = shared_client() else {
        return;
    };

    std::thread::scope(|s| {
        s.spawn(|| {
            musicbrainz_album_covers(client, artist, album, &|data| {
                sink(CoverCandidate {
                    source: "MusicBrainz".to_string(),
                    data,
                })
            });
        });
        s.spawn(|| {
            if let Some(data) = audiodb_album_cover(client, artist, album) {
                sink(CoverCandidate {
                    source: "TheAudioDB".to_string(),
                    data,
                });
            }
        });
        s.spawn(|| {
            itunes_album_covers(client, artist, album, &|data| {
                sink(CoverCandidate {
                    source: "iTunes".to_string(),
                    data,
                })
            });
        });
    });
}

/// Serialise calls to the MusicBrainz web service to at most one per ~1.1 s,
/// the rate the public server enforces (it answers 503 and can throttle the IP
/// if exceeded). Only `musicbrainz.org/ws/2` needs this: Cover Art Archive,
/// iTunes, Deezer and TheAudioDB are not gated, so every image download and
/// every non-MB lookup runs at full speed. The wait is measured against a
/// process-global last-call instant, so concurrent callers (the album slow lane
/// and the cover picker can both hit MB) stay serialised. The lock is
/// deliberately held across the sleep — that *is* the serialisation.
fn throttle_musicbrainz() {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    const MIN_INTERVAL: Duration = Duration::from_millis(1100);
    let mut last = LAST.lock().unwrap();
    if let Some(prev) = *last {
        let elapsed = prev.elapsed();
        if elapsed < MIN_INTERVAL {
            std::thread::sleep(MIN_INTERVAL - elapsed);
        }
    }
    *last = Some(Instant::now());
}

/// Query MusicBrainz for the release groups best matching the given artist and
/// album. Returns up to 3 release-group IDs, or an empty vec on failure.
///
/// A release group is the album itself, above its editions. Searching editions
/// directly made popular albums pick the wrong art: "...And Justice for All"
/// has 54 releases all scoring 100, and the first two were promo singles of the
/// title track. The query still runs against the *release* index, though —
/// releases carry their own localized credits ("Nobuo Uematsu" on a group
/// credited to 植松伸夫), which the release-group index does not match.
fn musicbrainz_release_groups(
    client: &reqwest::blocking::Client,
    artist: &str,
    album: &str,
) -> Vec<String> {
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let query = format!("release:\"{}\" AND artist:\"{}\"", esc(album), esc(artist));
    throttle_musicbrainz();
    let resp: Option<serde_json::Value> = client
        .get("https://musicbrainz.org/ws/2/release")
        .query(&[("query", query.as_str()), ("fmt", "json"), ("limit", "25")])
        .send()
        .ok()
        .and_then(|r| r.json().ok());
    resp.and_then(|r| {
        r["releases"]
            .as_array()
            .map(|rels| rank_release_groups(rels))
    })
    .unwrap_or_default()
}

/// Collapse release search hits into their release groups, best first: higher
/// score wins, and among equal scores a plain studio album beats a live or
/// compilation album, which beats an EP, then a single. MusicBrainz's own order
/// breaks the remaining ties (the sort is stable). Up to 3 IDs.
fn rank_release_groups(releases: &[serde_json::Value]) -> Vec<String> {
    let mut groups: Vec<(i64, u8, &str)> = Vec::new();
    for rel in releases {
        let group = &rel["release-group"];
        let Some(id) = group["id"].as_str() else {
            continue;
        };
        // Hits arrive best score first, so the first one seen for a group
        // already carries that group's best score.
        if groups.iter().any(|&(_, _, seen)| seen == id) {
            continue;
        }
        let score = rel["score"].as_i64().unwrap_or(0);
        groups.push((score, release_group_kind(group), id));
    }
    groups.sort_by_key(|&(score, kind, _)| (std::cmp::Reverse(score), kind));
    groups
        .into_iter()
        .take(3)
        .map(|(_, _, id)| id.to_string())
        .collect()
}

/// Preference rank of a release group's type; lower is preferred.
fn release_group_kind(group: &serde_json::Value) -> u8 {
    let secondary = group["secondary-types"]
        .as_array()
        .is_some_and(|types| !types.is_empty());
    let primary = group["primary-type"].as_str().unwrap_or("");
    if primary.eq_ignore_ascii_case("album") {
        if secondary {
            1
        } else {
            0
        }
    } else if primary.eq_ignore_ascii_case("ep") {
        2
    } else if primary.eq_ignore_ascii_case("single") {
        3
    } else {
        4
    }
}

/// Fetch the CoverArtArchive front-500 image of a release group, or None. It is
/// the cover MusicBrainz picks for the album as a whole, whichever edition it
/// comes from.
fn caa_front_500(client: &reqwest::blocking::Client, group_id: &str) -> Option<Vec<u8>> {
    let url = format!(
        "https://coverartarchive.org/release-group/{}/front-500",
        group_id
    );
    let resp = client.get(&url).send().ok()?;
    if !resp.status().is_success() {
        return None;
    }
    read_capped(resp)
}

/// Like `musicbrainz_album_cover` but emits the front art of the top few
/// matching release groups instead of stopping at the first hit, one image at a
/// time so the picker can show each as it arrives.
fn musicbrainz_album_covers(
    client: &reqwest::blocking::Client,
    artist: &str,
    album: &str,
    sink: &dyn Fn(Vec<u8>),
) {
    for group_id in musicbrainz_release_groups(client, artist, album) {
        // CAA is a separate, unthrottled host; the MB query above is already
        // gated, so the per-group art downloads run back to back.
        if let Some(bytes) = caa_front_500(client, &group_id) {
            sink(bytes);
        }
    }
}

/// iTunes album-art candidates for an explicit album title (the auto path
/// only ever queries by artist, so this lookup is picker-specific).
///
/// iTunes' free-text search is poor at titles: "Metallica ...And Justice for
/// All" returned other bands' records and not the album, although the catalog
/// carries it. So the artist is resolved first and the title matched against
/// their own discography. Only an artist iTunes does not know falls back to the
/// free-text search; a known artist without a matching album yields nothing,
/// since that search would only surface other artists' records.
fn itunes_album_covers(
    client: &reqwest::blocking::Client,
    artist: &str,
    album: &str,
    sink: &dyn Fn(Vec<u8>),
) {
    let urls = match itunes_artist_id(client, artist) {
        Some(id) => itunes_discography_art(client, id, album),
        None => itunes_search_art(client, artist, album),
    };
    for url in urls {
        if let Some(data) = download(client, &url.replace("100x100bb", "600x600bb")) {
            sink(data);
        }
    }
}

/// The iTunes ID of the artist whose name matches `artist` (ignoring case and
/// punctuation), or None. Only an exact name counts: a looser hit ("SaD -
/// Symphony and Metallica") would match the title against a tribute act.
fn itunes_artist_id(client: &reqwest::blocking::Client, artist: &str) -> Option<u64> {
    let resp: serde_json::Value = client
        .get("https://itunes.apple.com/search")
        .query(&[
            ("term", artist),
            ("media", "music"),
            ("entity", "musicArtist"),
            ("limit", "5"),
        ])
        .send()
        .ok()?
        .json()
        .ok()?;
    let wanted = match_key(artist);
    resp["results"]
        .as_array()?
        .iter()
        .find(|a| a["artistName"].as_str().map(match_key).as_deref() == Some(wanted.as_str()))
        .and_then(|a| a["artistId"].as_u64())
}

/// Artwork URLs (100px) of the artist's albums whose title matches `album`,
/// best match first, up to 5.
fn itunes_discography_art(
    client: &reqwest::blocking::Client,
    artist_id: u64,
    album: &str,
) -> Vec<String> {
    let id = artist_id.to_string();
    let resp: Option<serde_json::Value> = client
        .get("https://itunes.apple.com/lookup")
        .query(&[("id", id.as_str()), ("entity", "album"), ("limit", "200")])
        .send()
        .ok()
        .and_then(|r| r.json().ok());
    let Some(results) = resp.as_ref().and_then(|r| r["results"].as_array()) else {
        return Vec::new();
    };

    // The first result is the artist record itself; it has no collectionName,
    // so the filter drops it.
    let mut matches: Vec<(u8, &str)> = results
        .iter()
        .filter_map(|item| {
            let rank = title_match_rank(item["collectionName"].as_str()?, album)?;
            Some((rank, item["artworkUrl100"].as_str()?))
        })
        .collect();
    // Stable: iTunes' own order breaks ties.
    matches.sort_by_key(|&(rank, _)| rank);
    matches
        .into_iter()
        .take(5)
        .map(|(_, url)| url.to_string())
        .collect()
}

/// Artwork URLs (100px) from iTunes' free-text search for "artist album".
fn itunes_search_art(client: &reqwest::blocking::Client, artist: &str, album: &str) -> Vec<String> {
    let term = format!("{} {}", artist, album);
    let resp: Option<serde_json::Value> = client
        .get("https://itunes.apple.com/search")
        .query(&[
            ("term", term.as_str()),
            ("media", "music"),
            // For media=music the album entity is "album"; "musicAlbum" is
            // rejected with HTTP 400 ("Invalid value(s) for key(s):
            // [resultEntity]"), which is why iTunes never returned any art.
            ("entity", "album"),
            ("limit", "5"),
        ])
        .send()
        .ok()
        .and_then(|r| r.json().ok());

    resp.as_ref()
        .and_then(|r| r["results"].as_array())
        .map(|results| {
            results
                .iter()
                .filter_map(|item| item["artworkUrl100"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Fold a title or name for loose comparison: lowercase, punctuation as spaces,
/// whitespace collapsed. "...And Justice for All" and "…and justice for all"
/// both become "and justice for all".
fn match_key(s: &str) -> String {
    let spaced: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    spaced
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// How well a store title matches the wanted album; lower is better, None is
/// no match. 0 = same title, also once an edition tag is cut off ("Metallica
/// (Remastered)", "Da Funk - Single"); 1 = starts with it; 2 = contains it as
/// whole words.
fn title_match_rank(title: &str, wanted: &str) -> Option<u8> {
    let wanted = match_key(wanted);
    if wanted.is_empty() {
        return None;
    }
    let full = match_key(title);
    let cut = [" (", " [", " - "]
        .iter()
        .filter_map(|sep| title.find(sep))
        .min()
        .unwrap_or(title.len());
    let bare = match_key(&title[..cut]);

    if full == wanted || bare == wanted {
        Some(0)
    } else if format!("{full} ").starts_with(&format!("{wanted} ")) {
        Some(1)
    } else if format!(" {full} ").contains(&format!(" {wanted} ")) {
        Some(2)
    } else {
        None
    }
}

fn album_cache_path(artist: &str, album: &str) -> PathBuf {
    let key = format!("{}|{}", artist.to_lowercase(), album.to_lowercase());
    let hash = format!("{:x}", md5::compute(key.as_bytes()));
    cache_dir().join(format!("album_{}.jpg", hash))
}

fn artist_cache_path(artist: &str) -> PathBuf {
    let hash = format!("{:x}", md5::compute(artist.to_lowercase().as_bytes()));
    cache_dir().join(format!("artist_{}.jpg", hash))
}

fn write_cache(path: &PathBuf, data: &[u8]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, data);
}

/// Process-wide HTTP client, built once and shared across every metadata call.
///
/// Reusing one client keeps the connection pool alive between fetches, so the
/// TCP+TLS handshake to each host (MusicBrainz, Cover Art Archive, iTunes,
/// Deezer, TheAudioDB) is paid once and later requests ride keep-alive — plus
/// HTTP/2 where the host negotiates it over ALPN. Building a fresh client per
/// call, as before, threw that pool away on every album. The identifying
/// User-Agent MusicBrainz requires is set here; the 15 s timeout is the single
/// previous album value (artist photos used 10 s — the longer cap only means a
/// stalled photo waits a bit more before giving up). Returns `None` only if the
/// TLS backend fails to initialise, in which case there is no network path
/// anyway and callers already degrade to no artwork.
fn shared_client() -> Option<&'static reqwest::blocking::Client> {
    static CLIENT: std::sync::OnceLock<Option<reqwest::blocking::Client>> =
        std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .user_agent("audra/0.1 (https://github.com/amurpo/audra)")
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .ok()
        })
        .as_ref()
}

/// Cap on a single artwork download. Real album/artist art is a few hundred KB
/// at most; 15 MiB sits far above any genuine cover yet bounds a hostile or
/// zip-bombed response. Enforced on the *decompressed* stream, so it also caps
/// a small gzip that inflates past the limit — reqwest drops the Content-Length
/// header after inflating, so that header can't be trusted for this.
const MAX_DOWNLOAD_BYTES: u64 = 15 * 1024 * 1024;

/// Read a response body into memory, refusing anything over `MAX_DOWNLOAD_BYTES`.
/// Reads one byte past the cap so "exactly at cap" is told apart from "over cap".
fn read_capped(resp: reqwest::blocking::Response) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    resp.take(MAX_DOWNLOAD_BYTES + 1)
        .read_to_end(&mut buf)
        .ok()?;
    if buf.is_empty() || buf.len() as u64 > MAX_DOWNLOAD_BYTES {
        None
    } else {
        Some(buf)
    }
}

fn download(client: &reqwest::blocking::Client, url: &str) -> Option<Vec<u8>> {
    read_capped(client.get(url).send().ok()?)
}

/// Fetch an album cover: MusicBrainz → TheAudioDB.
pub fn fetch_album_cover(artist: &str, album: &str) -> Option<Vec<u8>> {
    let path = album_cache_path(artist, album);
    if path.exists() {
        return std::fs::read(&path).ok();
    }

    let client = shared_client()?;

    let data = musicbrainz_album_cover(client, artist, album)
        .or_else(|| audiodb_album_cover(client, artist, album))?;

    write_cache(&path, &data);
    Some(data)
}

fn musicbrainz_album_cover(
    client: &reqwest::blocking::Client,
    artist: &str,
    album: &str,
) -> Option<Vec<u8>> {
    for group_id in musicbrainz_release_groups(client, artist, album) {
        // CAA is unthrottled; the gating happens once, on the MB query above.
        if let Some(bytes) = caa_front_500(client, &group_id) {
            log::debug!("metadata: carátula MusicBrainz '{}' - '{}'", artist, album);
            return Some(bytes);
        }
    }
    None
}

fn audiodb_album_cover(
    client: &reqwest::blocking::Client,
    artist: &str,
    album: &str,
) -> Option<Vec<u8>> {
    let resp: serde_json::Value = client
        .get("https://www.theaudiodb.com/api/v1/json/123/searchalbum.php")
        .query(&[("s", artist), ("a", album)])
        .send()
        .ok()?
        .json()
        .ok()?;

    let albums = resp["album"]
        .as_array()
        .filter(|a| !a.is_empty())
        .or_else(|| {
            log::debug!(
                "metadata: TheAudioDB no encontró álbum '{}' - '{}'",
                artist,
                album
            );
            None
        })?;
    let url = albums[0]["strAlbumThumb"]
        .as_str()
        .or_else(|| {
            log::debug!(
                "metadata: TheAudioDB sin imagen para '{}' - '{}'",
                artist,
                album
            );
            None
        })?
        .to_string();
    let data = download(client, &url)?;
    log::debug!("metadata: carátula TheAudioDB '{}' - '{}'", artist, album);
    Some(data)
}

/// Persist a user-chosen artist photo into the same on-disk slot the
/// automatic fetch reads first, so the choice survives restarts.
pub fn set_artist_photo(artist: &str, data: &[u8]) {
    write_cache(&artist_cache_path(artist), data);
}

/// Stream artist-photo candidates to `sink`, each as it downloads. Deezer and
/// TheAudioDB run in parallel (one thread each), neither is rate-limited, so the
/// picker shows whichever lands first instead of waiting for both in series.
/// `sink` is shared across the threads, so it must be `Sync`. Network-bound:
/// must run off the UI thread.
pub fn fetch_artist_photo_candidates(artist: &str, sink: &(dyn Fn(CoverCandidate) + Sync)) {
    let Some(client) = shared_client() else {
        return;
    };

    std::thread::scope(|s| {
        s.spawn(|| {
            for url in deezer_artist_photos(client, artist) {
                if let Some(data) = download(client, &url) {
                    sink(CoverCandidate {
                        source: "Deezer".to_string(),
                        data,
                    });
                }
            }
        });
        s.spawn(|| {
            if let Some(url) = audiodb_artist_photo(client, artist) {
                if let Some(data) = download(client, &url) {
                    sink(CoverCandidate {
                        source: "TheAudioDB".to_string(),
                        data,
                    });
                }
            }
        });
    });
}

/// Read a usable photo URL from one Deezer `data[]` item, or None if the
/// hit is the empty-photo sentinel (`picture_xl` ends in `artist//`).
fn deezer_item_photo_url(item: &serde_json::Value) -> Option<String> {
    let url = item["picture_xl"]
        .as_str()
        .or_else(|| item["picture_big"].as_str())?;
    if url.contains("artist//") {
        return None;
    }
    Some(url.to_string())
}

/// Like `deezer_artist_photo` but returns the photo URL of the top few
/// matching artists instead of stopping at the first hit.
fn deezer_artist_photos(client: &reqwest::blocking::Client, artist: &str) -> Vec<String> {
    let resp: Option<serde_json::Value> = client
        .get("https://api.deezer.com/search/artist")
        .query(&[("q", artist), ("limit", "5")])
        .send()
        .ok()
        .and_then(|r| r.json().ok());

    resp.as_ref()
        .and_then(|r| r["data"].as_array())
        .map(|data| data.iter().filter_map(deezer_item_photo_url).collect())
        .unwrap_or_default()
}

/// Fetch an artist photo: Deezer → TheAudioDB. No iTunes fallback: iTunes only
/// has album art, so using it as an artist photo produced jarring artist/album
/// hybrids (an artist avatar showing one of their album covers). An artist with
/// no real photo on either source simply keeps the initials avatar.
pub fn fetch_artist_photo(artist: &str) -> Option<Vec<u8>> {
    let path = artist_cache_path(artist);
    if path.exists() {
        return std::fs::read(&path).ok();
    }

    let client = shared_client()?;

    let img_url =
        deezer_artist_photo(client, artist).or_else(|| audiodb_artist_photo(client, artist))?;

    let data = download(client, &img_url)?;
    write_cache(&path, &data);
    log::debug!("metadata: foto descargada para artista '{}'", artist);
    Some(data)
}

fn deezer_artist_photo(client: &reqwest::blocking::Client, artist: &str) -> Option<String> {
    let resp: serde_json::Value = client
        .get("https://api.deezer.com/search/artist")
        .query(&[("q", artist), ("limit", "1")])
        .send()
        .ok()?
        .json()
        .ok()?;

    let data = resp["data"]
        .as_array()
        .filter(|a| !a.is_empty())
        .or_else(|| {
            log::debug!("metadata: Deezer no encontró artista '{}'", artist);
            None
        })?;
    deezer_item_photo_url(&data[0]).or_else(|| {
        log::debug!("metadata: Deezer sin foto para '{}'", artist);
        None
    })
}

fn audiodb_artist_photo(client: &reqwest::blocking::Client, artist: &str) -> Option<String> {
    let resp: serde_json::Value = client
        .get("https://www.theaudiodb.com/api/v1/json/123/search.php")
        .query(&[("s", artist)])
        .send()
        .ok()?
        .json()
        .ok()?;

    resp["artists"]
        .as_array()
        .filter(|a| !a.is_empty())
        .and_then(|a| a[0]["strArtistThumb"].as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn release(group: &str, score: i64, primary: &str, secondary: &[&str]) -> serde_json::Value {
        json!({
            "score": score,
            "release-group": {
                "id": group,
                "primary-type": primary,
                "secondary-types": secondary,
            }
        })
    }

    #[test]
    fn album_group_beats_promo_single_on_equal_score() {
        // "...And Justice for All": promo single releases come back first.
        let hits = [
            release("single", 100, "Single", &[]),
            release("single", 100, "Single", &[]),
            release("album", 100, "Album", &[]),
            release("album", 100, "Album", &[]),
        ];
        assert_eq!(rank_release_groups(&hits), ["album", "single"]);
    }

    #[test]
    fn studio_album_beats_live_and_compilation() {
        let hits = [
            release("live", 100, "Album", &["Live"]),
            release("comp", 100, "Album", &["Compilation"]),
            release("studio", 100, "Album", &[]),
        ];
        assert_eq!(rank_release_groups(&hits), ["studio", "live", "comp"]);
    }

    #[test]
    fn higher_score_wins_over_type() {
        // A single is what was searched for ("Da Funk"); its remix albums
        // must not jump ahead just for being albums.
        let hits = [
            release("single", 100, "Single", &[]),
            release("remix", 89, "Album", &["Remix"]),
        ];
        assert_eq!(rank_release_groups(&hits), ["single", "remix"]);
    }

    #[test]
    fn release_groups_capped_at_three() {
        let hits = [
            release("a", 100, "Album", &[]),
            release("b", 95, "Album", &[]),
            release("c", 90, "Album", &[]),
            release("d", 85, "Album", &[]),
        ];
        assert_eq!(rank_release_groups(&hits), ["a", "b", "c"]);
    }

    #[test]
    fn match_key_folds_case_and_punctuation() {
        assert_eq!(match_key("...And Justice for All"), "and justice for all");
        assert_eq!(match_key("…and  Justice For All"), "and justice for all");
    }

    #[test]
    fn edition_tags_count_as_exact() {
        assert_eq!(
            title_match_rank(
                "...And Justice for All (Remastered)",
                "...And Justice for All"
            ),
            Some(0)
        );
        assert_eq!(title_match_rank("Da Funk - Single", "Da Funk"), Some(0));
        assert_eq!(
            title_match_rank(
                "FINAL FANTASY IX (Original Soundtrack)",
                "Final Fantasy IX Original Soundtrack"
            ),
            Some(0)
        );
    }

    #[test]
    fn exact_title_ranks_before_longer_ones() {
        let wanted = "Metallica";
        assert_eq!(title_match_rank("Metallica (Remastered)", wanted), Some(0));
        assert_eq!(
            title_match_rank("Metallica Through the Never", wanted),
            Some(1)
        );
        assert_eq!(title_match_rank("The Metallica Blacklist", wanted), Some(2));
    }

    #[test]
    fn title_match_needs_whole_words() {
        assert_eq!(title_match_rank("Metallicats", "Metallica"), None);
        assert_eq!(
            title_match_rank("Ride the Lightning", "...And Justice for All"),
            None
        );
        assert_eq!(title_match_rank("Anything", ""), None);
    }
}
