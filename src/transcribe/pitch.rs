// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! YIN pitch detection algorithm.
//!
//! Pure time-domain fundamental frequency estimation — no FFT required.
//! Operates on `f32` PCM samples and returns the detected frequency in Hz
//! for each analysis frame.
//!
//! ## Algorithm
//!
//! YIN (de Cheveigné & Kawahara, 2002) detects the fundamental period of a
//! signal by computing a normalised autocorrelation-like function:
//!
//! 1. **Difference function** `d(τ)` — squared difference between the signal
//!    and a lagged copy, summed over a half-window integration range.
//! 2. **Cumulative mean normalised difference** `d'(τ)` — normalises `d(τ)`
//!    by the running mean, suppressing octave errors.
//! 3. **Absolute threshold** — the first lag `τ` where `d'(τ)` drops below
//!    a configurable threshold is selected as the period candidate.
//! 4. **Parabolic interpolation** — refines the lag to sub-sample accuracy.
//! 5. **Frequency conversion** — `f = sample_rate / τ_refined`.
//!
//! ## References
//!
//! - de Cheveigné, A. & Kawahara, H. (2002). YIN, a fundamental frequency
//!   estimator for speech and music. JASA 111(4), 1917–1930.

/// Maximum number of frames that can be produced from a single call to
/// [`detect_pitches`].  V38: prevents unbounded allocation from pathological
/// inputs where `hop_size` is very small relative to sample count.
const MAX_FRAMES: usize = 4_000_000;

/// Detects the pitch (fundamental frequency) of each analysis frame.
///
/// Returns a `Vec<Option<f32>>` with one entry per frame.  `Some(hz)` if a
/// pitch was detected, `None` for unvoiced / silent frames.
///
/// # Arguments
///
/// - `samples` — mono PCM audio, f32 in \[-1.0, 1.0\]
/// - `sample_rate` — sample rate in Hz
/// - `window_size` — analysis window in samples (must be ≥ 4)
/// - `hop_size` — samples between successive frames (must be ≥ 1)
/// - `threshold` — YIN threshold (0.0–1.0); lower = stricter
/// - `min_freq` — minimum detectable frequency in Hz
/// - `max_freq` — maximum detectable frequency in Hz
pub fn detect_pitches(
    samples: &[f32],
    sample_rate: u32,
    window_size: usize,
    hop_size: usize,
    threshold: f32,
    min_freq: f32,
    max_freq: f32,
) -> Vec<Option<f32>> {
    if samples.is_empty() || window_size < 4 || hop_size == 0 || sample_rate == 0 {
        return Vec::new();
    }

    let sr = sample_rate as f32;

    // Lag range from frequency bounds.
    // min_tau corresponds to max_freq, max_tau corresponds to min_freq.
    let min_tau = if max_freq > 0.0 {
        (sr / max_freq).floor().max(1.0) as usize
    } else {
        1
    };
    let max_tau = if min_freq > 0.0 {
        (sr / min_freq).ceil() as usize
    } else {
        window_size / 2
    };
    // Clamp to half-window (YIN integration range).
    let half_window = window_size / 2;
    let max_tau = max_tau.min(half_window);
    if min_tau >= max_tau {
        return Vec::new();
    }

    let num_frames = samples
        .len()
        .saturating_sub(window_size)
        .checked_div(hop_size)
        .map(|n| n.saturating_add(1))
        .unwrap_or(0)
        .min(MAX_FRAMES);

    if num_frames == 0 && samples.len() >= window_size {
        // Exactly one frame fits.
        let pitch = yin_pitch(samples, max_tau, min_tau, threshold, sr);
        return vec![pitch];
    }

    let mut pitches = Vec::with_capacity(num_frames);

    for frame_idx in 0..num_frames {
        let start = frame_idx.saturating_mul(hop_size);
        let end = start.saturating_add(window_size).min(samples.len());
        let window = match samples.get(start..end) {
            Some(w) if w.len() >= window_size => w,
            _ => break,
        };
        pitches.push(yin_pitch(window, max_tau, min_tau, threshold, sr));
    }

    pitches
}

