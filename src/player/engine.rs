use crate::player::decoder::TolerantSource;
use anyhow::Result;
use rodio::{OutputStream, Sink, Source};

pub struct AudioEngine {
    _stream: OutputStream,
    sink: Sink,
}

impl AudioEngine {
    pub fn new() -> Result<Self> {
        let (_stream, handle) = OutputStream::try_default()?;
        let sink = Sink::try_new(&handle)?;
        Ok(Self { _stream, sink })
    }

    /// Play with a linear gain factor applied to the decoded audio.
    /// `gain` = 1.0 means no change; use `10_f32.powf(db / 20.0)` to convert dB.
    pub fn play_with_gain(&self, path: &str, gain: f32) -> Result<()> {
        let source = TolerantSource::open(path)?;
        self.sink.clear();
        self.sink.append(source.amplify(gain.max(0.0)));
        self.sink.play();
        Ok(())
    }

    pub fn stop(&self) {
        self.sink.clear();
        self.sink.pause();
    }

    pub fn pause(&self) {
        self.sink.pause();
    }

    pub fn resume(&self) {
        self.sink.play();
    }

    pub fn set_volume(&self, volume: f32) {
        self.sink.set_volume(volume.clamp(0.0, 1.0));
    }

    pub fn seek(&self, pos: std::time::Duration) {
        let _ = self.sink.try_seek(pos);
    }

    pub fn is_finished(&self) -> bool {
        self.sink.empty()
    }

    pub fn get_pos(&self) -> std::time::Duration {
        self.sink.get_pos()
    }
}
