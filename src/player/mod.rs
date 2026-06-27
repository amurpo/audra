pub mod decoder;
pub mod engine;
pub mod mpris;
pub mod replaygain;

use crate::library::Track;
use anyhow::Result;
use engine::AudioEngine;
use rand::seq::SliceRandom;
use replaygain::{read_gain, ReplayGainMode};

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerState {
    Stopped,
    Playing,
    Paused,
}

pub struct Player {
    // `None` only in headless tests, where no audio device is opened and the
    // playback methods become no-ops. Production always holds `Some`.
    engine: Option<AudioEngine>,
    pub queue: Vec<Track>,
    pub index: Option<usize>,
    pub shuffle: bool,
    /// When set, reaching the end of the queue wraps back to the start instead
    /// of stopping ("repeat all"). There is no per-track repeat: it only made
    /// sense for a one-track album/artist/library, so it was dropped.
    pub repeat_all: bool,
    pub replaygain_mode: Option<ReplayGainMode>,
    pub state: PlayerState,
    pub volume: f32,
    shuffled_order: Vec<usize>,
    shuffle_cursor: usize,
}

impl Player {
    pub fn new() -> Result<Self> {
        Ok(Self {
            engine: Some(AudioEngine::new()?),
            queue: Vec::new(),
            index: None,
            shuffle: false,
            repeat_all: false,
            replaygain_mode: None,
            state: PlayerState::Stopped,
            volume: 0.5,
            shuffled_order: Vec::new(),
            shuffle_cursor: 0,
        })
    }

    pub fn load_queue(&mut self, tracks: Vec<Track>, start_index: usize) {
        self.queue = tracks;
        self.index = Some(start_index);
        self.shuffled_order = Vec::new();
        self.shuffle_cursor = 0;
    }

    // Builds a random playback order with the current track pinned at position
    // 0, so toggling shuffle mid-track keeps that track playing.
    pub fn reshuffle(&mut self) {
        self.build_shuffle_order(true);
    }

    /// Builds a fresh random order and resets the cursor. With `pin_current`
    /// the playing track is moved to position 0 (used when shuffle is switched
    /// on mid-playback); without it the pass is a plain shuffle, used to start a
    /// new lap under repeat-all so it doesn't replay the track that just ended.
    fn build_shuffle_order(&mut self, pin_current: bool) {
        let len = self.queue.len();
        let mut order: Vec<usize> = (0..len).collect();
        order.shuffle(&mut rand::thread_rng());
        if pin_current {
            if let Some(current) = self.index {
                if let Some(pos) = order.iter().position(|&i| i == current) {
                    order.swap(0, pos);
                }
            }
        }
        self.shuffled_order = order;
        self.shuffle_cursor = 0;
    }

    pub fn play_current(&mut self) -> Result<Option<&Track>> {
        let Some(idx) = self.index else {
            return Ok(None);
        };
        let Some(track) = self.queue.get(idx) else {
            return Ok(None);
        };
        let gain = match self.replaygain_mode {
            Some(mode) => read_gain(&track.path, mode),
            None => 1.0,
        };
        if let Some(engine) = &self.engine {
            if let Err(e) = engine.play_with_gain(&track.path, gain) {
                log::error!("playback failed for {}: {e:#}", track.path);
                return Err(e);
            }
            engine.set_volume(self.volume);
        }
        self.state = PlayerState::Playing;
        Ok(self.queue.get(idx))
    }

    pub fn stop(&mut self) {
        if let Some(engine) = &self.engine {
            engine.stop();
        }
        self.state = PlayerState::Stopped;
    }

    pub fn pause_resume(&mut self) {
        match self.state {
            PlayerState::Playing => {
                if let Some(engine) = &self.engine {
                    engine.pause();
                }
                self.state = PlayerState::Paused;
            }
            PlayerState::Paused => {
                if let Some(engine) = &self.engine {
                    engine.resume();
                }
                self.state = PlayerState::Playing;
            }
            _ => {}
        }
    }