/// Runs the YIN algorithm on a single window and returns the detected
/// frequency, or `None` if no pitch is found.
fn yin_pitch(
    window: &[f32],
    max_tau: usize,
    min_tau: usize,
    threshold: f32,
    sample_rate: f32,
) -> Option<f32> {
    let d_prime = yin_cmnd(window, max_tau);

    // Find the first tau ≥ min_tau where d'(tau) < threshold.
    let mut best_tau: Option<usize> = None;
    let mut tau = min_tau;
    while tau < d_prime.len() {
        let val = d_prime.get(tau).copied().unwrap_or(1.0);
        if val < threshold {
            best_tau = Some(tau);
            // Walk forward to find the local minimum in this dip.
            while tau.saturating_add(1) < d_prime.len() {
                let next = d_prime.get(tau.saturating_add(1)).copied().unwrap_or(1.0);
                let curr = d_prime.get(tau).copied().unwrap_or(1.0);
                if next >= curr {
                    break;
                }
                tau = tau.saturating_add(1);
                best_tau = Some(tau);
            }
            break;
        }
        tau = tau.saturating_add(1);
    }

    let tau = best_tau?;
    let refined = parabolic_interpolation(&d_prime, tau);
    if refined < 1.0 {
        return None;
    }
    let freq = sample_rate / refined;
    Some(freq)
}

/// Computes the cumulative mean normalised difference function d'(τ).
///
/// Combines the difference function computation with normalisation in a
/// single pass to avoid a separate allocation for d(τ).
///
/// d(τ) = Σ\_{j=0}^{W/2-1} (x\[j\] - x\[j+τ\])²
/// d'(τ) = d(τ) / ((1/τ) × Σ\_{j=1}^{τ} d(j))    for τ > 0
/// d'(0) = 1.0
fn yin_cmnd(window: &[f32], max_tau: usize) -> Vec<f32> {
    let w = window.len() / 2;
    let tau_limit = max_tau.min(w);

    let mut d_prime = Vec::with_capacity(tau_limit.saturating_add(1));
    d_prime.push(1.0); // d'(0) = 1 by convention

    let mut running_sum: f64 = 0.0;

    for tau in 1..=tau_limit {
        // Compute d(tau).
        let mut sum: f64 = 0.0;
        for j in 0..w {
            let a = window.get(j).copied().unwrap_or(0.0) as f64;
            let b = window.get(j.saturating_add(tau)).copied().unwrap_or(0.0) as f64;
            let diff = a - b;
            sum += diff * diff;
        }

        running_sum += sum;

        if running_sum.abs() < f64::EPSILON {
            d_prime.push(1.0);
        } else {
            let normalised = sum * (tau as f64) / running_sum;
            d_prime.push(normalised as f32);
        }
    }

    d_prime
}

/// Parabolic interpolation around the minimum for sub-sample accuracy.
///
/// Given d'(tau-1), d'(tau), d'(tau+1), refines `tau` to a fractional value
/// by fitting a parabola through the three points and returning the vertex.
fn parabolic_interpolation(d_prime: &[f32], tau: usize) -> f32 {
    if tau == 0 || tau.saturating_add(1) >= d_prime.len() {
        return tau as f32;
    }

    let s0 = d_prime.get(tau.saturating_sub(1)).copied().unwrap_or(1.0) as f64;
    let s1 = d_prime.get(tau).copied().unwrap_or(1.0) as f64;
    let s2 = d_prime.get(tau.saturating_add(1)).copied().unwrap_or(1.0) as f64;

    let denominator = 2.0 * (2.0 * s1 - s2 - s0);
    if denominator.abs() < f64::EPSILON {
        return tau as f32;
    }

    let adjustment = (s2 - s0) / denominator;
    (tau as f64 + adjustment) as f32
}

/// Converts a frequency in Hz to the nearest MIDI note number.
///
/// MIDI note 69 = A4 = 440 Hz.
/// Returns `None` if the frequency maps outside the 0–127 range.
pub fn freq_to_midi_note(freq_hz: f32) -> Option<u8> {
    if !freq_hz.is_finite() || freq_hz <= 0.0 {
        return None;
    }
    let note_f = 69.0 + 12.0 * (freq_hz / 440.0).log2();
    let note = note_f.round() as i32;
    if !(0..=127).contains(&note) {
        return None;
    }
    Some(note as u8)
}

