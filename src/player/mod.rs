pub mod engine;

use crate::library::Track;
use engine::AudioEngine;
use anyhow::Result;
use rand::seq::SliceRandom;

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

    // Genera un orden aleatorio poniendo la pista actual en la posición 0.
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
        let Some(idx) = self.index else { return Ok(None) };
        let Some(track) = self.queue.get(idx) else { return Ok(None) };
        self.engine.play(&track.path)?;
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
        if len == 0 { return Ok(None); }

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
        if len == 0 { return Ok(None); }
        let prev_idx = self.index.map(|i| i.saturating_sub(1)).unwrap_or(0);
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
