//! Audio output: one rodio `Player` feeding the default device's mixer.
//!
//! The device can fail under us (PipeWire still settling at login, a USB or
//! Bluetooth sink unplugged). The stream's error callback only raises a flag;
//! the next call from the GTK thread (the 500 ms UI tick at the latest) drops
//! the dead stream, opens a new one and resumes the current track where it was.
//!
//! Some rodio calls wait for the audio thread to answer (`Player::clear`,
//! `Player::try_seek`). A dead stream never answers, so those calls would
//! freeze the UI for good. Hence: every track gets a fresh `Player` instead of
//! `clear()`, and `try_seek` only runs on a stream that hasn't failed.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use rodio::cpal::traits::HostTrait;
use rodio::{CpalError, CpalErrorKind, DeviceSinkBuilder, DeviceTrait, MixerDeviceSink, Source};

use crate::player::decoder::TolerantSource;

/// Minimum gap between attempts to reopen a failed device.
const RETRY_EVERY: Duration = Duration::from_secs(2);

struct Output {
    player: rodio::Player,
    device: MixerDeviceSink,
    /// Raised by this stream's error callback. One flag per stream, so a late
    /// error from a stream being torn down can't condemn its replacement.
    failed: Arc<AtomicBool>,
}

/// The track that should be sounding, kept apart from the device so it can be
/// replayed onto a new output after a failure.
struct Loaded {
    path: String,
    gain: f32,
}

pub struct AudioEngine {
    /// `None` while the device is gone and couldn't be reopened yet.
    out: RefCell<Option<Output>>,
    loaded: RefCell<Option<Loaded>>,
    paused: Cell<bool>,
    /// Player volume, already on the perceptual taper.
    volume: Cell<f32>,
    /// Added to the player's position: after a reopen the track is seeked
    /// before the new player owns it, so the player's own clock starts at 0.
    pos_offset: Cell<Duration>,
    /// Position to report, and to resume from, while there is no output.
    last_pos: Cell<Duration>,
    next_retry: Cell<Instant>,
}

impl AudioEngine {
    pub fn new() -> Result<Self> {
        Ok(Self {
            out: RefCell::new(Some(open_output(1.0)?)),
            loaded: RefCell::new(None),
            paused: Cell::new(false),
            volume: Cell::new(1.0),
            pos_offset: Cell::new(Duration::ZERO),
            last_pos: Cell::new(Duration::ZERO),
            next_retry: Cell::new(Instant::now()),
        })
    }

    /// Play with a linear gain factor applied to the decoded audio.
    /// `gain` = 1.0 means no change; use `10_f32.powf(db / 20.0)` to convert dB.
    pub fn play_with_gain(&self, path: &str, gain: f32) -> Result<()> {
        self.recover();
        let source = TolerantSource::open(path)?;
        let gain = gain.max(0.0);
        *self.loaded.borrow_mut() = Some(Loaded {
            path: path.to_owned(),
            gain,
        });
        self.paused.set(false);
        self.pos_offset.set(Duration::ZERO);
        self.last_pos.set(Duration::ZERO);
        // Without an output the track is only remembered; recover() starts it
        // once the device is back.
        if let Some(out) = self.out.borrow_mut().as_mut() {
            // Dropping the old player stops it without waiting on the audio thread.
            out.player = new_player(&out.device, self.volume.get());
            out.player.append(source.amplify(gain));
        }
        Ok(())
    }

    pub fn stop(&self) {
        self.recover();
        *self.loaded.borrow_mut() = None;
        self.pos_offset.set(Duration::ZERO);
        self.last_pos.set(Duration::ZERO);
        if let Some(out) = self.out.borrow_mut().as_mut() {
            out.player = new_player(&out.device, self.volume.get());
        }
    }

    pub fn pause(&self) {
        self.recover();
        self.paused.set(true);
        if let Some(out) = self.out.borrow().as_ref() {
            out.player.pause();
        }
    }

    pub fn resume(&self) {
        self.recover();
        self.paused.set(false);
        if let Some(out) = self.out.borrow().as_ref() {
            out.player.play();
        }
    }

    pub fn set_volume(&self, volume: f32) {
        let v = volume.clamp(0.0, 1.0);
        // Perceptual (cubic) taper, matching PulseAudio: a linear slider sounds
        // linear to the ear instead of saturating across the lower half. Without
        // it, 0.5 is only -6 dB and still plays loud, which is especially obvious
        // on Windows where WASAPI applies no perceptual curve of its own.
        self.volume.set(v * v * v);
        self.recover();
        if let Some(out) = self.out.borrow().as_ref() {
            out.player.set_volume(self.volume.get());
        }
    }

    pub fn seek(&self, pos: Duration) {
        self.recover();
        match self.out.borrow().as_ref() {
            Some(out) if !out.failed.load(Ordering::Acquire) => {
                if out.player.try_seek(pos).is_ok() {
                    // The player's clock now reads `pos` by itself.
                    self.pos_offset.set(Duration::ZERO);
                }
            }
            // No live stream to wait on: resume from here once it's back.
            _ => self.last_pos.set(pos),
        }
    }

