// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! MIDI file parser, writer, and SoundFont renderer (`.mid`).
//!
//! Standard MIDI File (SMF) format support — Type 0 (single track) and
//! Type 1 (multi-track).  MIDI is the intermediate format for Iron Curtain's
//! LLM audio generation pipeline (ABC → MIDI → SoundFont → PCM) and a
//! universal standard in game audio tooling.
//!
//! ## Architecture
//!
//! This module is a thin wrapper around two pure-Rust crates:
//!
//! - **`midly`** (Unlicense) — zero-allocation MIDI parser and writer
//! - **`rustysynth`** (MIT) — SoundFont SF2 synthesizer (renders MIDI → PCM)
//!
//! Both are WASM-compatible with zero C bindings.
//!
//! ## Historical Note
//!
//! C&C (TD/RA) shipped music as `.aud` digital audio, not MIDI.  Earlier
//! Westwood titles used synthesiser formats (`.adl`, XMIDI `.xmi`), not
//! standard `.mid`.  MIDI support in `cnc-formats` serves the broader game
//! modding ecosystem and IC's LLM generation pipeline.
//!
//! ## References
//!
//! - MIDI 1.0 Detailed Specification (MMA, 1996)
//! - Standard MIDI Files specification (RP-001, MMA)
//! - `midly` crate documentation

use crate::error::Error;

// ── Re-exports ───────────────────────────────────────────────────────────────

/// Re-export `midly::Format` so callers can inspect MIDI file type without
/// depending on `midly` directly.
pub use midly::Format as MidiFormat;

/// Re-export `midly::Timing` for callers inspecting the time division.
pub use midly::Timing;

// ── MidFile ──────────────────────────────────────────────────────────────────

/// A parsed Standard MIDI File.
///
/// Wraps the `midly` parse result with convenience accessors for track count,
/// timing, and format type.  The raw `midly::Smf` is available via the
/// `smf()` method for advanced users who need direct event-level access.
#[derive(Debug, Clone)]
pub struct MidFile<'a> {
    /// The parsed SMF structure from `midly`.
    smf: midly::Smf<'a>,
}

