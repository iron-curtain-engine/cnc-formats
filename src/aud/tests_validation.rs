// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

// ── Error field & Display verification ────────────────────────────────

/// `UnexpectedEof` for a too-short header carries the exact byte counts.
///
/// Why: structured error fields let callers generate precise diagnostics.
/// A 5-byte input needs 12 (header size) and has only 5 available.
#[test]
fn eof_error_carries_header_byte_counts() {
    let err = AudFile::parse(&[0u8; 5]).unwrap_err();
    match err {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, 12, "AUD header is 12 bytes");
            assert_eq!(available, 5);
        }
        other => panic!("Expected UnexpectedEof, got: {other}"),
    }
}

/// `UnexpectedEof` for a truncated payload includes the total needed
/// bytes (header + compressed_size).
///
/// Why: this tests the `saturating_add` path — the `needed` field should
/// report `12 + 500 = 512` even though only 22 bytes are available.
#[test]
fn eof_error_for_truncated_payload_carries_total_needed() {
    let mut bytes =
        super::tests::make_header_bytes(22050, 500, 1000, AUD_FLAG_16BIT, SCOMP_WESTWOOD);
    bytes.extend_from_slice(&[0u8; 10]);
    let err = AudFile::parse(&bytes).unwrap_err();
    match err {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, 512, "need 12-byte header + 500 compressed bytes");
            assert_eq!(available, 22, "12-byte header + 10 payload bytes");
        }
        other => panic!("Expected UnexpectedEof, got: {other}"),
    }
}

/// `Error::Display` embeds the numeric context for human-readable output.
///
/// Why: the Display trait output is the user-facing message; it must
/// include `needed` and `available` byte counts for diagnostics.
#[test]
fn eof_display_contains_byte_counts() {
    let err = AudFile::parse(&[0u8; 5]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("12"), "should mention needed bytes: {msg}");
    assert!(msg.contains('5'), "should mention available bytes: {msg}");
}

// ── Determinism ──────────────────────────────────────────────────────

/// Parsing the same AUD bytes twice yields identical results.
///
/// Why: the parser is a pure function of its input; any hidden state that
/// leaked between calls would break reproducibility and make test
/// failures non-deterministic.
#[test]
fn parse_is_deterministic() {
    let mut bytes = super::tests::make_header_bytes(22050, 4, 16, AUD_FLAG_16BIT, SCOMP_WESTWOOD);
    bytes.extend_from_slice(&[0x07, 0x07, 0x07, 0x07]);
    let a = AudFile::parse(&bytes).unwrap();
    let b = AudFile::parse(&bytes).unwrap();
    assert_eq!(a, b);
}

/// Decoding the same compressed data twice yields identical PCM output.
///
/// Why: the ADPCM decoder carries per-channel state (`sample`,
/// `step_index`) internally; both must be re-initialised to zero on each
/// call.  This catches any accidental state reuse.
#[test]
fn adpcm_decode_is_deterministic() {
    let compressed = vec![0x77u8; 50];
    let a = decode_adpcm(&compressed, false, 0);
    let b = decode_adpcm(&compressed, false, 0);
    assert_eq!(a, b);
}

// ── Boundary tests ──────────────────────────────────────────────────

/// AUD with `compressed_size == 0` is a valid header-only file.
///
/// Why: boundary test for the smallest well-formed AUD.  The parser must
/// accept this and expose an empty `compressed_data` slice.
#[test]
fn parse_zero_compressed_size_is_valid() {
    let bytes = super::tests::make_header_bytes(22050, 0, 0, 0, SCOMP_WESTWOOD);
    let aud = AudFile::parse(&bytes).unwrap();
    assert_eq!(aud.header.compressed_size, 0);
    assert!(aud.compressed_data.is_empty());
}

/// Decoding zero-length stereo compressed data produces no samples.
///
/// Why: ensures the stereo interleave loop handles an empty iterator
/// without panic or accidental output.
#[test]
fn adpcm_zero_length_produces_no_samples() {
    let samples = decode_adpcm(&[], true, 0);
    assert!(samples.is_empty());
}

// ── Known ADPCM output verification ──────────────────────────────────
//
// These tests verify the IMA ADPCM decoder produces specific known
// sample values, ensuring compatibility with the standard IMA algorithm.

