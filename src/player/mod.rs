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
    engine: AudioEngine,
    pub queue: Vec<Track>,
    pub index: Option<usize>,
    pub shuffle: bool,
    pub repeat_one: bool,
    pub replaygain_mode: Option<ReplayGainMode>,
    pub state: PlayerState,
    pub volume: f32,
    shuffled_order: Vec<usize>,
    shuffle_cursor: usize,
}

impl Player {
    pub fn new() -> Result<Self> {
        Ok(Self {
            engine: AudioEngine::new()?,
            queue: Vec::new(),
            index: None,
            shuffle: false,
            repeat_one: false,
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

    // Builds a random playback order with the current track pinned at position 0.
    pub fn reshuffle(&mut self) {
        let len = self.queue.len();
        let mut order: Vec<usize> = (0..len).collect();
        order.shuffle(&mut rand::thread_rng());
        if let Some(current) = self.index {
            if let Some(pos) = order.iter().position(|&i| i == current) {
                order.swap(0, pos);
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
        self.engine.play_with_gain(&track.path, gain)?;
        self.engine.set_volume(self.volume);
        self.state = PlayerState::Playing;
        Ok(self.queue.get(idx))
    }

    pub fn pause_resume(&mut self) {
        match self.state {
            PlayerState::Playing => {
                self.engine.pause();
                self.state = PlayerState::Paused;
            }
            PlayerState::Paused => {
                self.engine.resume();
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
            self.shuffle_cursor += 1;
            if self.shuffle_cursor >= self.shuffled_order.len() {
                self.state = PlayerState::Stopped;
                return Ok(None);
            }
            self.shuffled_order[self.shuffle_cursor]
        } else {
            let current = self.index.unwrap_or(0);
            if current + 1 >= len {
                self.state = PlayerState::Stopped;
                return Ok(None);
            }
            current + 1
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
            // at the start, stay on the first shuffled track.
            self.shuffle_cursor = self.shuffle_cursor.saturating_sub(1);
            self.shuffled_order[self.shuffle_cursor]
        } else {
            self.index.map(|i| i.saturating_sub(1)).unwrap_or(0)
        };

        self.index = Some(prev_idx);
        self.play_current()
    }

    pub fn seek(&self, secs: f64) {
        self.engine.seek(std::time::Duration::from_secs_f64(secs));
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v;
        self.engine.set_volume(v);
    }

    pub fn current_track(&self) -> Option<&Track> {
        self.index.and_then(|i| self.queue.get(i))
    }

    pub fn is_finished(&self) -> bool {
        self.engine.is_finished()
    }

    pub fn position(&self) -> std::time::Duration {
        self.engine.get_pos()
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
        }
    }

    /// CI runners have no audio device, so `Player::new()` fails there. These
    /// tests exercise only queue/shuffle bookkeeping (no playback), and skip
    /// cleanly when no output device is available.
    fn player_or_skip() -> Option<Player> {
        match Player::new() {
            Ok(p) => Some(p),
            Err(_) => {
                eprintln!("skipping: no audio output device available");
                None
            }
        }
    }

    #[test]
    fn defaults_are_sane() {
        let Some(p) = player_or_skip() else { return };
        assert_eq!(p.state, PlayerState::Stopped);
        assert!(!p.shuffle);
        assert!(!p.repeat_one);
        assert_eq!(p.index, None);
        assert!(p.queue.is_empty());
        assert!(p.current_track().is_none());
    }

    #[test]
    fn load_queue_sets_index_and_resets_shuffle_state() {
        let Some(mut p) = player_or_skip() else {
            return;
        };
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
        let Some(mut p) = player_or_skip() else {
            return;
        };
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
        let Some(mut p) = player_or_skip() else {
            return;
        };
        p.load_queue(vec![], 0);
        p.reshuffle();
        assert!(p.shuffled_order.is_empty());
    }

    #[test]
    fn set_volume_updates_state() {
        let Some(mut p) = player_or_skip() else {
            return;
        };
        p.set_volume(0.25);
        assert_eq!(p.volume, 0.25);
    }

    #[test]
    fn next_at_end_of_queue_stops_and_returns_none() {
        let Some(mut p) = player_or_skip() else {
            return;
        };
        p.load_queue((0..3).map(mk_track).collect(), 2);
        // next() from the last track must stop playback, not advance.
        let result = p.next().unwrap();
        assert!(result.is_none());
        assert_eq!(p.state, PlayerState::Stopped);
        assert_eq!(p.index, Some(2), "index must remain unchanged at boundary");
    }

    #[test]
    fn previous_at_start_clamps_to_index_zero() {
        let Some(mut p) = player_or_skip() else {
            return;
        };
        p.load_queue((0..3).map(mk_track).collect(), 0);
        // previous() from index 0 saturates — index stays at 0.
        let _ = p.previous(); // may Err (file doesn't exist), ignore result
        assert_eq!(p.index, Some(0));
    }

    #[test]
    fn next_on_empty_queue_returns_none() {
        let Some(mut p) = player_or_skip() else {
            return;
        };
        assert!(p.next().unwrap().is_none());
    }
}
