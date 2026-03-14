// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

// ── Basic functionality ──────────────────────────────────────────────────────

/// Build a minimal valid Type 0 MIDI file (single track, one note).
///
/// Layout: MThd header (14 bytes) + MTrk header (8 bytes) + events.
/// This produces a structurally valid SMF that `midly` will parse.
fn build_minimal_mid() -> Vec<u8> {
    let mut buf = Vec::new();

    // ── MThd chunk ──
    buf.extend_from_slice(b"MThd"); // magic
    buf.extend_from_slice(&0x00000006u32.to_be_bytes()); // chunk length = 6
    buf.extend_from_slice(&0x0000u16.to_be_bytes()); // format = 0 (single track)
    buf.extend_from_slice(&0x0001u16.to_be_bytes()); // num tracks = 1
    buf.extend_from_slice(&0x0060u16.to_be_bytes()); // ticks per beat = 96

    // ── MTrk chunk ──
    // Events: Note-On C4 vel=100 at delta=0, Note-Off C4 at delta=96,
    // End-of-Track meta event.
    let track_data: Vec<u8> = vec![
        0x00, 0x90, 0x3C, 0x64, // delta=0, Note-On ch0 C4 vel=100
        0x60, 0x80, 0x3C, 0x00, // delta=96, Note-Off ch0 C4
        0x00, 0xFF, 0x2F, 0x00, // delta=0, End of Track
    ];
    buf.extend_from_slice(b"MTrk");
    buf.extend_from_slice(&(track_data.len() as u32).to_be_bytes());
    buf.extend_from_slice(&track_data);

    buf
}

/// Parsing a minimal valid MIDI file succeeds and reports correct metadata.
///
/// Why: baseline golden-path test confirming the MThd header fields (format,
/// track count, timing) are correctly extracted by the `midly` wrapper.
#[test]
fn parse_minimal_mid() {
    let data = build_minimal_mid();
    let mid = MidFile::parse(&data).unwrap();
    assert!(matches!(mid.format(), MidiFormat::SingleTrack));
    assert_eq!(mid.track_count(), 1);
    assert_eq!(mid.event_count(), 3); // Note-On + Note-Off + EndOfTrack
}

/// Timing is correctly reported as Metrical with the expected ticks-per-beat.
///
/// Why: confirms the timing field extraction matches the encoded 96 tpb value.
#[test]
fn parse_timing() {
    let data = build_minimal_mid();
    let mid = MidFile::parse(&data).unwrap();
    match mid.timing() {
        Timing::Metrical(tpb) => assert_eq!(tpb.as_int(), 96),
        _ => panic!("expected Metrical timing"),
    }
}

/// Channels used reports channel 0 for a single-channel MIDI file.
///
/// Why: verifies channel scanning correctly identifies used MIDI channels.
#[test]
fn channels_used_single_channel() {
    let data = build_minimal_mid();
    let mid = MidFile::parse(&data).unwrap();
    let channels = mid.channels_used();
    assert_eq!(channels, vec![0]);
}

/// Duration estimation returns a positive value for a file with notes.
///
/// Why: confirms the tempo-based duration calculation doesn't return zero
/// for a valid MIDI file (defaults to 120 BPM when no tempo event present).
#[test]
fn duration_positive() {
    let data = build_minimal_mid();
    let mid = MidFile::parse(&data).unwrap();
    let dur = mid.duration_secs();
    // 96 ticks at 120 BPM (default) with 96 tpb = 0.5 seconds.
    assert!(dur > 0.0, "duration should be positive: {dur}");
    assert!((dur - 0.5).abs() < 0.01, "expected ~0.5s, got {dur}");
}

/// Write round-trips: parse → write → re-parse produces the same metadata.
///
/// Why: confirms the midly write path produces a valid SMF that re-parses
/// with identical track count and event count.
#[test]
fn write_roundtrip() {
    let data = build_minimal_mid();
    let mid = MidFile::parse(&data).unwrap();
    let written = write(&mid).unwrap();
    let mid2 = MidFile::parse(&written).unwrap();
    assert_eq!(mid.track_count(), mid2.track_count());
    assert_eq!(mid.event_count(), mid2.event_count());
}