    pub fn next(&mut self) -> Result<Option<&Track>> {
        let len = self.queue.len();
        if len == 0 {
            return Ok(None);
        }

        let next_idx = if self.shuffle {
            if self.shuffled_order.is_empty() {
                self.reshuffle();
            }
            if self.shuffle_cursor + 1 >= self.shuffled_order.len() {
                // End of the shuffled pass. Under repeat-all, start a fresh
                // random lap; otherwise report "no next track" with `None`
                // WITHOUT advancing the cursor past the last track (that
                // overflow is what made `previous()` index out of bounds and
                // abort the app). `next` is a pure query: the caller decides
                // what "no next track" means — a manual skip keeps the current
                // track playing, the auto-advance timer stops.
                if self.repeat_all {
                    self.build_shuffle_order(false);
                } else {
                    return Ok(None);
                }
            } else {
                self.shuffle_cursor += 1;
            }
            self.shuffled_order[self.shuffle_cursor]
        } else {
            let current = self.index.unwrap_or(0);
            if current + 1 >= len {
                // Wrap to the start under repeat-all, else report "no next
                // track" (same pure-query contract as the shuffle branch).
                if self.repeat_all {
                    0
                } else {
                    return Ok(None);
                }
            } else {
                current + 1
            }
        };

        self.index = Some(next_idx);
        self.play_current()
    }

    pub fn previous(&mut self) -> Result<Option<&Track>> {
        let len = self.queue.len();
        if len == 0 {
            return Ok(None);
        }

        let prev_idx = if self.shuffle {
            if self.shuffled_order.is_empty() {
                self.reshuffle();
            }
            // Step the shuffle cursor back so it stays in sync with `index`;
            // at the start, stay on the first shuffled track. Clamp to a valid
            // slot *before* stepping back: a cursor left at the end (playback
            // just stopped on the last track) must never index past the order,
            // which previously aborted the whole app from the GTK click handler.
            let last = self.shuffled_order.len().saturating_sub(1);
            self.shuffle_cursor = self.shuffle_cursor.min(last).saturating_sub(1);
            self.shuffled_order[self.shuffle_cursor]
        } else {
            self.index.map(|i| i.saturating_sub(1)).unwrap_or(0)
        };

        self.index = Some(prev_idx);
        self.play_current()
    }

    pub fn seek(&self, secs: f64) {
        if let Some(engine) = &self.engine {
            engine.seek(std::time::Duration::from_secs_f64(secs));
        }
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v;
        if let Some(engine) = &self.engine {
            engine.set_volume(v);
        }
    }

    pub fn current_track(&self) -> Option<&Track> {
        self.index.and_then(|i| self.queue.get(i))
    }

    pub fn is_finished(&self) -> bool {
        self.engine.as_ref().is_none_or(|e| e.is_finished())
    }