/// Full nibble sequence [7, 0, 15, 0] produces a known sample trajectory.
///
/// Nibble 7 at step_index 0 (step=7):  diff = +11, sample =  11, step_index → 8
/// Nibble 0 at step_index 8 (step=16): diff =  +2, sample =  13, step_index → 7
/// Nibble F at step_index 7 (step=14): diff = −25, sample = −12, step_index → 15
/// Nibble 0 at step_index 15 (step=31): diff = +3, sample =  −9, step_index → 14
#[test]
fn adpcm_known_sample_trajectory() {
    // byte 0x07: low=7, high=0; byte 0x0F: low=15, high=0
    let compressed = [0x07u8, 0x0F];
    let samples = decode_adpcm(&compressed, false, 0);
    assert_eq!(samples.len(), 4);
    assert_eq!(samples[0], 11);
    assert_eq!(samples[1], 13);
    assert_eq!(samples[2], -12);
    assert_eq!(samples[3], -9);
}

// ── Compression ID coverage ──────────────────────────────────────────

/// Parser accepts `SCOMP_NONE` (ID 0) without error.
///
/// Why: the parser is compression-agnostic; it stores the raw ID and
/// leaves interpretation to the caller.  Each known ID must be accepted.
#[test]
fn parse_accepts_scomp_none() {
    let bytes = super::tests::make_header_bytes(22050, 0, 0, 0, SCOMP_NONE);
    let aud = AudFile::parse(&bytes).unwrap();
    assert_eq!(aud.header.compression, SCOMP_NONE);
}

/// Parser accepts `SCOMP_SONARC` (ID 33) without error.
///
/// Why: although this crate only decodes Westwood ADPCM, the parser must
/// not reject files that use other compression schemes — callers decide.
#[test]
fn parse_accepts_scomp_sonarc() {
    let bytes = super::tests::make_header_bytes(22050, 0, 0, 0, SCOMP_SONARC);
    let aud = AudFile::parse(&bytes).unwrap();
    assert_eq!(aud.header.compression, SCOMP_SONARC);
}

/// Parser accepts `SCOMP_SOS` (ID 99) without error.
///
/// Why: same rationale as `SCOMP_SONARC` — permissive design.
#[test]
fn parse_accepts_scomp_sos() {
    let bytes = super::tests::make_header_bytes(22050, 0, 0, 0, SCOMP_SOS);
    let aud = AudFile::parse(&bytes).unwrap();
    assert_eq!(aud.header.compression, SCOMP_SOS);
}

/// Parser accepts unknown compression IDs (e.g. 255) without error.
///
/// Why: future or modded games may use undefined IDs.  The parser is
/// deliberately permissive — it reports the ID and lets callers decide
/// whether they can handle it.
#[test]
fn parse_accepts_unknown_compression_id() {
    let bytes = super::tests::make_header_bytes(22050, 0, 0, 0, 255);
    let aud = AudFile::parse(&bytes).unwrap();
    assert_eq!(aud.header.compression, 255);
}

// ── Integer overflow safety ──────────────────────────────────────────

