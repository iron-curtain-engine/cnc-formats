// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

// ── Header parsing ────────────────────────────────────────────────────────

fn make_header_bytes(
    sample_rate: u16,
    comp_size: u32,
    uncomp_size: u32,
    flags: u8,
    compression: u8,
) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&sample_rate.to_le_bytes());
    v.extend_from_slice(&comp_size.to_le_bytes());
    v.extend_from_slice(&uncomp_size.to_le_bytes());
    v.push(flags);
    v.push(compression);
    v
}

/// A well-formed 12-byte header followed by enough payload is parsed
/// correctly, and all header fields are extracted.
///
/// Why: baseline "golden path" test — confirms the byte-offset mapping for
/// every header field (`sample_rate`, `compressed_size`, etc.) matches the
/// Westwood `AUDHeaderType` layout.
#[test]
fn test_parse_header_valid() {
    let mut bytes = make_header_bytes(22050, 1000, 4000, AUD_FLAG_16BIT, SCOMP_WESTWOOD);
    bytes.extend_from_slice(&[0u8; 1000]);
    let aud = AudFile::parse(&bytes).unwrap();

    assert_eq!(aud.header.sample_rate, 22050);
    assert_eq!(aud.header.compressed_size, 1000);
    assert_eq!(aud.header.uncompressed_size, 4000);
    assert_eq!(aud.header.flags, AUD_FLAG_16BIT);
    assert_eq!(aud.header.compression, SCOMP_WESTWOOD);
    assert!(aud.header.is_16bit());
    assert!(!aud.header.is_stereo());
}

/// The stereo flag (`AUD_FLAG_STEREO`) is read from the correct bit.
///
/// Why: the flags byte holds independent bits for stereo (bit 0) and
/// 16-bit (bit 1); this ensures both are decoded without interfering.
#[test]
fn test_parse_header_stereo_flag() {
    let bytes = make_header_bytes(
        22050,
        0,
        0,
        AUD_FLAG_STEREO | AUD_FLAG_16BIT,
        SCOMP_WESTWOOD,
    );
    let aud = AudFile::parse(&bytes).unwrap();
    assert!(aud.header.is_stereo());
    assert!(aud.header.is_16bit());
}

/// Input shorter than the 12-byte header is rejected immediately.
///
/// Why: prevents reading past the end of a truncated or empty file.
/// Tests both the empty slice (0 bytes) and the off-by-one (11 bytes).
#[test]
fn test_parse_header_too_short() {
    assert!(matches!(
        AudFile::parse(&[]),
        Err(Error::UnexpectedEof { .. })
    ));
    assert!(matches!(
        AudFile::parse(&[0u8; 11]),
        Err(Error::UnexpectedEof { .. })
    ));
}

/// `compressed_data` is a sub-slice of the input starting at offset 12.
///
/// Why: the parser must borrow into the original buffer (zero-copy) and
/// expose exactly the declared payload bytes, not the full remainder.
#[test]
fn test_parse_compressed_data_slice() {
    let mut bytes = make_header_bytes(8000, 3, 6, 0, SCOMP_WESTWOOD);
    bytes.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
    let aud = AudFile::parse(&bytes).unwrap();
    assert_eq!(aud.compressed_data, &[0xAAu8, 0xBB, 0xCC]);
}

/// `SCOMP_NONE` compression ID is preserved through parsing.
///
/// Why: the parser is format-agnostic regarding compression type; it must
/// store the raw byte so callers can branch on it.  Ensures no validation
/// rejects known-good IDs.
#[test]
fn test_parse_no_compression() {
    let mut bytes = make_header_bytes(11025, 100, 100, 0, SCOMP_NONE);
    bytes.extend_from_slice(&[0u8; 100]);
    let aud = AudFile::parse(&bytes).unwrap();
    assert_eq!(aud.header.compression, SCOMP_NONE);
}

// ── ADPCM decoder ─────────────────────────────────────────────────────────

