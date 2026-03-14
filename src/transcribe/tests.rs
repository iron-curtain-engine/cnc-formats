// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Tests for the transcribe module — functionality, error paths, Display,
//! and determinism.

use super::*;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Generates a mono sine wave at a given frequency.
fn build_sine(freq_hz: f32, duration_secs: f32, sample_rate: u32) -> Vec<f32> {
    let num_samples = (sample_rate as f32 * duration_secs) as usize;
    (0..num_samples)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            (2.0 * std::f32::consts::PI * freq_hz * t).sin()
        })
        .collect()
}

/// Generates a two-note sequence: `freq1` for `dur` seconds, then `freq2`.
fn build_two_notes(freq1: f32, freq2: f32, dur: f32, sample_rate: u32) -> Vec<f32> {
    let mut samples = build_sine(freq1, dur, sample_rate);
    samples.extend(build_sine(freq2, dur, sample_rate));
    samples
}

/// Generates silence (zeros).
fn build_silence(duration_secs: f32, sample_rate: u32) -> Vec<f32> {
    vec![0.0f32; (sample_rate as f32 * duration_secs) as usize]
}

// ── Basic functionality ──────────────────────────────────────────────────────

#[test]
fn detect_single_a440() {
    let samples = build_sine(440.0, 0.5, 44100);
    let config = TranscribeConfig::default();
    let notes = pcm_to_notes(&samples, 44100, &config).unwrap();
    assert!(!notes.is_empty(), "should detect A4");
    assert_eq!(notes.first().map(|n| n.note), Some(69));
}

#[test]
fn detect_single_c4() {
    let samples = build_sine(261.63, 0.5, 44100);
    let config = TranscribeConfig::default();
    let notes = pcm_to_notes(&samples, 44100, &config).unwrap();
    assert!(!notes.is_empty(), "should detect C4");
    assert_eq!(notes.first().map(|n| n.note), Some(60));
}

#[test]
fn detect_two_consecutive_notes() {
    let samples = build_two_notes(440.0, 523.25, 0.3, 44100);
    let config = TranscribeConfig::default();
    let notes = pcm_to_notes(&samples, 44100, &config).unwrap();
    assert!(
        notes.len() >= 2,
        "should detect at least 2 notes, got {}",
        notes.len()
    );
    assert_eq!(notes.first().map(|n| n.note), Some(69)); // A4
    assert_eq!(notes.get(1).map(|n| n.note), Some(72)); // C5
}

#[test]
fn pcm_to_mid_produces_valid_smf() {
    let samples = build_sine(440.0, 0.5, 44100);
    let config = TranscribeConfig::default();
    let mid = pcm_to_mid(&samples, 44100, &config).unwrap();
    // Valid SMF starts with MThd.
    assert_eq!(mid.get(..4), Some(b"MThd".as_slice()));
    // Contains MTrk.
    let has_mtrk = mid.windows(4).any(|w| w == b"MTrk");
    assert!(has_mtrk, "should contain MTrk chunk");
}

#[test]
fn pcm_to_notes_sorted_by_onset() {
    let samples = build_two_notes(440.0, 523.25, 0.3, 44100);
    let config = TranscribeConfig::default();
    let notes = pcm_to_notes(&samples, 44100, &config).unwrap();
    for window in notes.windows(2) {
        assert!(
            window[0].onset_secs <= window[1].onset_secs,
            "notes should be sorted by onset"
        );
    }
}

#[test]
fn notes_to_mid_roundtrip_event_count() {
    let notes = vec![
        DetectedNote {
            note: 60,
            onset_secs: 0.0,
            duration_secs: 0.25,
            velocity: 100,
        },
        DetectedNote {
            note: 64,
            onset_secs: 0.3,
            duration_secs: 0.25,
            velocity: 90,
        },
        DetectedNote {
            note: 67,
            onset_secs: 0.6,
            duration_secs: 0.25,
            velocity: 80,
        },
    ];
    let config = TranscribeConfig::default();
    let mid = notes_to_mid(&notes, &config);
    // Should have 3 Note-On and 3 Note-Off events.
    let note_on_count = mid.windows(1).filter(|w| w[0] & 0xF0 == 0x90).count();
    let note_off_count = mid.windows(1).filter(|w| w[0] & 0xF0 == 0x80).count();
    // The raw byte scan will overcount (data bytes may match 0x9x/0x8x).
    // Instead, scan for status + known note pairs.
    let note_ons: Vec<_> = mid
        .windows(3)
        .filter(|w| w[0] == 0x90 && (w[1] == 60 || w[1] == 64 || w[1] == 67))
        .collect();
    let note_offs: Vec<_> = mid
        .windows(3)
        .filter(|w| w[0] == 0x80 && (w[1] == 60 || w[1] == 64 || w[1] == 67))
        .collect();
    assert_eq!(note_ons.len(), 3, "expected 3 Note-On events");
    assert_eq!(note_offs.len(), 3, "expected 3 Note-Off events");
    // Suppress unused variable warnings.
    let _ = (note_on_count, note_off_count);
}

#[test]
fn silence_produces_empty_notes() {
    let samples = build_silence(0.5, 44100);
    let config = TranscribeConfig::default();
    let notes = pcm_to_notes(&samples, 44100, &config).unwrap();
    assert!(notes.is_empty(), "silence should produce no notes");
}

