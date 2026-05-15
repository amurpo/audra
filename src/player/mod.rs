pub mod engine;

use crate::library::Track;
use engine::AudioEngine;
use anyhow::Result;

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
            volume: 1.0,
        })
    }

    pub fn load_queue(&mut self, tracks: Vec<Track>, start_index: usize) {
        self.queue = tracks;
        self.index = Some(start_index);
    }

    pub fn play_current(&mut self) -> Result<Option<&Track>> {
        let Some(idx) = self.index else { return Ok(None) };
        let Some(track) = self.queue.get(idx) else { return Ok(None) };
        self.engine.play(&track.path)?;
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
            rand::random::<usize>() % len
        } else {
            self.index.map(|i| (i + 1) % len).unwrap_or(0)
        };
        self.index = Some(next_idx);
        self.play_current()
    }

    pub fn previous(&mut self) -> Result<Option<&Track>> {
        let len = self.queue.len();
        if len == 0 { return Ok(None); }
        let prev_idx = self.index
            .map(|i| if i == 0 { len - 1 } else { i - 1 })
            .unwrap_or(0);
        self.index = Some(prev_idx);
        self.play_current()
    }

    pub fn stop(&mut self) {
        self.engine.stop();
        self.state = PlayerState::Stopped;
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