    pub fn position(&self) -> std::time::Duration {
        self.engine
            .as_ref()
            .map_or(std::time::Duration::ZERO, |e| e.get_pos())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_track(i: usize) -> Track {
        Track {
            id: Some(i as i64),
            path: format!("/m/{i}.mp3"),
            title: Some(format!("t{i}")),
            artist: Some("A".into()),
            album: Some("X".into()),
            track_num: Some(i as i64),
            duration_secs: Some(100),
            disc_num: None,
            album_artist: None,
            mtime: None,
        }
    }

    /// CI runners have no audio device (and opening one can hang), so these
    /// tests build a `Player` with no engine. They exercise only queue/shuffle
    /// bookkeeping; the playback methods are no-ops when `engine` is `None`.
    fn headless_player() -> Player {
        Player {
            engine: None,
            queue: Vec::new(),
            index: None,
            shuffle: false,
            repeat_all: false,
            replaygain_mode: None,
            state: PlayerState::Stopped,
            volume: 0.5,
            shuffled_order: Vec::new(),
            shuffle_cursor: 0,
        }
    }

    #[test]
    fn defaults_are_sane() {
        let p = headless_player();
        assert_eq!(p.state, PlayerState::Stopped);
        assert!(!p.shuffle);
        assert!(!p.repeat_all);
        assert_eq!(p.index, None);
        assert!(p.queue.is_empty());
        assert!(p.current_track().is_none());
    }

    #[test]
    fn load_queue_sets_index_and_resets_shuffle_state() {
        let mut p = headless_player();
        p.load_queue((0..5).map(mk_track).collect(), 2);
        assert_eq!(p.queue.len(), 5);
        assert_eq!(p.index, Some(2));
        assert_eq!(
            p.current_track().map(|t| t.path.clone()),
            Some("/m/2.mp3".into())
        );

        // A stale shuffled order must be cleared by load_queue.
        p.shuffle = true;
        p.reshuffle();
        assert!(!p.shuffled_order.is_empty());
        p.load_queue((0..3).map(mk_track).collect(), 0);
        assert!(p.shuffled_order.is_empty());
        assert_eq!(p.shuffle_cursor, 0);
    }

    #[test]
    fn reshuffle_is_a_permutation_with_current_first() {
        let mut p = headless_player();
        p.load_queue((0..10).map(mk_track).collect(), 4);
        p.reshuffle();

        assert_eq!(p.shuffled_order.len(), 10);
        assert_eq!(p.shuffle_cursor, 0);
        // The currently playing index is moved to the front.
        assert_eq!(p.shuffled_order[0], 4);
        // Every original index appears exactly once.
        let mut sorted = p.shuffled_order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn reshuffle_on_empty_queue_is_safe() {
        let mut p = headless_player();
        p.load_queue(vec![], 0);
        p.reshuffle();
        assert!(p.shuffled_order.is_empty());
    }

    #[test]
    fn set_volume_updates_state() {
        let mut p = headless_player();
        p.set_volume(0.25);
        assert_eq!(p.volume, 0.25);
    }

    #[test]
    fn next_at_end_of_queue_is_a_noop_and_keeps_playing() {
        let mut p = headless_player();
        p.load_queue((0..3).map(mk_track).collect(), 2);
        let _ = p.play_current(); // pretend the last track is playing
        assert_eq!(p.state, PlayerState::Playing);
        // A manual skip past the end must not advance and must not stop
        // playback: the current track keeps sounding. `next` only reports "no
        // next track" via None — stopping at the end of the disc is the
        // auto-advance timer's job, not next()'s.
        let result = p.next().unwrap();
        assert!(result.is_none());
        assert_eq!(p.index, Some(2), "index must remain unchanged at boundary");
        assert_eq!(
            p.state,
            PlayerState::Playing,
            "next at the end must not kill playback"
        );
    }

    #[test]
    fn previous_at_start_clamps_to_index_zero() {
        let mut p = headless_player();
        p.load_queue((0..3).map(mk_track).collect(), 0);
        // previous() from index 0 saturates — index stays at 0.
        let _ = p.previous(); // may Err (file doesn't exist), ignore result
        assert_eq!(p.index, Some(0));
    }

    #[test]
    fn next_on_empty_queue_returns_none() {
        let mut p = headless_player();
        assert!(p.next().unwrap().is_none());
    }

    #[test]
    fn shuffle_next_past_end_keeps_cursor_in_range_so_previous_is_safe() {
        // Repro for the abort: in shuffle mode, hammering "next" past the end
        // used to push shuffle_cursor beyond the order's length, after which
        // `previous()` indexed out of bounds and aborted the whole app via a
        // non-unwinding panic in the GTK click trampoline.
        let mut p = headless_player();
        p.shuffle = true;
        p.load_queue((0..3).map(mk_track).collect(), 0);
        p.reshuffle();

        // Advance well past the end (more clicks than the queue is long).
        for _ in 0..10 {
            let _ = p.next();
        }
        // The cursor must never run past the last valid index — that overflow
        // is what made `previous()` index out of bounds and abort the app.
        assert!(
            p.shuffle_cursor < p.shuffled_order.len(),
            "cursor {} escaped order len {}",
            p.shuffle_cursor,
            p.shuffled_order.len()
        );

        // The crash itself: this must not panic, and must land on a real track.
        let _ = p.previous();
        assert!(p.shuffle_cursor < p.shuffled_order.len());
        assert!(p.index.is_some_and(|i| i < p.queue.len()));
    }

    #[test]
    fn next_at_end_with_repeat_all_wraps_to_start() {
        let mut p = headless_player();
        p.repeat_all = true;
        p.load_queue((0..3).map(mk_track).collect(), 2);
        // From the last track, next wraps back to the first instead of stopping.
        let result = p.next().unwrap();
        assert!(result.is_some());
        assert_eq!(p.index, Some(0));
    }

    #[test]
    fn shuffle_next_at_end_with_repeat_all_starts_a_fresh_pass() {
        let mut p = headless_player();
        p.shuffle = true;
        p.repeat_all = true;
        p.load_queue((0..4).map(mk_track).collect(), 0);
        p.reshuffle();

        // Exhaust the shuffled pass (queue is 4 long, current sits at cursor 0).
        for _ in 0..3 {
            let _ = p.next();
        }
        assert_eq!(p.shuffle_cursor, p.shuffled_order.len() - 1);

        // One more next starts a new lap: the cursor resets and the order is a
        // full, fresh permutation of every index.
        let _ = p.next();
        assert_eq!(p.shuffle_cursor, 0, "wrapped to a fresh pass");
        let mut sorted = p.shuffled_order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..4).collect::<Vec<_>>());
        assert!(p.index.is_some_and(|i| i < p.queue.len()));
    }
}