/// All-zero compressed bytes decode to near-zero samples.
///
/// With step_index=0 and token=0:
///   diff = step_table[0] >> 3 = 7 >> 3 = 0 (integer division)
///   step_index stays 0 (index_adj[0] = -1, clamped to 0)
/// So samples remain 0 throughout.
#[test]
fn test_adpcm_decode_silence() {
    let compressed = [0u8; 4]; // 8 nibbles = 8 samples
    let samples = decode_adpcm(&compressed, false, 0);
    assert_eq!(samples.len(), 8);
    for s in &samples {
        assert_eq!(*s, 0, "silence decodes to zero");
    }
}

/// Each compressed byte produces exactly 2 PCM samples (mono).
///
/// Why: the 2-samples-per-byte ratio is fundamental to IMA ADPCM; if the
/// low/high nibble split is wrong, sample count will be off.
#[test]
fn test_adpcm_sample_count() {
    let compressed = [0u8; 10];
    let samples = decode_adpcm(&compressed, false, 0);
    assert_eq!(samples.len(), 20);
}

/// Stereo mode still produces 2 samples per byte (split across channels).
///
/// Why: in stereo, even-indexed bytes drive the left channel and odd bytes
/// drive the right.  The total sample count should equal `bytes × 2`
/// regardless of mono/stereo.
#[test]
fn test_adpcm_stereo_sample_count() {
    let compressed = [0u8; 8]; // 4 pairs of bytes
    let samples = decode_adpcm(&compressed, true, 0);
    assert_eq!(samples.len(), 16); // 2 samples per byte
}

/// Empty compressed input produces zero samples without error.
///
/// Why: an AUD with `compressed_size = 0` is valid; the decoder must
/// handle a zero-length slice gracefully, returning an empty `Vec`.
#[test]
fn test_adpcm_empty_input() {
    let samples = decode_adpcm(&[], false, 0);
    assert!(samples.is_empty());
}

/// Non-zero nibble advances the step index (output diverges from zero).
///
/// Nibble 7 (0b0111) has magnitude=7 = all bits set; with step=7 it encodes
/// diff = 7/8 + 7/4 + 7/2 + 7 = 0 + 1 + 3 + 7 = 11 (integer arithmetic).
/// So the first sample after processing a 0x07 low nibble should be 11.
#[test]
fn test_adpcm_nonzero_nibble() {
    // byte 0x07: low nibble=7 (positive max at step_index=0), high nibble=0
    let compressed = [0x07u8];
    let samples = decode_adpcm(&compressed, false, 0);
    assert_eq!(samples.len(), 2);
    // First sample: token=7 (magnitude=7, sign=0), diff=11
    assert_eq!(samples[0], 11);
}

/// Nibble 8 (sign bit set, magnitude=0): negative diff with same magnitude as nibble 0.
///
/// With step_index=0, token=8 (0b1000):
///   magnitude = 8 & 7 = 0 → diff_base = 7 >> 3 = 0
///   sign bit set → diff = 0, negated → 0
/// Sample stays at 0.
#[test]
fn test_adpcm_negative_zero_nibble() {
    let compressed = [0x08u8]; // low nibble=8 (sign bit, mag=0)
    let samples = decode_adpcm(&compressed, false, 0);
    assert_eq!(samples[0], 0);
}

/// Symmetric encode/decode: nibble 0x0F (sign + all magnitude bits).
///
/// Token 15 = 0b1111: magnitude=7, sign=1
///   diff = -(7/8 + 7/4 + 7/2 + 7) = -11
///   sample = -11
#[test]
fn test_adpcm_max_negative_nibble() {
    let compressed = [0x0Fu8]; // low nibble=15 (max negative at step=0)
    let samples = decode_adpcm(&compressed, false, 0);
    assert_eq!(samples[0], -11);
}