// ── Error paths ──────────────────────────────────────────────────────────────

/// Parsing fewer than 14 bytes returns UnexpectedEof.
///
/// Why: the minimum SMF is 14 bytes (MThd header).  Anything shorter must
/// be rejected before reaching midly.
#[test]
fn parse_too_short() {
    let err = MidFile::parse(&[0u8; 10]).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { needed: 14, .. }));
}

/// Parsing data without the MThd magic returns InvalidMagic.
///
/// Why: non-MIDI files should be cleanly rejected with our error type.
#[test]
fn parse_bad_magic() {
    let data = [0u8; 20];
    let err = MidFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidMagic { context: "MIDI" }));
}

/// An empty input returns UnexpectedEof.
///
/// Why: zero-length input is the most basic truncation case.
#[test]
fn parse_empty() {
    let err = MidFile::parse(&[]).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

// ── Error Display verification ───────────────────────────────────────────────

/// `Error::Display` for MID UnexpectedEof includes numeric byte counts.
///
/// Why: the Display output is the user-facing diagnostic message; it must
/// include `needed` and `available` so the user can diagnose truncation.
#[test]
fn error_display_eof_contains_byte_counts() {
    let err = MidFile::parse(&[0u8; 10]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("14"), "should mention needed bytes: {msg}");
    assert!(msg.contains("10"), "should mention available bytes: {msg}");
}

/// `Error::Display` for MID InvalidMagic includes the format context.
///
/// Why: when a non-MIDI file is rejected, the error message must identify
/// which format validation failed.
#[test]
fn error_display_magic_contains_context() {
    let err = MidFile::parse(&[0u8; 20]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("MIDI"), "should mention MIDI context: {msg}");
}

// ── Determinism ──────────────────────────────────────────────────────────────

/// Parsing the same input twice produces identical results.
///
/// Why: parsers must be pure functions of their input (AGENTS.md).
#[test]
fn parse_deterministic() {
    let data = build_minimal_mid();
    let a = MidFile::parse(&data).unwrap();
    let b = MidFile::parse(&data).unwrap();
    assert_eq!(a.track_count(), b.track_count());
    assert_eq!(a.event_count(), b.event_count());
    assert!((a.duration_secs() - b.duration_secs()).abs() < f64::EPSILON);
}

// ── Security edge-case tests (V38) ──────────────────────────────────────────

/// `MidFile::parse` on 256 bytes of `0xFF` must not panic.
///
/// Why (V38): an all-ones buffer maximises every header field, exercising
/// overflow guards and bounds checks in midly's parser.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 256];
    let _ = MidFile::parse(&data);
}

/// `MidFile::parse` on 256 bytes of `0x00` must not panic.
///
/// Why (V38): all-zero exercises zero-dimension paths and degenerate headers.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = vec![0x00u8; 256];
    let _ = MidFile::parse(&data);
}

// ── SoundFont / render error paths ──────────────────────────────────────────

/// Invalid SoundFont bytes are rejected by the SF2 loader.
///
/// Why: `load_soundfont()` is the public entry point for render inputs and
/// must surface a clean error for non-SF2 data.
#[test]
fn load_soundfont_rejects_invalid_bytes() {
    let err = load_soundfont(b"not an sf2").unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidMagic {
            context: "SoundFont"
        }
    ));
}

/// Render-path MIDI parsing rejects non-SMF input.
///
/// Why: `render_to_pcm()` uses rustysynth's MIDI loader internally, so the
/// render path needs its own invalid-MIDI coverage separate from `MidFile`.
#[test]
fn render_path_rejects_invalid_midi_bytes() {
    let err = parse_render_midi(b"not a midi").unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidMagic {
            context: "MIDI (rustysynth)"
        }
    ));
}

/// Render-path sample-rate validation rejects unsupported values.
///
/// Why: rustysynth only supports 16–192 kHz synthesis.  Out-of-range rates
/// must fail before any audio rendering work begins.
#[test]
fn render_path_rejects_out_of_range_sample_rate() {
    let err = validate_render_sample_rate(8_000).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidMagic {
            context: "SoundFont synthesizer"
        }
    ));
}
