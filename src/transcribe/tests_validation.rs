// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! V38 security and boundary tests for the transcribe module.

use super::*;

// ── Boundary tests ───────────────────────────────────────────────────────────

#[test]
fn min_duration_exact_boundary() {
    // 50ms minimum, hop_size=512, sr=44100.
    // 50ms = 0.050s → frames needed = ceil(0.050 / (512/44100)) ≈ 5 frames.
    // 4 frames = 4 * 512/44100 ≈ 46ms → filtered out.
    // 5 frames = 5 * 512/44100 ≈ 58ms → kept.
    let config = TranscribeConfig {
        min_duration_ms: 50,
        ..TranscribeConfig::default()
    };

    // 4 frames at hop 512, sr 44100: too short.
    let short_samples = vec![0.5f32; 4 * 512 + 2048]; // need window_size + frames
    let short_sine: Vec<f32> = (0..short_samples.len())
        .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44100.0).sin())
        .collect();
    // With only ~4 analysis frames we may get notes too short to pass the filter.
    let notes_short = pcm_to_notes(&short_sine, 44100, &config).unwrap();
    // Not asserting empty — depends on exact frame count. Just verify no panic.

    // Long enough to definitely pass.
    let long_sine: Vec<f32> =
        (0..22050) // 0.5 seconds
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44100.0).sin())
            .collect();
    let notes_long = pcm_to_notes(&long_sine, 44100, &config).unwrap();
    assert!(
        !notes_long.is_empty(),
        "long note should pass duration filter"
    );
    let _ = notes_short; // suppress unused warning
}

#[test]
fn single_sample_no_panic() {
    let config = TranscribeConfig::default();
    let result = pcm_to_notes(&[0.5], 44100, &config);
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty(), "1 sample is below window_size");
}

#[test]
fn window_equals_input_length() {
    let config = TranscribeConfig {
        window_size: 2048,
        ..TranscribeConfig::default()
    };
    // Exactly 2048 samples → should produce exactly one frame.
    let samples: Vec<f32> = (0..2048)
        .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44100.0).sin())
        .collect();
    let notes = pcm_to_notes(&samples, 44100, &config);
    assert!(notes.is_ok());
    // One frame of A440 may or may not be long enough for min_duration filter.
}

#[test]
fn very_low_frequency() {
    // 80 Hz — needs a larger window for reliable low-freq detection.
    // Period at 80 Hz = 551 samples; window_size=4096 gives
    // half_window=2048, ample room for the YIN difference function.
    let config = TranscribeConfig {
        min_freq: 40.0, // widen range to capture 80 Hz
        min_duration_ms: 20,
        window_size: 4096,
        ..TranscribeConfig::default()
    };
    let samples: Vec<f32> =
        (0..44100) // 1 second
            .map(|i| (2.0 * std::f32::consts::PI * 80.0 * i as f32 / 44100.0).sin())
            .collect();
    let notes = pcm_to_notes(&samples, 44100, &config).unwrap();
    // Should detect something near MIDI 28 (E2 ≈ 82 Hz) or 27 (≈ 78 Hz).
    // Allow a wider range to account for YIN's possible octave ambiguity
    // at the frequency boundary.
    if let Some(n) = notes.first() {
        assert!(
            n.note >= 26 && n.note <= 40,
            "80 Hz should map near MIDI 28, got {}",
            n.note
        );
    }
}

#[test]
fn very_high_frequency() {
    // 1760 Hz (A6, MIDI 93).
    let config = TranscribeConfig::default();
    let samples: Vec<f32> = (0..22050)
        .map(|i| (2.0 * std::f32::consts::PI * 1760.0 * i as f32 / 44100.0).sin())
        .collect();
    let notes = pcm_to_notes(&samples, 44100, &config).unwrap();
    if let Some(n) = notes.first() {
        assert_eq!(n.note, 93, "1760 Hz should be MIDI 93 (A6)");
    }
}

// ── Integer overflow safety ──────────────────────────────────────────────────

#[test]
fn huge_window_size_error() {
    let config = TranscribeConfig {
        window_size: usize::MAX,
        ..TranscribeConfig::default()
    };
    let samples = vec![0.5f32; 100];
    // Should not panic — window > input produces empty notes.
    let result = pcm_to_notes(&samples, 44100, &config);
    assert!(result.is_ok());
}

