//! A fault-tolerant audio source backed by Symphonia.
//!
//! rodio's built-in Symphonia decoder aborts a stream after more than three
//! consecutive `DecodeError`s (`MAX_DECODE_RETRIES`). Real-world MP3 rips often
//! open with several frames that reference a bit reservoir which isn't present
//! at the start of the stream — Symphonia reports `mpa: invalid main_data
//! offset` for each — so the whole file fails to play with no audible error.
//! Tolerant players (ffmpeg, mpg123, GStreamer) simply discard those leading
//! frames; this source does the same, skipping any number of bad packets both
//! while priming the stream and during playback. Affected files lose at most a
//! few milliseconds at the very beginning instead of refusing to play.

use std::fs::File;
use std::time::Duration;

use anyhow::{anyhow, Result};
use rodio::source::SeekError;
use rodio::Source;
use symphonia::core::audio::{AudioBufferRef, SampleBuffer, SignalSpec};
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::{Duration as SymDuration, Time};

pub struct TolerantSource {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    /// Spec of the most recently decoded packet; channels/rate are read from
    /// here, matching how rodio's own decoder reports them.
    spec: SignalSpec,
    total_duration: Option<Duration>,
    /// Interleaved f32 samples of the current packet and a read cursor into it.
    buffer: Vec<f32>,
    offset: usize,
}

impl TolerantSource {
    pub fn open(path: &str) -> Result<Self> {
        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
        {
            hint.with_extension(ext);
        }

        let probed = symphonia::default::get_probe().format(
            &hint,
            mss,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )?;
        let mut format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| anyhow!("no track with a supported codec"))?;
        let track_id = track.id;
        let total_duration = track
            .codec_params
            .time_base
            .zip(track.codec_params.n_frames)
            .map(|(base, frames)| {
                let t = base.calc_time(frames);
                Duration::from_secs_f64(t.seconds as f64 + t.frac)
            });
        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())?;

        // Prime the stream: pull packets until one decodes, tolerating any
        // number of leading bad frames — this is exactly what rodio refuses to
        // do once it hits its 3-retry ceiling.
        let (spec, buffer) = loop {
            match next_block(format.as_mut(), decoder.as_mut(), track_id) {
                Block::Decoded(spec, buffer) => break (spec, buffer),
                Block::Skip => continue,
                Block::End => return Err(anyhow!("stream ended before any audio decoded")),
            }
        };

        Ok(Self {
            format,
            decoder,
            track_id,
            spec,
            total_duration,
            buffer,
            offset: 0,
        })
    }
}

enum Block {
    Decoded(SignalSpec, Vec<f32>),
    /// A bad or foreign packet that the caller should step over.
    Skip,
    /// End of stream (or an unrecoverable read error).
    End,
}

/// Pull the next packet for `track_id` and decode it, converting the result to
/// interleaved f32. `DecodeError`s are reported as [`Block::Skip`] so callers
/// loop past them; only EOF / IO failures end the stream.
fn next_block(format: &mut dyn FormatReader, decoder: &mut dyn Decoder, track_id: u32) -> Block {
    let packet = match format.next_packet() {
        Ok(p) => p,
        // IoError here is how Symphonia signals end-of-stream.
        Err(_) => return Block::End,
    };
    if packet.track_id() != track_id {
        return Block::Skip;
    }
    match decoder.decode(&packet) {
        Ok(decoded) => {
            let spec = *decoded.spec();
            let samples = to_f32(decoded, &spec);
            if samples.is_empty() {
                Block::Skip
            } else {
                Block::Decoded(spec, samples)
            }
        }
        // The whole point: a decode error is recoverable, just move on.
        Err(SymError::DecodeError(_)) => Block::Skip,
        Err(_) => Block::End,
    }
}

fn to_f32(decoded: AudioBufferRef, spec: &SignalSpec) -> Vec<f32> {
    let duration = SymDuration::from(decoded.capacity() as u64);
    let mut sb = SampleBuffer::<f32>::new(duration, *spec);
    sb.copy_interleaved_ref(decoded);
    sb.samples().to_vec()
}

impl Iterator for TolerantSource {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        loop {
            if let Some(&s) = self.buffer.get(self.offset) {
                self.offset += 1;
                return Some(s);
            }
            match next_block(self.format.as_mut(), self.decoder.as_mut(), self.track_id) {
                Block::Decoded(spec, samples) => {
                    self.spec = spec;
                    self.buffer = samples;
                    self.offset = 0;
                }
                Block::Skip => continue,
                Block::End => return None,
            }
        }
    }
}

impl Source for TolerantSource {
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        Some(self.buffer.len())
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.spec.channels.count() as u16
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.spec.rate
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.format
            .seek(
                SeekMode::Coarse,
                SeekTo::Time {
                    time: Time::from(pos.as_secs_f64()),
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| SeekError::Other(Box::new(SeekFail(e.to_string()))))?;
        self.decoder.reset();
        self.buffer.clear();
        self.offset = 0;
        Ok(())
    }
}

#[derive(Debug)]
struct SeekFail(String);

impl std::fmt::Display for SeekFail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "seek failed: {}", self.0)
    }
}

impl std::error::Error for SeekFail {}