/// Converts a MIDI note number to its standard frequency in Hz.
///
/// MIDI note 69 = A4 = 440.0 Hz.
pub fn midi_note_to_freq(note: u8) -> f32 {
    440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freq_to_midi_known_values() {
        assert_eq!(freq_to_midi_note(440.0), Some(69)); // A4
        assert_eq!(freq_to_midi_note(261.63), Some(60)); // C4
        assert_eq!(freq_to_midi_note(880.0), Some(81)); // A5
        assert_eq!(freq_to_midi_note(32.70), Some(24)); // C1
        assert_eq!(freq_to_midi_note(4186.0), Some(108)); // C8
    }

    #[test]
    fn freq_to_midi_out_of_range() {
        assert_eq!(freq_to_midi_note(0.0), None);
        assert_eq!(freq_to_midi_note(-1.0), None);
        assert_eq!(freq_to_midi_note(f32::NAN), None);
        assert_eq!(freq_to_midi_note(f32::INFINITY), None);
    }

    #[test]
    fn midi_note_roundtrip() {
        for note in 21..=108 {
            let freq = midi_note_to_freq(note);
            let back = freq_to_midi_note(freq);
            assert_eq!(back, Some(note), "roundtrip failed for note {note}");
        }
    }

    #[test]
    fn detect_a440_sine() {
        let sample_rate = 44100u32;
        let duration = 0.25;
        let num_samples = (sample_rate as f32 * duration) as usize;
        let freq = 440.0;
        let samples: Vec<f32> = (0..num_samples)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                (2.0 * std::f32::consts::PI * freq * t).sin()
            })
            .collect();

        let pitches = detect_pitches(&samples, sample_rate, 2048, 512, 0.15, 80.0, 2000.0);
        assert!(!pitches.is_empty(), "should detect frames");

        // At least half the frames should detect ~440 Hz.
        let detected: Vec<f32> = pitches.iter().filter_map(|p| *p).collect();
        assert!(
            !detected.is_empty(),
            "should detect pitch in A440 sine wave"
        );
        for &hz in &detected {
            let note = freq_to_midi_note(hz);
            assert_eq!(note, Some(69), "expected A4 (69), got {hz} Hz → {note:?}");
        }
    }

    #[test]
    fn detect_c4_sine() {
        let sample_rate = 44100u32;
        let freq = 261.63;
        let num_samples = (sample_rate as f32 * 0.25) as usize;
        let samples: Vec<f32> = (0..num_samples)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                (2.0 * std::f32::consts::PI * freq * t).sin()
            })
            .collect();

        let pitches = detect_pitches(&samples, sample_rate, 2048, 512, 0.15, 80.0, 2000.0);
        let detected: Vec<f32> = pitches.iter().filter_map(|p| *p).collect();
        assert!(!detected.is_empty());
        for &hz in &detected {
            let note = freq_to_midi_note(hz);
            assert_eq!(note, Some(60), "expected C4 (60), got {hz} Hz → {note:?}");
        }
    }

    #[test]
    fn silence_returns_none() {
        let samples = vec![0.0f32; 4096];
        let pitches = detect_pitches(&samples, 44100, 2048, 512, 0.15, 80.0, 2000.0);
        // Silence should have no detected pitches (all None).
        for p in &pitches {
            assert!(p.is_none(), "silence should not detect pitch: {p:?}");
        }
    }

    #[test]
    fn empty_input_returns_empty() {
        let pitches = detect_pitches(&[], 44100, 2048, 512, 0.15, 80.0, 2000.0);
        assert!(pitches.is_empty());
    }

    #[test]
    fn zero_hop_returns_empty() {
        let samples = vec![0.0f32; 4096];
        let pitches = detect_pitches(&samples, 44100, 2048, 0, 0.15, 80.0, 2000.0);
        assert!(pitches.is_empty());
    }

    #[test]
    fn zero_sample_rate_returns_empty() {
        let samples = vec![0.0f32; 4096];
        let pitches = detect_pitches(&samples, 0, 2048, 512, 0.15, 80.0, 2000.0);
        assert!(pitches.is_empty());
    }

    #[test]
    fn input_shorter_than_window_returns_empty() {
        let samples = vec![0.5f32; 100];
        let pitches = detect_pitches(&samples, 44100, 2048, 512, 0.15, 80.0, 2000.0);
        assert!(pitches.is_empty());
    }

    #[test]
    fn parabolic_interpolation_center() {
        // A symmetric parabola centered at tau=5 should return 5.0.
        let d = vec![1.0, 0.8, 0.5, 0.3, 0.15, 0.05, 0.15, 0.3, 0.5, 0.8];
        let result = parabolic_interpolation(&d, 5);
        assert!((result - 5.0).abs() < 0.01, "expected ~5.0, got {result}");
    }
}
