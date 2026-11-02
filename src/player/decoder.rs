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
use std::num::NonZero;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use rodio::source::SeekError;
use rodio::{ChannelCount, SampleRate, Source};
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

impl TolerantSource {
    /// Load the next decodable packet into the buffer, skipping bad ones. At
    /// the end of the stream the buffer is left empty, which is how rodio is
    /// told the source is exhausted (`current_span_len() == Some(0)`).
    fn refill(&mut self) {
        self.offset = 0;
        loop {
            match next_block(self.format.as_mut(), self.decoder.as_mut(), self.track_id) {
                Block::Decoded(spec, samples) => {
                    self.spec = spec;
                    self.buffer = samples;
                    return;
                }
                Block::Skip => continue,
                Block::End => {
                    self.buffer.clear();
                    return;
                }
            }
        }
    }
}

impl Iterator for TolerantSource {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        if self.offset >= self.buffer.len() {
            self.refill();
        }
        let s = *self.buffer.get(self.offset)?;
        self.offset += 1;
        Some(s)
    }
}

impl Source for TolerantSource {
    /// The whole current packet (rodio 0.21+ wants the span's total length,
    /// not what's left of it). Packets always hold whole frames.
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        Some(self.buffer.len())
    }

    #[inline]
    fn channels(&self) -> ChannelCount {
        u16::try_from(self.spec.channels.count())
            .ok()
            .and_then(NonZero::new)
            .unwrap_or(NonZero::<u16>::MIN)
    }

    #[inline]
    fn sample_rate(&self) -> SampleRate {
        NonZero::new(self.spec.rate).unwrap_or(NonZero::<u32>::MIN)
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        // rodio applies a seek between any two samples, possibly mid-frame.
        // The new packet starts on the first channel, so step to the channel
        // the consumer expects next, or left and right swap for the rest of
        // the track. (Same fix as rodio's own Symphonia decoder.)
        let channels = self.spec.channels.count().max(1);
        let active_channel = self.offset % channels;
        self.format
            .seek(
                SeekMode::Coarse,
                SeekTo::Time {
                    time: Time::from(pos.as_secs_f64()),
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| SeekError::Other(Arc::new(SeekFail(e.to_string()))))?;
        self.decoder.reset();
        // Decode the first packet right away: an empty buffer would read as
        // "exhausted" and the player would drop the track.
        self.refill();
        self.offset = active_channel.min(self.buffer.len());
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

#[cfg(test)]
mod tests {
    use super::*;

    /// One second of 16-bit stereo PCM at 8 kHz: every left sample is
    /// positive and every right one negative, so a channel swap is visible.
    fn stereo_wav(tag: &str) -> std::path::PathBuf {
        const RATE: u32 = 8_000;
        let frames = RATE;
        let data_len = frames * 4;
        let mut wav = Vec::with_capacity(44 + data_len as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&2u16.to_le_bytes()); // channels
        wav.extend_from_slice(&RATE.to_le_bytes());
        wav.extend_from_slice(&(RATE * 4).to_le_bytes()); // byte rate
        wav.extend_from_slice(&4u16.to_le_bytes()); // block align
        wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        for _ in 0..frames {
            wav.extend_from_slice(&8_000i16.to_le_bytes());
            wav.extend_from_slice(&(-8_000i16).to_le_bytes());
        }
        let p = std::env::temp_dir().join(format!("audra_dec_{}_{tag}.wav", std::process::id()));
        std::fs::write(&p, wav).unwrap();
        p
    }

    #[test]
    fn seek_keeps_the_source_alive_and_channels_aligned() {
        let p = stereo_wav("seek");
        let mut src = TolerantSource::open(p.to_str().unwrap()).unwrap();
        // Stop mid-frame: the next sample the consumer expects is a right one.
        assert!(src.next().unwrap() > 0.0);
        src.try_seek(Duration::from_millis(500)).unwrap();
        // An empty span here would make rodio drop the track after a seek.
        assert_ne!(src.current_span_len(), Some(0));
        assert!(
            src.next().unwrap() < 0.0,
            "left and right swapped after seek"
        );
        assert!(src.next().unwrap() > 0.0);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn end_of_stream_reports_an_empty_span() {
        let p = stereo_wav("end");
        let mut src = TolerantSource::open(p.to_str().unwrap()).unwrap();
        assert_eq!(src.channels().get(), 2);
        assert_eq!(src.sample_rate().get(), 8_000);
        assert_eq!(src.by_ref().count(), 16_000);
        assert_eq!(src.current_span_len(), Some(0));
        std::fs::remove_file(p).ok();
    }
}