/// `compressed_size == u32::MAX` is rejected without panic or wrap.
///
/// Why: `12 + u32::MAX` overflows `usize` on 32-bit targets.  The
/// `saturating_add` in the parser must avoid wrapping and return
/// `UnexpectedEof` instead.
#[test]
fn parse_u32_max_compressed_size_rejected() {
    let bytes = super::tests::make_header_bytes(22050, u32::MAX, 0, 0, SCOMP_WESTWOOD);
    let err = AudFile::parse(&bytes).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// `uncompressed_size == u32::MAX` does not affect parsing.
///
/// Why: the field is informational metadata — the parser never allocates
/// based on it.  Accepting extreme values here is correct.
#[test]
fn parse_u32_max_uncompressed_size_accepted() {
    let bytes = super::tests::make_header_bytes(22050, 0, u32::MAX, 0, SCOMP_WESTWOOD);
    let aud = AudFile::parse(&bytes).unwrap();
    assert_eq!(aud.header.uncompressed_size, u32::MAX);
}

// ── Security: overflow & edge-case tests ─────────────────────────────

/// `UnexpectedEof.needed` uses `saturating_add` so even `u32::MAX`
/// compressed_size produces a coherent error, not a wrapped value.
///
/// Why (V38): on 32-bit platforms `12 + u32::MAX` would wrap to 11.
/// We assert that `needed >= 12` to prove no wrap occurred.
#[test]
fn error_needed_field_does_not_wrap() {
    let bytes = super::tests::make_header_bytes(22050, u32::MAX, 0, 0, SCOMP_WESTWOOD);
    let err = AudFile::parse(&bytes).unwrap_err();
    match err {
        Error::UnexpectedEof { needed, .. } => {
            // On any platform, needed must be >= 12 (header) — it must
            // never wrap to a small number.
            assert!(
                needed >= 12,
                "needed should not wrap below header size: got {needed}"
            );
        }
        other => panic!("Expected UnexpectedEof, got: {other}"),
    }
}

/// ADPCM decode with `max_samples = 1` stops after exactly 1 sample.
///
/// Why: the cap prevents unbounded allocation.  With one byte (two
/// nibbles available), the decoder must emit only one sample and stop.
#[test]
fn adpcm_max_samples_one() {
    let compressed = [0x77u8; 10];
    let samples = decode_adpcm(&compressed, false, 1);
    assert_eq!(samples.len(), 1);
}

/// Stereo ADPCM with `max_samples = 1` stops mid-byte.
///
/// Why: in stereo mode each byte still yields two nibbles, but the
/// sample-count cap must take precedence — only one interleaved sample
/// is emitted.
#[test]
fn adpcm_stereo_max_samples_one() {
    let compressed = [0x77u8; 10];
    let samples = decode_adpcm(&compressed, true, 1);
    assert_eq!(samples.len(), 1);
}

/// `max_samples = 0` means "use default cap", not "produce nothing".
///
/// Why: callers that don't know the expected count pass 0; the decoder
/// must fall back to the compile-time default cap and decode normally.
#[test]
fn adpcm_max_samples_zero_uses_default_cap() {
    let compressed = [0x00u8; 4];
    let samples = decode_adpcm(&compressed, false, 0);
    // 4 bytes × 2 nibbles = 8 samples (well under default cap)
    assert_eq!(samples.len(), 8);
}

/// Single-byte stereo: only the left channel receives both nibbles.
///
/// Why: Westwood stereo assigns bytes by index parity.  Byte 0 routes
/// to the left channel, so a 1-byte input means the right channel gets
/// nothing.  We verify that both nibbles decode on the left side.
#[test]
fn adpcm_stereo_single_byte_left_only() {
    let compressed = [0x07u8]; // byte index 0 = left channel
    let samples = decode_adpcm(&compressed, true, 0);
    assert_eq!(samples.len(), 2);
    // Both nibbles decoded by left channel
    assert_eq!(samples[0], 11); // nibble 7 at step 0
}

/// 100 bytes of `0xFF` (all nibbles = 15) decode without panic.
///
/// Why: nibble 15 is the maximum negative delta at every step; it
/// drives `step_index` to its ceiling and `sample` toward `−32768`.
/// This stress-tests the `i16` clamping path.
#[test]
fn adpcm_all_ff_does_not_panic() {
    let compressed = vec![0xFFu8; 100];
    let samples = decode_adpcm(&compressed, false, 0);
    assert_eq!(samples.len(), 200);
    // All samples should be valid i16 (clamping prevents overflow)
    for s in &samples {
        assert!((-32768..=32767).contains(&(*s as i32)));
    }
}

// ── Adversarial security tests ───────────────────────────────────────

/// `AudFile::parse` on 256 bytes of `0xFF` must not panic.
///
/// Why (V38): an all-ones buffer maximises every header field
/// (`sample_rate = 0xFFFF`, `compressed_size = 0xFFFFFFFF`, etc.).
/// The parser must reject this via `UnexpectedEof` without overflow or
/// out-of-bounds access.
#[test]
fn adversarial_all_ff_parse_no_panic() {
    let data = vec![0xFFu8; 256];
    let _ = AudFile::parse(&data);
}

/// `AudFile::parse` on 256 zero bytes must not panic.
///
/// Why: an all-zero header has `sample_rate = 0`, `compressed_size = 0`,
/// `uncompressed_size = 0`, which exercises the zero-length payload path.
#[test]
fn adversarial_all_zero_parse_no_panic() {
    let data = vec![0u8; 256];
    let _ = AudFile::parse(&data);
}
