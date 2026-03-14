// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! PCM audio → MIDI transcription (`.wav` → `.mid`).
//!
//! Monophonic pitch detection using the YIN algorithm, with onset detection
//! and automatic MIDI event generation.  Produces Standard MIDI File (Type 0)
//! output that is valid for playback, editing, or further conversion to XMIDI.
//!
//! ## Pipeline
//!
//! ```text
//! f32 PCM mono samples
//!   → YIN pitch detection per analysis frame
//!   → Onset detection and note segmentation
//!   → MIDI event generation (Note-On / Note-Off)
//!   → SMF Type 0 binary
//! ```
//!
//! ## Limitations
//!
//! - **Monophonic only** — YIN detects a single fundamental frequency per
//!   frame.  Polyphonic audio (chords, multiple instruments) will produce
//!   unreliable results.  For C&C game audio from individual FM synthesis
//!   channels, this is acceptable.
//! - **No pitch bend** — portamento and slides are quantized to the nearest
//!   semitone.
//! - **No rhythm quantization** — note onsets are placed at their exact
//!   detected time.  Grid snapping may be added in a future version.
//!
//! ## Example
//!
//! ```ignore
//! use cnc_formats::transcribe::{TranscribeConfig, pcm_to_mid};
//!
//! let config = TranscribeConfig::default();
//! let midi_bytes = pcm_to_mid(&pcm_samples, 44100, &config)?;
//! ```
//!
//! ## References
//!
//! - de Cheveigné, A. & Kawahara, H. (2002). YIN, a fundamental frequency
//!   estimator for speech and music. JASA 111(4), 1917–1930.
//! - MIDI 1.0 Detailed Specification (MMA, 1996)

pub mod onset;
pub mod pitch;
mod quantize;

use crate::error::Error;
use onset::DetectedNote;

// Re-export key types at module level.
pub use onset::detect_notes;
pub use pitch::{detect_pitches, freq_to_midi_note, midi_note_to_freq};

// ── Configuration ────────────────────────────────────────────────────────────

/// Configuration for the WAV-to-MIDI transcription pipeline.
///
/// All parameters have sensible defaults for C&C game audio (FM synthesis,
/// simple waveforms, 22050 or 44100 Hz sample rates).
#[derive(Debug, Clone)]
pub struct TranscribeConfig {
    /// YIN pitch detection threshold (0.0–1.0).  Lower = stricter,
    /// reducing false positives but potentially missing quiet notes.
    /// Default: 0.15.
    pub yin_threshold: f32,

    /// Analysis window size in samples.  Larger windows detect lower
    /// frequencies but reduce time resolution.  Default: 2048.
    pub window_size: usize,

    /// Hop size in samples between successive analysis frames.
    /// Smaller hops give finer time resolution.  Default: 512.
    pub hop_size: usize,

    /// Minimum frequency to detect in Hz.  Default: 80.0 (≈E2).
    pub min_freq: f32,

    /// Maximum frequency to detect in Hz.  Default: 2000.0 (≈C7).
    pub max_freq: f32,

    /// Minimum note duration in milliseconds.  Shorter detections are
    /// discarded as noise.  Default: 50.
    pub min_duration_ms: u32,

    /// MIDI ticks per quarter note.  Default: 480.
    pub ticks_per_beat: u16,

    /// Tempo in BPM.  Default: 120.
    pub tempo_bpm: u16,

    /// MIDI channel for output notes (0–15).  Default: 0.
    pub channel: u8,

    /// MIDI velocity for detected notes (1–127).  Used when
    /// `estimate_velocity` is `false`.  Default: 100.
    pub velocity: u8,

    /// If `true`, estimate velocity from RMS energy of the audio.
    /// If `false`, all notes use the fixed `velocity` value.
    /// Default: `false`.
    pub estimate_velocity: bool,
}

impl Default for TranscribeConfig {
    fn default() -> Self {
        Self {
            yin_threshold: 0.15,
            window_size: 2048,
            hop_size: 512,
            min_freq: 80.0,
            max_freq: 2000.0,
            min_duration_ms: 50,
            ticks_per_beat: 480,
            tempo_bpm: 120,
            channel: 0,
            velocity: 100,
            estimate_velocity: false,
        }
    }
}

// ── Validation ───────────────────────────────────────────────────────────────

