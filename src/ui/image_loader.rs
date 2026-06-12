//! Two-pass async image pipeline shared by every grid that fetches artwork.
//!
//! Unifies what used to be `start_cover_fetch` (albums) and `start_photo_fetch`
//! (artists): a worker thread produces decoded pixel data and sends it over a
//! channel; a future on the GTK main loop applies textures to widgets as each
//! result arrives — no polling timer, no per-poll latency.
//!
//! Why two passes:
//! - **Fast lane** runs over every item with no delay; ideal for local sources
//!   (DB cache, embedded cover art).
//! - **Slow lane** runs only over the items that missed the fast lane, with a
//!   configurable delay between calls; ideal for rate-limited network sources
//!   (Last.fm).
//!
//! The fast lane returns [`FetchOutcome`] instead of `Option` because callers
//! need a third state — *explicit skip* — for items the user has cleared on
//! purpose (empty bytes in the DB). Without it, those items would loop
//! forever through the slow lane.
use glib;
use gtk4::gdk;

use crate::ui::image_utils::{pixels_to_texture, scale_to_pixels};

/// Fast-lane outcome. `Miss` defers to the slow lane; `Skip` does not.
pub enum FetchOutcome {
    Got(Vec<u8>),
    Skip,
    Miss,
}

pub struct ImagePipelineConfig {
    /// Square edge in pixels used for the in-memory scaled bitmap.
    pub target_size: i32,
    /// Delay between slow-lane calls. Set to 0 to disable rate limiting.
    pub slow_delay_ms: u64,
}

/// Optional slow-lane fetcher. Boxed because `Option<impl Fn>` would force
/// callers that don't have a slow lane to spell out an unused type parameter.
pub type SlowFetcher<K> = Box<dyn Fn(&K) -> Option<Vec<u8>> + Send>;

/// One ready-to-paint result on the worker side: `(key, pixels, rowstride, has_alpha)`.
type ScaledResult<K> = (K, Vec<u8>, i32, bool);

/// Run the pipeline.
///
/// * `fetch_fast` — called once per item on the worker thread. Returns
///   [`FetchOutcome`] (see above).
/// * `fetch_slow` — optional. If present, items that returned `Miss` go
///   through this second pass with `slow_delay_ms` between calls.
/// * `apply` — runs on the GTK thread when each result lands. Receives the
///   item key and the ready `gdk::Texture`; the caller decides which widget
///   to update.
pub fn run<K, FF, AP>(
    items: Vec<K>,
    config: ImagePipelineConfig,
    fetch_fast: FF,
    fetch_slow: Option<SlowFetcher<K>>,
    apply: AP,
) where
    K: Send + Clone + 'static,
    FF: Fn(&K) -> FetchOutcome + Send + 'static,
    AP: Fn(&K, gdk::Texture) + 'static,
{
    if items.is_empty() {
        return;
    }

    let (tx, rx) = async_channel::unbounded::<ScaledResult<K>>();
    let target_size = config.target_size;
    let slow_delay = std::time::Duration::from_millis(config.slow_delay_ms);

    std::thread::spawn(move || {
        let mut leftover: Vec<K> = Vec::new();

        for item in &items {
            match fetch_fast(item) {
                FetchOutcome::Got(bytes) => {
                    if let Some((pixels, stride, alpha)) = scale_to_pixels(&bytes, target_size) {
                        let _ = tx.send_blocking((item.clone(), pixels, stride, alpha));
                    }
                }
                FetchOutcome::Skip => {}
                FetchOutcome::Miss => {
                    if fetch_slow.is_some() {
                        leftover.push(item.clone());
                    }
                }
            }
        }

        if let Some(slow) = fetch_slow {
            for item in leftover {
                if !slow_delay.is_zero() {
                    std::thread::sleep(slow_delay);
                }
                if let Some(bytes) = slow(&item) {
                    if let Some((pixels, stride, alpha)) = scale_to_pixels(&bytes, target_size) {
                        let _ = tx.send_blocking((item, pixels, stride, alpha));
                    }
                }
            }
        }
        // Dropping `tx` here closes the channel and ends the UI future below.
    });

    glib::spawn_future_local(async move {
        while let Ok((k, pixels, stride, alpha)) = rx.recv().await {
            let texture = pixels_to_texture(pixels, stride, alpha, target_size);
            apply(&k, texture);
        }
    });
}