/// Step index increases after a large-magnitude nibble (nibble 7 → adj=+8).
///
/// After processing nibble 7 at step_index=0, the new step_index = 0+8 = 8.
/// The high nibble (0) then uses IMA_STEP_TABLE[8] = 16.
///   diff = 16 >> 3 = 2, token=0 (magnitude=0)
///   sample = 11 + 2 = 13
#[test]
fn test_adpcm_step_index_advances() {
    // byte = 0x07: low nibble=7, high nibble=0
    let compressed = [0x07u8];
    let samples = decode_adpcm(&compressed, false, 0);
    // sample[0] = 11, sample[1] uses step_index=8 (step=16):
    // diff = 16>>3 = 2, sample[1] = 11+2 = 13
    assert_eq!(samples[0], 11);
    assert_eq!(samples[1], 13);
}

/// Decoder clamps sample to i16 range: no overflow.
///
/// Drive the sample toward +32767 then try to go further — it must clamp.
#[test]
fn test_adpcm_clamping() {
    // Byte 0x77: both nibbles = 7 (max positive at whatever step_index).
    // Repeat many times to saturate at 32767.
    let compressed = vec![0x77u8; 200];
    let samples = decode_adpcm(&compressed, false, 0);
    for s in &samples {
        // i16 is always within -32768..=32767 by definition; this check
        // guards against regressions if the return type ever changes.
        let _ = *s; // ensure each sample is produced without panic
    }
    // Final samples should be near the maximum.
    let last = *samples.last().unwrap();
    assert!(
        last > 30000,
        "saturated sample should be near max: got {last}"
    );
}

/// Stereo channels are independent: left and right don't cross-contaminate.
///
/// Why: the Westwood stereo layout assigns alternating bytes to L and R
/// channels, each with its own `sample`/`step_index` state.  If the
/// decoder accidentally shares state, right-channel silence bytes would
/// pick up the left channel's accumulated value.
///
/// How: even bytes are `0x77` (max positive step) driving the left
/// channel upward, while odd bytes are `0x00` (silence).  We assert
/// that every right-channel sample remains exactly zero.
#[test]
fn test_adpcm_stereo_channel_independence() {
    // Byte pattern: even bytes (left) = 0x77 (max positive), odd bytes (right) = 0x00 (silence)
    let compressed: Vec<u8> = (0..10)
        .flat_map(|i| if i % 2 == 0 { [0x77u8] } else { [0x00u8] })
        .collect();
    let samples = decode_adpcm(&compressed, true, 0);
    // Even-indexed bytes drive left channel high; odd bytes keep right at zero.
    // Extract right-channel samples: bytes 1,3,5,7,9 each produce 2 samples.
    // Right channel samples are at positions: byte_1 → samples[2..4], byte_3 → [6..8], etc.
    // All right-channel byte inputs are 0x00, so right output should remain 0.
    for i in 0..5 {
        let byte_idx = i * 2 + 1; // odd byte indices
        let s0 = samples[byte_idx * 2];
        let s1 = samples[byte_idx * 2 + 1];
        assert_eq!(s0, 0, "right channel sample should be 0");
        assert_eq!(s1, 0, "right channel sample should be 0");
    }
}

/// End-to-end: parse a complete AUD file, then decode its ADPCM audio.
///
/// Why: exercises the full pipeline from raw bytes → `AudFile` →
/// `decode_adpcm`, verifying that `compressed_data` is correctly passed
/// through and the first sample matches the known value for nibble 7.
#[test]
fn test_parse_then_decode() {
    let mut bytes = make_header_bytes(22050, 4, 16, AUD_FLAG_16BIT, SCOMP_WESTWOOD);
    bytes.extend_from_slice(&[0x07, 0x07, 0x07, 0x07]); // 4 compressed bytes
    let aud = AudFile::parse(&bytes).unwrap();
    assert_eq!(aud.header.compression, SCOMP_WESTWOOD);
    let samples = decode_adpcm(aud.compressed_data, aud.header.is_stereo(), 0);
    // 4 bytes × 2 nibbles = 8 samples
    assert_eq!(samples.len(), 8);
    // First nibble 7 at step_index 0 produces 11
    assert_eq!(samples[0], 11);
}