    pub fn is_finished(&self) -> bool {
        self.recover();
        match self.out.borrow().as_ref() {
            Some(out) => out.player.empty(),
            // No device: hold the current track rather than letting the UI
            // tick skip through the whole queue.
            None => self.loaded.borrow().is_none(),
        }
    }

    pub fn get_pos(&self) -> Duration {
        self.recover();
        match self.out.borrow().as_ref() {
            Some(out) => out.player.get_pos() + self.pos_offset.get(),
            None => self.last_pos.get(),
        }
    }

    /// Replace a failed output. Runs at the start of every public method, so
    /// nothing below ever touches a dead stream.
    fn recover(&self) {
        let failed = self
            .out
            .borrow()
            .as_ref()
            .is_some_and(|o| o.failed.load(Ordering::Acquire));
        if failed {
            // Taking it out drops it right here, which also stops a cpal
            // thread that might be spinning on errors.
            let dead = self.out.borrow_mut().take();
            if let Some(dead) = dead {
                self.last_pos
                    .set(dead.player.get_pos() + self.pos_offset.get());
            }
        }
        if self.out.borrow().is_some() || Instant::now() < self.next_retry.get() {
            return;
        }
        self.next_retry.set(Instant::now() + RETRY_EVERY);
        match self.reopen() {
            Ok(out) => {
                *self.out.borrow_mut() = Some(out);
                log::info!("audio output reopened");
            }
            Err(e) => log::debug!("audio output still unavailable: {e:#}"),
        }
    }

    /// Open a new output and replay the current track onto it, at the last
    /// known position and with the pause state it had.
    fn reopen(&self) -> Result<Output> {
        let out = open_output(self.volume.get())?;
        self.pos_offset.set(Duration::ZERO);
        if self.paused.get() {
            // Before appending, so not even a few ms play.
            out.player.pause();
        }
        let mut loaded = self.loaded.borrow_mut();
        if let Some(track) = loaded.as_ref() {
            match TolerantSource::open(&track.path) {
                Ok(mut source) => {
                    let pos = self.last_pos.get();
                    // Seek before the player owns the source: Player::try_seek
                    // would wait on the audio thread.
                    if source.try_seek(pos).is_ok() {
                        self.pos_offset.set(pos);
                    }
                    out.player.append(source.amplify(track.gain));
                }
                Err(e) => {
                    // The device is fine but the file isn't (moved, unplugged
                    // drive): let the UI move on to the next track.
                    log::error!(
                        "could not resume {} after reopening audio: {e:#}",
                        track.path
                    );
                    *loaded = None;
                }
            }
        }
        Ok(out)
    }
}

fn new_player(device: &MixerDeviceSink, volume: f32) -> rodio::Player {
    let player = rodio::Player::connect_new(device.mixer());
    player.set_volume(volume);
    player
}

fn open_output(volume: f32) -> Result<Output> {
    let failed = Arc::new(AtomicBool::new(false));
    let mut device = open_device(error_callback(Arc::clone(&failed)))?;
    // rodio prints a line on stderr whenever a device is dropped otherwise.
    device.log_on_drop(false);
    let player = new_player(&device, volume);
    Ok(Output {
        player,
        device,
        failed,
    })
}

/// The default output device, falling back to any other real one, the same
/// way rodio's own `open_default_sink` does (which can't take a callback).
fn open_device<E>(callback: E) -> Result<MixerDeviceSink>
where
    E: FnMut(CpalError) + Clone + Send + 'static,
{
    let first_err = match DeviceSinkBuilder::from_default_device().and_then(|b| {
        b.with_error_callback(callback.clone())
            .open_sink_or_fallback()
    }) {
        Ok(device) => return Ok(device),
        Err(e) => e,
    };
    let host = rodio::cpal::default_host();
    let fallback = host.output_devices().ok().and_then(|mut devices| {
        devices.find_map(|dev| {
            let real = dev
                .description()
                .is_ok_and(|d| d.driver().is_some_and(|driver| driver != "null"));
            if !real {
                return None;
            }
            DeviceSinkBuilder::from_device(dev)
                .and_then(|b| {
                    b.with_error_callback(callback.clone())
                        .open_sink_or_fallback()
                })
                .ok()
        })
    });
    fallback.ok_or_else(|| first_err.into())
}

/// Runs on the audio thread. Never prints per error: a stream stuck in an
/// error loop calls this tens of thousands of times a second, and printing
/// each one is what flooded the journal and froze the app before.
fn error_callback(failed: Arc<AtomicBool>) -> impl FnMut(CpalError) + Clone + Send + 'static {
    let mut glitches: u32 = 0;
    move |err| match err.kind() {
        // cpal already recovered from these and the stream keeps playing.
        CpalErrorKind::Xrun | CpalErrorKind::DeviceChanged | CpalErrorKind::RealtimeDenied => {
            glitches = glitches.saturating_add(1);
            // 1st, 2nd, 4th, 8th...: a storm stays a handful of lines.
            if glitches.is_power_of_two() {
                log::debug!("audio output glitch #{glitches}: {err}");
            }
        }
        // Anything else may have killed the stream: hand it to recover().
        _ => {
            if !failed.swap(true, Ordering::AcqRel) {
                log::error!("audio output failed, reopening it: {err}");
            }
        }
    }
}
