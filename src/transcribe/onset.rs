// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Onset detection and note segmentation.
//!
//! Groups consecutive pitch-detection frames with the same MIDI note into
//! discrete note events with onset time, duration, and velocity.
//!
//! ## Approach
//!
//! 1. Compute per-frame RMS energy.
//! 2. Walk the pitch vector: a new note segment starts when either:
//!    - The detected MIDI note changes, or
//!    - The frame is voiced after one or more unvoiced frames.
//! 3. Segments shorter than `min_duration_ms` are discarded.
//! 4. Velocity is estimated from the RMS energy of the onset frame,
//!    scaled to the MIDI 1–127 range.

use super::pitch::freq_to_midi_note;

/// A detected note with timing and velocity.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedNote {
    /// MIDI note number (0–127).
    pub note: u8,
    /// Onset time in seconds from the start of the audio.
    pub onset_secs: f64,
    /// Duration in seconds.
    pub duration_secs: f64,
    /// MIDI velocity (1–127).
    pub velocity: u8,
}

/// Maximum number of note segments to accumulate.
/// V38: prevents unbounded allocation from pathological inputs.
const MAX_NOTES: usize = 1_000_000;

/// Groups pitch frames into note segments.
///
/// # Arguments
///
/// - `pitches` — per-frame pitch estimates from [`super::pitch::detect_pitches`]
/// - `samples` — original mono PCM audio
/// - `sample_rate` — audio sample rate in Hz
/// - `hop_size` — samples between successive frames
/// - `min_duration_ms` — minimum note duration; shorter segments are discarded
/// - `estimate_velocity` — if `true`, derives velocity from RMS energy;
///   otherwise all notes get `default_velocity`
/// - `default_velocity` — velocity assigned when `estimate_velocity` is `false`
pub fn detect_notes(
    pitches: &[Option<f32>],
    samples: &[f32],
    sample_rate: u32,
    hop_size: usize,
    min_duration_ms: u32,
    estimate_velocity: bool,
    default_velocity: u8,
) -> Vec<DetectedNote> {
    if pitches.is_empty() || sample_rate == 0 || hop_size == 0 {
        return Vec::new();
    }

    let secs_per_frame = hop_size as f64 / sample_rate as f64;
    let min_duration_secs = min_duration_ms as f64 / 1000.0;

    let mut notes = Vec::new();

    // Current segment tracking.
    let mut seg_note: Option<u8> = None;
    let mut seg_start_frame: usize = 0;
    let mut seg_energy_sum: f64 = 0.0;
    let mut seg_energy_count: usize = 0;

    for (i, pitch) in pitches.iter().enumerate() {
        let current_midi = pitch.and_then(freq_to_midi_note);

        match (seg_note, current_midi) {
            (Some(prev), Some(curr)) if prev == curr => {
                // Same note continues — accumulate energy.
                let energy = frame_rms(samples, i, hop_size);
                seg_energy_sum += energy as f64;
                seg_energy_count = seg_energy_count.saturating_add(1);
            }
            (Some(prev_note), _) => {
                // Note changed or went silent — close the segment.
                let duration_secs = (i - seg_start_frame) as f64 * secs_per_frame;
                if duration_secs >= min_duration_secs && notes.len() < MAX_NOTES {
                    let velocity = if estimate_velocity {
                        energy_to_velocity(seg_energy_sum, seg_energy_count)
                    } else {
                        default_velocity
                    };
                    notes.push(DetectedNote {
                        note: prev_note,
                        onset_secs: seg_start_frame as f64 * secs_per_frame,
                        duration_secs,
                        velocity,
                    });
                }
                // Start new segment if there's a new note.
                if let Some(new_note) = current_midi {
                    seg_note = Some(new_note);
                    seg_start_frame = i;
                    let energy = frame_rms(samples, i, hop_size);
                    seg_energy_sum = energy as f64;
                    seg_energy_count = 1;
                } else {
                    seg_note = None;
                }
            }
            (None, Some(new_note)) => {
                // New note after silence.
                seg_note = Some(new_note);
                seg_start_frame = i;
                let energy = frame_rms(samples, i, hop_size);
                seg_energy_sum = energy as f64;
                seg_energy_count = 1;
            }
            (None, None) => {
                // Silence continues.
            }
        }
    }

    // Close final segment.
    if let Some(note) = seg_note {
        let duration_secs = (pitches.len() - seg_start_frame) as f64 * secs_per_frame;
        if duration_secs >= min_duration_secs && notes.len() < MAX_NOTES {
            let velocity = if estimate_velocity {
                energy_to_velocity(seg_energy_sum, seg_energy_count)
            } else {
                default_velocity
            };
            notes.push(DetectedNote {
                note,
                onset_secs: seg_start_frame as f64 * secs_per_frame,
                duration_secs,
                velocity,
            });
        }
    }

    notes
}