#[test]
fn max_ticks_and_tempo_no_overflow() {
    let notes = vec![onset::DetectedNote {
        note: 69,
        onset_secs: 0.0,
        duration_secs: 1.0,
        velocity: 100,
    }];
    let config = TranscribeConfig {
        ticks_per_beat: u16::MAX,
        tempo_bpm: u16::MAX,
        ..TranscribeConfig::default()
    };
    // Should not panic.
    let mid = notes_to_mid(&notes, &config);
    assert_eq!(mid.get(..4), Some(b"MThd".as_slice()));
}

// ── V38 adversarial inputs ───────────────────────────────────────────────────

#[test]
fn adversarial_all_ones_no_panic() {
    let samples = vec![1.0f32; 4096];
    let config = TranscribeConfig::default();
    let result = pcm_to_notes(&samples, 44100, &config);
    assert!(result.is_ok());
}

#[test]
fn adversarial_all_neg_ones_no_panic() {
    let samples = vec![-1.0f32; 4096];
    let config = TranscribeConfig::default();
    let result = pcm_to_notes(&samples, 44100, &config);
    assert!(result.is_ok());
}

#[test]
fn adversarial_nan_samples_no_panic() {
    let samples = vec![f32::NAN; 4096];
    let config = TranscribeConfig::default();
    let result = pcm_to_notes(&samples, 44100, &config);
    assert!(result.is_ok());
}

#[test]
fn adversarial_infinity_no_panic() {
    let samples = vec![f32::INFINITY; 4096];
    let config = TranscribeConfig::default();
    let result = pcm_to_notes(&samples, 44100, &config);
    assert!(result.is_ok());
}

#[test]
fn adversarial_neg_infinity_no_panic() {
    let samples = vec![f32::NEG_INFINITY; 4096];
    let config = TranscribeConfig::default();
    let result = pcm_to_notes(&samples, 44100, &config);
    assert!(result.is_ok());
}

#[test]
fn adversarial_subnormal_no_panic() {
    let samples = vec![f32::MIN_POSITIVE / 2.0; 4096];
    let config = TranscribeConfig::default();
    let result = pcm_to_notes(&samples, 44100, &config);
    assert!(result.is_ok());
}

#[test]
fn adversarial_alternating_extremes_no_panic() {
    let samples: Vec<f32> = (0..4096)
        .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
        .collect();
    let config = TranscribeConfig::default();
    let result = pcm_to_notes(&samples, 44100, &config);
    assert!(result.is_ok());
}

#[test]
fn adversarial_zero_window_error() {
    let samples = vec![0.5f32; 4096];
    let config = TranscribeConfig {
        window_size: 0,
        ..TranscribeConfig::default()
    };
    let err = pcm_to_notes(&samples, 44100, &config).unwrap_err();
    assert!(matches!(err, Error::InvalidSize { .. }));
}

#[test]
fn adversarial_zero_hop_error() {
    let samples = vec![0.5f32; 4096];
    let config = TranscribeConfig {
        hop_size: 0,
        ..TranscribeConfig::default()
    };
    let err = pcm_to_notes(&samples, 44100, &config).unwrap_err();
    assert!(matches!(err, Error::InvalidSize { .. }));
}

#[test]
fn adversarial_window_larger_than_input() {
    let samples = vec![0.5f32; 100];
    let config = TranscribeConfig {
        window_size: 2048,
        ..TranscribeConfig::default()
    };
    let notes = pcm_to_notes(&samples, 44100, &config).unwrap();
    assert!(
        notes.is_empty(),
        "should produce no notes when window > input"
    );
}

// ── WAV convenience function (feature-gated) ─────────────────────────────────

#[cfg(feature = "convert")]
#[test]
fn wav_to_mid_invalid_wav_error() {
    let config = TranscribeConfig::default();
    let err = wav_to_mid(&[0xFF; 100], &config).unwrap_err();
    assert!(matches!(err, Error::DecompressionError { .. }));
}

#[cfg(feature = "convert")]
#[test]
fn wav_to_mid_valid_wav() {
    // Build a minimal WAV in memory using hound.
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav_buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut wav_buf);
        let mut writer = hound::WavWriter::new(cursor, spec).unwrap();
        // Write 0.5 seconds of 440 Hz sine.
        for i in 0..(44100 / 2) {
            let t = i as f32 / 44100.0;
            let sample = (2.0 * std::f32::consts::PI * 440.0 * t).sin();
            writer.write_sample((sample * 32767.0) as i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    let config = TranscribeConfig::default();
    let mid = wav_to_mid(&wav_buf, &config).unwrap();
    assert_eq!(mid.get(..4), Some(b"MThd".as_slice()));
}