impl<'a> MidFile<'a> {
    /// Parses a Standard MIDI File from a byte slice.
    ///
    /// Accepts SMF Type 0 (single track) and Type 1 (multi-track).
    /// Type 2 (multi-song) files are parsed but unusual in game audio.
    ///
    /// # Errors
    ///
    /// - [`Error::InvalidMagic`] — data does not start with `MThd` header
    /// - [`Error::UnexpectedEof`] — data is truncated
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        // V38: basic size check before handing off to midly.
        if data.len() < 14 {
            return Err(Error::UnexpectedEof {
                needed: 14,
                available: data.len(),
            });
        }
        // Validate the MIDI magic bytes ("MThd") before parsing so we
        // return our own error type rather than a midly error string.
        if data.get(..4) != Some(b"MThd".as_slice()) {
            return Err(Error::InvalidMagic { context: "MIDI" });
        }
        let smf = midly::Smf::parse(data).map_err(|_| Error::InvalidMagic { context: "MIDI" })?;
        Ok(MidFile { smf })
    }

    /// Returns the MIDI file format (Type 0, 1, or 2).
    #[inline]
    pub fn format(&self) -> MidiFormat {
        self.smf.header.format
    }

    /// Returns the timing mode (ticks per beat or SMPTE).
    #[inline]
    pub fn timing(&self) -> Timing {
        self.smf.header.timing
    }

    /// Returns the number of tracks.
    #[inline]
    pub fn track_count(&self) -> usize {
        self.smf.tracks.len()
    }

    /// Returns the total number of MIDI events across all tracks.
    pub fn event_count(&self) -> usize {
        self.smf.tracks.iter().map(|t| t.len()).sum()
    }

    /// Returns a reference to the underlying `midly::Smf` for advanced access.
    #[inline]
    pub fn smf(&self) -> &midly::Smf<'a> {
        &self.smf
    }

    /// Estimates the duration in seconds based on tempo events and timing.
    ///
    /// For Metrical timing (ticks per beat), scans all tracks for tempo
    /// meta-events and computes the total duration from tick counts.
    /// Defaults to 120 BPM if no tempo event is found (MIDI standard default).
    /// Returns 0.0 for SMPTE timing (not commonly used in game audio).
    pub fn duration_secs(&self) -> f64 {
        let ticks_per_beat = match self.smf.header.timing {
            Timing::Metrical(tpb) => u32::from(tpb.as_int()),
            Timing::Timecode(..) => return 0.0,
        };
        if ticks_per_beat == 0 {
            return 0.0;
        }

        // Collect tempo changes from all tracks (they can appear in any track
        // in Type 1 files, though conventionally they're in track 0).
        // A tempo change is a Meta event with MetaMessage::Tempo.
        let mut tempo_events: Vec<(u32, u32)> = Vec::new(); // (absolute_tick, microseconds_per_beat)
        for track in &self.smf.tracks {
            let mut abs_tick: u32 = 0;
            for event in track {
                abs_tick = abs_tick.saturating_add(event.delta.as_int());
                if let midly::TrackEventKind::Meta(midly::MetaMessage::Tempo(t)) = event.kind {
                    tempo_events.push((abs_tick, t.as_int()));
                }
            }
        }
        tempo_events.sort_by_key(|(tick, _)| *tick);

        // Find the maximum absolute tick across all tracks.
        let mut max_tick: u32 = 0;
        for track in &self.smf.tracks {
            let mut abs_tick: u32 = 0;
            for event in track {
                abs_tick = abs_tick.saturating_add(event.delta.as_int());
            }
            max_tick = max_tick.max(abs_tick);
        }

        // Walk tempo regions to accumulate duration.
        // Default MIDI tempo: 120 BPM = 500,000 µs/beat.
        let default_tempo: u32 = 500_000;
        let mut duration_us: f64 = 0.0;
        let mut prev_tick: u32 = 0;
        let mut current_tempo: u32 = default_tempo;

        for &(tick, tempo) in &tempo_events {
            if tick > prev_tick {
                let delta_ticks = tick - prev_tick;
                duration_us +=
                    f64::from(delta_ticks) * f64::from(current_tempo) / f64::from(ticks_per_beat);
            }
            current_tempo = tempo;
            prev_tick = tick;
        }
        // Remaining ticks after last tempo change.
        if max_tick > prev_tick {
            let delta_ticks = max_tick - prev_tick;
            duration_us +=
                f64::from(delta_ticks) * f64::from(current_tempo) / f64::from(ticks_per_beat);
        }

        duration_us / 1_000_000.0
    }

    /// Collects the set of MIDI channel numbers used across all tracks.
    ///
    /// Channel 9 (zero-indexed) is the General MIDI percussion channel.
    pub fn channels_used(&self) -> Vec<u8> {
        let mut seen = [false; 16];
        for track in &self.smf.tracks {
            for event in track {
                if let midly::TrackEventKind::Midi { channel, .. } = event.kind {
                    let ch = channel.as_int();
                    if let Some(slot) = seen.get_mut(ch as usize) {
                        *slot = true;
                    }
                }
            }
        }
        seen.iter()
            .enumerate()
            .filter(|(_, &used)| used)
            .map(|(i, _)| i as u8)
            .collect()
    }
}

// ── Write ────────────────────────────────────────────────────────────────────

/// Serialises a `MidFile` back to SMF binary format.
///
/// Produces a valid Standard MIDI File that can be loaded by any MIDI player.
pub fn write(mid: &MidFile<'_>) -> Result<Vec<u8>, Error> {
    let mut buf = Vec::new();
    mid.smf.write(&mut buf).map_err(|_| Error::InvalidMagic {
        context: "MIDI write",
    })?;
    Ok(buf)
}

// ── SoundFont rendering ──────────────────────────────────────────────────────

/// Loads a SoundFont (`.sf2`) file from bytes.
///
/// Returns a `rustysynth::SoundFont` that can be passed to [`render_to_pcm`].
///
/// # Errors
///
/// - [`Error::InvalidMagic`] — the data is not a valid SF2 file
pub fn load_soundfont(data: &[u8]) -> Result<std::sync::Arc<rustysynth::SoundFont>, Error> {
    let mut cursor = std::io::Cursor::new(data);
    let sf = rustysynth::SoundFont::new(&mut cursor).map_err(|_| Error::InvalidMagic {
        context: "SoundFont",
    })?;
    Ok(std::sync::Arc::new(sf))
}