/// Computes the RMS energy of the samples in one analysis frame.
fn frame_rms(samples: &[f32], frame_idx: usize, hop_size: usize) -> f32 {
    let start = frame_idx.saturating_mul(hop_size);
    let end = start.saturating_add(hop_size).min(samples.len());
    let slice = match samples.get(start..end) {
        Some(s) => s,
        None => return 0.0,
    };
    if slice.is_empty() {
        return 0.0;
    }

    let sum_sq: f64 = slice
        .iter()
        .map(|&s| {
            let s = if s.is_finite() { s } else { 0.0 };
            (s as f64) * (s as f64)
        })
        .sum();
    (sum_sq / slice.len() as f64).sqrt() as f32
}

/// Maps average RMS energy to MIDI velocity (1–127).
///
/// Uses a simple logarithmic curve.  RMS ≈ 0.7 (full-scale sine) maps to
/// velocity 127; RMS near zero maps to velocity 1.
fn energy_to_velocity(energy_sum: f64, count: usize) -> u8 {
    if count == 0 {
        return 64;
    }
    let avg_rms = energy_sum / count as f64;
    if avg_rms <= 0.0 {
        return 1;
    }
    // Map RMS to 1–127.  A full-scale sine has RMS ≈ 0.707.
    // Use a mild log curve: velocity = 127 * (1 + log10(rms)) clamped.
    let log_scaled = 1.0 + avg_rms.log10();
    let normalised = log_scaled.clamp(0.0, 1.0);
    let vel = (normalised * 126.0 + 1.0).round() as u8;
    vel.clamp(1, 127)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_pitches_returns_empty() {
        let notes = detect_notes(&[], &[], 44100, 512, 50, false, 100);
        assert!(notes.is_empty());
    }

    #[test]
    fn all_none_returns_empty() {
        let pitches = vec![None; 20];
        let samples = vec![0.0f32; 20 * 512];
        let notes = detect_notes(&pitches, &samples, 44100, 512, 50, false, 100);
        assert!(notes.is_empty());
    }

    #[test]
    fn single_note_segment() {
        // 20 frames of A4 (440 Hz) at hop_size=512, sr=44100
        // Duration = 20 * 512 / 44100 ≈ 0.232 seconds
        let pitches: Vec<Option<f32>> = vec![Some(440.0); 20];
        let samples = vec![0.5f32; 20 * 512];
        let notes = detect_notes(&pitches, &samples, 44100, 512, 50, false, 100);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes.first().map(|n| n.note), Some(69));
        assert!(
            notes.first().map(|n| n.onset_secs).unwrap_or(1.0) < 0.001,
            "onset should be near 0"
        );
    }

    #[test]
    fn two_note_sequence() {
        // 10 frames A4, 10 frames C5.
        let mut pitches: Vec<Option<f32>> = vec![Some(440.0); 10];
        pitches.extend(vec![Some(523.25); 10]);
        let samples = vec![0.5f32; 20 * 512];
        let notes = detect_notes(&pitches, &samples, 44100, 512, 50, false, 100);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes.first().map(|n| n.note), Some(69)); // A4
        assert_eq!(notes.get(1).map(|n| n.note), Some(72)); // C5
    }

    #[test]
    fn short_notes_filtered() {
        // 2 frames at hop_size=512, sr=44100 → ~23ms, below 50ms minimum.
        let pitches = vec![Some(440.0); 2];
        let samples = vec![0.5f32; 2 * 512];
        let notes = detect_notes(&pitches, &samples, 44100, 512, 50, false, 100);
        assert!(
            notes.is_empty(),
            "notes shorter than min_duration should be filtered"
        );
    }

    #[test]
    fn velocity_estimation() {
        let pitches = vec![Some(440.0); 20];
        // Full-scale signal → high velocity.
        let samples = vec![0.7f32; 20 * 512];
        let notes = detect_notes(&pitches, &samples, 44100, 512, 50, true, 100);
        assert_eq!(notes.len(), 1);
        let vel = notes.first().map(|n| n.velocity).unwrap_or(0);
        assert!(
            vel > 90,
            "full-scale signal should have high velocity, got {vel}"
        );
    }

    #[test]
    fn energy_to_velocity_range() {
        assert_eq!(energy_to_velocity(0.0, 1), 1);
        assert_eq!(energy_to_velocity(0.0, 0), 64);
        let v = energy_to_velocity(0.707, 1);
        assert!(v > 100, "RMS 0.707 should yield high velocity, got {v}");
    }
}