#[test]
fn freq_to_midi_note_known_values() {
    assert_eq!(freq_to_midi_note(440.0), Some(69));
    assert_eq!(freq_to_midi_note(261.63), Some(60));
    assert_eq!(freq_to_midi_note(880.0), Some(81));
    assert_eq!(freq_to_midi_note(32.70), Some(24));
}

#[test]
fn config_defaults() {
    let c = TranscribeConfig::default();
    assert!((c.yin_threshold - 0.15).abs() < 0.001);
    assert_eq!(c.window_size, 2048);
    assert_eq!(c.hop_size, 512);
    assert_eq!(c.ticks_per_beat, 480);
    assert_eq!(c.tempo_bpm, 120);
    assert_eq!(c.channel, 0);
    assert_eq!(c.velocity, 100);
    assert!(!c.estimate_velocity);
}

// ── Error paths ──────────────────────────────────────────────────────────────

#[test]
fn pcm_to_mid_empty_samples_error() {
    let config = TranscribeConfig::default();
    let err = pcm_to_mid(&[], 44100, &config).unwrap_err();
    match err {
        Error::InvalidSize { value: 0, .. } => {}
        other => panic!("expected InvalidSize, got {other:?}"),
    }
}

#[test]
fn pcm_to_mid_zero_sample_rate_error() {
    let samples = vec![0.5f32; 4096];
    let config = TranscribeConfig::default();
    let err = pcm_to_mid(&samples, 0, &config).unwrap_err();
    match err {
        Error::InvalidSize { value: 0, .. } => {}
        other => panic!("expected InvalidSize, got {other:?}"),
    }
}

#[test]
fn pcm_to_notes_empty_error() {
    let config = TranscribeConfig::default();
    let err = pcm_to_notes(&[], 44100, &config).unwrap_err();
    assert!(matches!(err, Error::InvalidSize { .. }));
}

#[test]
fn pcm_to_notes_zero_window_error() {
    let samples = vec![0.5f32; 4096];
    let config = TranscribeConfig {
        window_size: 0,
        ..TranscribeConfig::default()
    };
    let err = pcm_to_notes(&samples, 44100, &config).unwrap_err();
    assert!(matches!(err, Error::InvalidSize { .. }));
}

#[test]
fn pcm_to_notes_zero_hop_error() {
    let samples = vec![0.5f32; 4096];
    let config = TranscribeConfig {
        hop_size: 0,
        ..TranscribeConfig::default()
    };
    let err = pcm_to_notes(&samples, 44100, &config).unwrap_err();
    assert!(matches!(err, Error::InvalidSize { .. }));
}

// ── Error Display ────────────────────────────────────────────────────────────

#[test]
fn error_display_contains_context() {
    let config = TranscribeConfig::default();
    let err = pcm_to_mid(&[], 44100, &config).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("transcribe"),
        "error should mention transcribe context: {msg}"
    );
}

#[test]
fn error_display_contains_values() {
    let config = TranscribeConfig::default();
    let err = pcm_to_mid(&[], 44100, &config).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains('0'), "should mention the value: {msg}");
}

// ── Determinism ──────────────────────────────────────────────────────────────

#[test]
fn pcm_to_mid_deterministic() {
    let samples = build_sine(440.0, 0.5, 44100);
    let config = TranscribeConfig::default();
    let mid1 = pcm_to_mid(&samples, 44100, &config).unwrap();
    let mid2 = pcm_to_mid(&samples, 44100, &config).unwrap();
    assert_eq!(mid1, mid2, "same input should produce identical output");
}

#[test]
fn pcm_to_notes_deterministic() {
    let samples = build_sine(440.0, 0.5, 44100);
    let config = TranscribeConfig::default();
    let n1 = pcm_to_notes(&samples, 44100, &config).unwrap();
    let n2 = pcm_to_notes(&samples, 44100, &config).unwrap();
    assert_eq!(n1, n2, "same input should produce identical notes");
}

// ── XMI conversion ──────────────────────────────────────────────────────────

#[cfg(feature = "xmi")]
#[test]
fn mid_to_xmi_produces_valid_iff() {
    let notes = vec![DetectedNote {
        note: 69,
        onset_secs: 0.0,
        duration_secs: 0.5,
        velocity: 100,
    }];
    let config = TranscribeConfig::default();
    let mid = notes_to_mid(&notes, &config);
    let xmi = mid_to_xmi(&mid).unwrap();
    // Should start with FORM:XDIR.
    assert_eq!(xmi.get(..4), Some(b"FORM".as_slice()));
    assert_eq!(xmi.get(8..12), Some(b"XDIR".as_slice()));
    // Should contain CAT:XMID.
    let has_cat = xmi.windows(4).any(|w| w == b"CAT ");
    assert!(has_cat, "should contain CAT chunk");
    let has_xmid = xmi.windows(4).any(|w| w == b"XMID");
    assert!(has_xmid, "should contain XMID");
    // Should contain EVNT chunk.
    let has_evnt = xmi.windows(4).any(|w| w == b"EVNT");
    assert!(has_evnt, "should contain EVNT chunk");
}

#[cfg(feature = "xmi")]
#[test]
fn mid_to_xmi_invalid_input_error() {
    let err = mid_to_xmi(&[0u8; 10]).unwrap_err();
    assert!(matches!(err, Error::InvalidMagic { .. }));
}