/// Validates configuration and input parameters before running the pipeline.
fn validate_input(
    samples: &[f32],
    sample_rate: u32,
    config: &TranscribeConfig,
) -> Result<(), Error> {
    if samples.is_empty() {
        return Err(Error::InvalidSize {
            value: 0,
            limit: 1,
            context: "transcribe: samples must not be empty",
        });
    }
    if sample_rate == 0 {
        return Err(Error::InvalidSize {
            value: 0,
            limit: 1,
            context: "transcribe: sample_rate must be > 0",
        });
    }
    if config.window_size == 0 {
        return Err(Error::InvalidSize {
            value: 0,
            limit: 1,
            context: "transcribe: window_size must be > 0",
        });
    }
    if config.hop_size == 0 {
        return Err(Error::InvalidSize {
            value: 0,
            limit: 1,
            context: "transcribe: hop_size must be > 0",
        });
    }
    Ok(())
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Detects notes in PCM audio (intermediate form).
///
/// Returns detected notes sorted by onset time.  Useful for inspection
/// or modification before MIDI generation.
///
/// # Errors
///
/// - [`Error::InvalidSize`] — empty samples, zero sample rate, or invalid config
pub fn pcm_to_notes(
    samples: &[f32],
    sample_rate: u32,
    config: &TranscribeConfig,
) -> Result<Vec<DetectedNote>, Error> {
    validate_input(samples, sample_rate, config)?;

    let pitches = pitch::detect_pitches(
        samples,
        sample_rate,
        config.window_size,
        config.hop_size,
        config.yin_threshold,
        config.min_freq,
        config.max_freq,
    );

    let notes = onset::detect_notes(
        &pitches,
        samples,
        sample_rate,
        config.hop_size,
        config.min_duration_ms,
        config.estimate_velocity,
        config.velocity,
    );

    Ok(notes)
}

/// Converts detected notes to a Standard MIDI File (Type 0).
///
/// Allows callers to modify the note list (quantize, filter, transpose)
/// before generating MIDI output.
pub fn notes_to_mid(notes: &[DetectedNote], config: &TranscribeConfig) -> Vec<u8> {
    quantize::notes_to_smf(
        notes,
        config.ticks_per_beat,
        config.tempo_bpm,
        config.channel,
    )
}

/// Transcribes PCM audio to a Standard MIDI File (Type 0).
///
/// This is the primary entry point.  Takes mono f32 PCM samples and
/// produces a valid SMF binary.
///
/// For stereo input, mix down to mono first (average L+R channels).
///
/// # Errors
///
/// - [`Error::InvalidSize`] — empty samples, zero sample rate, or invalid config
pub fn pcm_to_mid(
    samples: &[f32],
    sample_rate: u32,
    config: &TranscribeConfig,
) -> Result<Vec<u8>, Error> {
    let notes = pcm_to_notes(samples, sample_rate, config)?;
    Ok(notes_to_mid(&notes, config))
}

/// Transcribes a WAV file to a Standard MIDI File.
///
/// Convenience function: decodes the WAV, mixes stereo to mono, normalises
/// to f32, then runs the transcription pipeline.
///
/// # Errors
///
/// - [`Error::DecompressionError`] — invalid WAV data
/// - [`Error::InvalidSize`] — WAV contains no samples
#[cfg(feature = "convert")]
pub fn wav_to_mid(wav_data: &[u8], config: &TranscribeConfig) -> Result<Vec<u8>, Error> {
    let (samples, sample_rate) = decode_wav_to_mono_f32(wav_data)?;
    pcm_to_mid(&samples, sample_rate, config)
}

/// Transcribes a WAV file to XMIDI format.
///
/// Pipeline: WAV → mono f32 → detect notes → MIDI → XMI.
///
/// # Errors
///
/// - [`Error::DecompressionError`] — invalid WAV data
/// - [`Error::InvalidSize`] — WAV contains no samples or zero-length MIDI
/// - [`Error::InvalidMagic`] — generated MIDI could not be wrapped as XMI
#[cfg(all(feature = "convert", feature = "xmi"))]
pub fn wav_to_xmi(wav_data: &[u8], config: &TranscribeConfig) -> Result<Vec<u8>, Error> {
    let mid_bytes = wav_to_mid(wav_data, config)?;
    mid_to_xmi(&mid_bytes)
}

/// Wraps a Standard MIDI File in an XMIDI IFF container.
///
/// Builds a minimal FORM:XDIR + CAT:XMID structure containing one
/// FORM:XMID with a TIMB chunk (empty) and the EVNT data extracted
/// from the MIDI track.
///
/// # Errors
///
/// - [`Error::InvalidMagic`] — input is not valid SMF
#[cfg(feature = "xmi")]
pub fn mid_to_xmi(mid_bytes: &[u8]) -> Result<Vec<u8>, Error> {
    // Parse the MIDI to extract track data.
    if mid_bytes.len() < 14 || mid_bytes.get(..4) != Some(b"MThd".as_slice()) {
        return Err(Error::InvalidMagic { context: "MIDI" });
    }

    // Find the MTrk chunk.
    let mut pos: usize = 14; // skip MThd header
    let track_data = loop {
        if pos.saturating_add(8) > mid_bytes.len() {
            return Err(Error::InvalidMagic {
                context: "MIDI: no MTrk chunk found",
            });
        }
        let chunk_id = mid_bytes.get(pos..pos.saturating_add(4));
        let chunk_len_bytes = mid_bytes.get(pos.saturating_add(4)..pos.saturating_add(8));
        let chunk_len = chunk_len_bytes
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .unwrap_or(0);

        if chunk_id == Some(b"MTrk".as_slice()) {
            let data_start = pos.saturating_add(8);
            let data_end = data_start.saturating_add(chunk_len).min(mid_bytes.len());
            break mid_bytes
                .get(data_start..data_end)
                .ok_or(Error::InvalidMagic {
                    context: "MIDI: MTrk truncated",
                })?;
        }
        pos = pos.saturating_add(8).saturating_add(chunk_len);
    };

    // Build XMI IFF structure:
    // FORM:XDIR { INFO(2 bytes: count=1) }
    // CAT :XMID { FORM:XMID { TIMB(0 bytes) EVNT(track_data) } }

    let evnt_chunk_size = track_data.len();
    let timb_chunk_size = 0usize;
    let form_xmid_size = 4 + 8 + timb_chunk_size + 8 + evnt_chunk_size; // "XMID" + TIMB + EVNT
    let cat_xmid_size = 4 + form_xmid_size + 8; // "XMID" + FORM chunk
    let info_data: [u8; 2] = 1u16.to_le_bytes();
    let form_xdir_size = 4 + 8 + info_data.len(); // "XDIR" + INFO chunk

    let total_size = 8 + form_xdir_size + 8 + cat_xmid_size;
    let mut xmi = Vec::with_capacity(total_size);

    // FORM:XDIR
    xmi.extend_from_slice(b"FORM");
    xmi.extend_from_slice(&(form_xdir_size as u32).to_be_bytes());
    xmi.extend_from_slice(b"XDIR");
    // INFO chunk
    xmi.extend_from_slice(b"INFO");
    xmi.extend_from_slice(&(info_data.len() as u32).to_be_bytes());
    xmi.extend_from_slice(&info_data);

    // CAT:XMID
    xmi.extend_from_slice(b"CAT ");
    xmi.extend_from_slice(&(cat_xmid_size as u32).to_be_bytes());
    xmi.extend_from_slice(b"XMID");

    // FORM:XMID
    xmi.extend_from_slice(b"FORM");
    xmi.extend_from_slice(&((form_xmid_size) as u32).to_be_bytes());
    xmi.extend_from_slice(b"XMID");
    // TIMB chunk (empty)
    xmi.extend_from_slice(b"TIMB");
    xmi.extend_from_slice(&0u32.to_be_bytes());
    // EVNT chunk
    xmi.extend_from_slice(b"EVNT");
    xmi.extend_from_slice(&(evnt_chunk_size as u32).to_be_bytes());
    xmi.extend_from_slice(track_data);

    Ok(xmi)
}

// ── WAV decoding helper ──────────────────────────────────────────────────────

/// Decodes a WAV file to mono f32 samples.
///
/// Mixes stereo to mono (average L+R).  Converts integer samples to f32.
#[cfg(feature = "convert")]
fn decode_wav_to_mono_f32(wav_data: &[u8]) -> Result<(Vec<f32>, u32), Error> {
    let cursor = std::io::Cursor::new(wav_data);
    let mut reader = hound::WavReader::new(cursor).map_err(|_| Error::DecompressionError {
        reason: "WAV decode failed",
    })?;

    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let channels = spec.channels as usize;

    let raw_samples: Vec<f32> = if spec.sample_format == hound::SampleFormat::Float {
        reader
            .samples::<f32>()
            .map(|s| s.unwrap_or(0.0).clamp(-1.0, 1.0))
            .collect()
    } else {
        reader
            .samples::<i16>()
            .map(|s| s.unwrap_or(0) as f32 / 32768.0)
            .collect()
    };

    if raw_samples.is_empty() {
        return Err(Error::InvalidSize {
            value: 0,
            limit: 1,
            context: "WAV contains no samples",
        });
    }

    // Mix to mono.
    let mono = if channels <= 1 {
        raw_samples
    } else {
        raw_samples
            .chunks(channels)
            .map(|frame| {
                let sum: f32 = frame.iter().sum();
                sum / channels as f32
            })
            .collect()
    };

    Ok((mono, sample_rate))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_validation;