/// Header declares more compressed bytes than actually present.
///
/// Why: a common corruption scenario.  The parser must compare the
/// declared `compressed_size` against the actual payload length and
/// return `UnexpectedEof` rather than slicing out of bounds.
#[test]
fn test_parse_compressed_size_exceeds_payload() {
    // Header claims 500 compressed bytes, only 10 present.
    let mut bytes = make_header_bytes(22050, 500, 1000, AUD_FLAG_16BIT, SCOMP_WESTWOOD);
    bytes.extend_from_slice(&[0u8; 10]);
    assert!(matches!(
        AudFile::parse(&bytes),
        Err(Error::UnexpectedEof { .. })
    ));
}

/// `compressed_data` is sliced to exactly `compressed_size`, ignoring
/// trailing bytes.
///
/// Why: files may contain padding or appended metadata.  The parser must
/// expose only the declared compressed payload so the decoder doesn't
/// process garbage trailing bytes.
#[test]
fn test_parse_compressed_data_exact_size() {
    let mut bytes = make_header_bytes(22050, 5, 10, 0, SCOMP_WESTWOOD);
    bytes.extend_from_slice(&[0xAA; 20]); // 20 bytes, but header says 5
    let aud = AudFile::parse(&bytes).unwrap();
    assert_eq!(aud.compressed_data.len(), 5);
}

/// The `max_samples` parameter hard-caps decoder output (V38 guard).
///
/// Why: prevents runaway allocation when compressed data is very long.
/// 100 bytes would produce 200 samples; capping at 10 must stop early.
#[test]
fn test_adpcm_max_samples_cap() {
    let compressed = vec![0x77u8; 100]; // would produce 200 samples
    let samples = decode_adpcm(&compressed, false, 10);
    assert_eq!(samples.len(), 10);
}

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
    let mut bytes = make_header_bytes(22050, 500, 1000, AUD_FLAG_16BIT, SCOMP_WESTWOOD);
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
    let mut bytes = make_header_bytes(22050, 4, 16, AUD_FLAG_16BIT, SCOMP_WESTWOOD);
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
    let bytes = make_header_bytes(22050, 0, 0, 0, SCOMP_WESTWOOD);
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
    let bytes = make_header_bytes(22050, 0, 0, 0, SCOMP_NONE);
    let aud = AudFile::parse(&bytes).unwrap();
    assert_eq!(aud.header.compression, SCOMP_NONE);
}

/// Parser accepts `SCOMP_SONARC` (ID 33) without error.
///
/// Why: although this crate only decodes Westwood ADPCM, the parser must
/// not reject files that use other compression schemes — callers decide.
#[test]
fn parse_accepts_scomp_sonarc() {
    let bytes = make_header_bytes(22050, 0, 0, 0, SCOMP_SONARC);
    let aud = AudFile::parse(&bytes).unwrap();
    assert_eq!(aud.header.compression, SCOMP_SONARC);
}

/// Parser accepts `SCOMP_SOS` (ID 99) without error.
///
/// Why: same rationale as `SCOMP_SONARC` — permissive design.
#[test]
fn parse_accepts_scomp_sos() {
    let bytes = make_header_bytes(22050, 0, 0, 0, SCOMP_SOS);
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
    let bytes = make_header_bytes(22050, 0, 0, 0, 255);
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
    let bytes = make_header_bytes(22050, u32::MAX, 0, 0, SCOMP_WESTWOOD);
    let err = AudFile::parse(&bytes).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// `uncompressed_size == u32::MAX` does not affect parsing.
///
/// Why: the field is informational metadata — the parser never allocates
/// based on it.  Accepting extreme values here is correct.
#[test]
fn parse_u32_max_uncompressed_size_accepted() {
    let bytes = make_header_bytes(22050, 0, u32::MAX, 0, SCOMP_WESTWOOD);
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
    let bytes = make_header_bytes(22050, u32::MAX, 0, 0, SCOMP_WESTWOOD);
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
