use anyhow::Result;
use rodio::{Decoder, OutputStream, Sink};
use std::fs::File;
use std::io::BufReader;

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

    pub fn play(&self, path: &str) -> Result<()> {
        let file = BufReader::new(File::open(path)?);
        let source = Decoder::new(file)?;
        self.sink.clear();
        self.sink.append(source);
        self.sink.play();
        Ok(())
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
