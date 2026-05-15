use std::fs::File;
use std::io::BufReader;
use std::sync::{Arc, Mutex};
use anyhow::Result;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};

pub struct AudioEngine {
    _stream: OutputStream,
    handle: OutputStreamHandle,
    sink: Arc<Mutex<Option<Sink>>>,
}

impl AudioEngine {
    pub fn new() -> Result<Self> {
        let (_stream, handle) = OutputStream::try_default()?;
        Ok(Self {
            _stream,
            handle,
            sink: Arc::new(Mutex::new(None)),
        })
    }

    pub fn play(&self, path: &str) -> Result<()> {
        let file = BufReader::new(File::open(path)?);
        let source = Decoder::new(file)?;

        let sink = Sink::try_new(&self.handle)?;
        sink.append(source);
        sink.play();

        *self.sink.lock().unwrap() = Some(sink);
        Ok(())
    }

    pub fn pause(&self) {
        if let Some(sink) = self.sink.lock().unwrap().as_ref() {
            sink.pause();
        }
    }

    pub fn resume(&self) {
        if let Some(sink) = self.sink.lock().unwrap().as_ref() {
            sink.play();
        }
    }

    pub fn stop(&self) {
        if let Some(sink) = self.sink.lock().unwrap().as_ref() {
            sink.stop();
        }
    }

    pub fn set_volume(&self, volume: f32) {
        if let Some(sink) = self.sink.lock().unwrap().as_ref() {
            sink.set_volume(volume.clamp(0.0, 1.0));
        }
    }

    pub fn is_finished(&self) -> bool {
        self.sink
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.empty())
            .unwrap_or(true)
    }

    pub fn is_paused(&self) -> bool {
        self.sink
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.is_paused())
            .unwrap_or(false)
    }

    pub fn get_pos(&self) -> std::time::Duration {
        self.sink
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.get_pos())
            .unwrap_or_default()
    }
}