/// Renders a MIDI file to PCM audio using a SoundFont.
///
/// Returns `(left_channel, right_channel)` — stereo PCM at the requested
/// sample rate.  Each channel contains `f32` samples in [-1.0, 1.0].
///
/// Uses `rustysynth::MidiFileSequencer` for offline rendering — no real-time
/// playback or threading is involved.
///
/// # Arguments
///
/// - `mid_bytes` — raw SMF bytes (not a parsed `MidFile`, because rustysynth
///   uses its own MIDI parser internally)
/// - `soundfont` — loaded SoundFont (from [`load_soundfont`])
/// - `sample_rate` — output sample rate in Hz (e.g. 44100)
pub fn render_to_pcm(
    mid_bytes: &[u8],
    soundfont: &std::sync::Arc<rustysynth::SoundFont>,
    sample_rate: u32,
) -> Result<(Vec<f32>, Vec<f32>), Error> {
    let midi_file = parse_render_midi(mid_bytes)?;
    validate_render_sample_rate(sample_rate)?;

    let settings = rustysynth::SynthesizerSettings::new(sample_rate as i32);
    let synthesizer =
        rustysynth::Synthesizer::new(soundfont, &settings).map_err(|_| Error::InvalidMagic {
            context: "SoundFont synthesizer",
        })?;
    let mut sequencer = rustysynth::MidiFileSequencer::new(synthesizer);

    // Start playback (non-looping).
    sequencer.play(&midi_file, false);

    // Estimate duration to size the output buffers.
    // Parse with midly just for duration estimation.
    let duration_secs = if let Ok(mid) = MidFile::parse(mid_bytes) {
        mid.duration_secs()
    } else {
        10.0 // fallback: 10 seconds
    };
    // Add 2 seconds of tail for reverb/release.
    let total_samples = ((duration_secs + 2.0) * f64::from(sample_rate)) as usize;
    let mut left = vec![0f32; total_samples];
    let mut right = vec![0f32; total_samples];

    // Render the entire file offline.
    sequencer.render(&mut left, &mut right);

    Ok((left, right))
}

/// Parses the MIDI bytes using rustysynth's SMF loader for render paths.
///
/// Why: `render_to_pcm()` uses rustysynth's own parser, not `midly`, so the
/// render error path needs a dedicated validation step with its own tests.
fn parse_render_midi(mid_bytes: &[u8]) -> Result<std::sync::Arc<rustysynth::MidiFile>, Error> {
    let mut cursor = std::io::Cursor::new(mid_bytes);
    let midi_file = rustysynth::MidiFile::new(&mut cursor).map_err(|_| Error::InvalidMagic {
        context: "MIDI (rustysynth)",
    })?;
    Ok(std::sync::Arc::new(midi_file))
}

/// Validates the requested render sample rate against rustysynth's limits.
///
/// Why: rustysynth rejects rates outside 16–192 kHz.  Validate that before
/// synthesis so tests can cover the render configuration path without needing
/// a full SoundFont fixture.
fn validate_render_sample_rate(sample_rate: u32) -> Result<(), Error> {
    if !(16_000..=192_000).contains(&sample_rate) {
        return Err(Error::InvalidMagic {
            context: "SoundFont synthesizer",
        });
    }
    Ok(())
}

/// Renders a MIDI file to WAV format bytes using a SoundFont.
///
/// Returns a complete WAV file (16-bit PCM, stereo) as a byte vector.
///
/// # Arguments
///
/// - `mid_bytes` — raw SMF bytes
/// - `soundfont` — loaded SoundFont
/// - `sample_rate` — output sample rate in Hz (e.g. 44100)
#[cfg(feature = "convert")]
pub fn render_to_wav(
    mid_bytes: &[u8],
    soundfont: &std::sync::Arc<rustysynth::SoundFont>,
    sample_rate: u32,
) -> Result<Vec<u8>, Error> {
    let (left, right) = render_to_pcm(mid_bytes, soundfont, sample_rate)?;

    // Interleave stereo and convert f32 → i16.
    let sample_count = left.len().min(right.len());
    let mut output = Vec::new();
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer =
        hound::WavWriter::new(std::io::Cursor::new(&mut output), spec).map_err(|_| {
            Error::InvalidMagic {
                context: "WAV writer",
            }
        })?;

    for i in 0..sample_count {
        // Clamp and convert f32 [-1.0, 1.0] to i16.
        let l = (left.get(i).copied().unwrap_or(0.0) * 32767.0)
            .round()
            .clamp(-32768.0, 32767.0) as i16;
        let r = (right.get(i).copied().unwrap_or(0.0) * 32767.0)
            .round()
            .clamp(-32768.0, 32767.0) as i16;
        writer.write_sample(l).map_err(|_| Error::InvalidMagic {
            context: "WAV sample",
        })?;
        writer.write_sample(r).map_err(|_| Error::InvalidMagic {
            context: "WAV sample",
        })?;
    }
    writer.finalize().map_err(|_| Error::InvalidMagic {
        context: "WAV finalize",
    })?;

    Ok(output)
}

#[cfg(test)]
mod tests;
