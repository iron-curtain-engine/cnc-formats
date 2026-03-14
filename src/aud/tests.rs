// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

// ── Header parsing ────────────────────────────────────────────────────────

pub(crate) fn make_header_bytes(
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
/// drive the right.  The total sample count should equal `bytes Ã— 2`
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
    // 4 bytes Ã— 2 nibbles = 8 samples
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

// ── ADPCM encoder round-trip tests ──────────────────────────────────

/// Encoding silence (all-zero samples) then decoding yields near-zero output.
///
/// Why: silence is the simplest signal; a working encode/decode round-trip
/// must preserve it.  At step_index=0 with input 0 the encoder picks
/// nibble 0 which decodes back to 0.
#[test]
fn encode_adpcm_round_trip() {
    let samples = vec![0i16; 64];
    let compressed = encode_adpcm(&samples, false);
    let decoded = decode_adpcm(&compressed, false, 0);
    assert_eq!(decoded.len(), samples.len());
    for (i, &s) in decoded.iter().enumerate() {
        assert!(s.abs() <= 1, "sample {i}: expected near zero, got {s}");
    }
}

/// Encoding stereo silence then decoding yields near-zero output.
///
/// Why: stereo interleaving uses separate L/R channel states.  Encoding
/// then decoding silence must stay near zero on both channels, confirming
/// the encoder's interleave pattern matches the decoder's.
#[test]
fn encode_adpcm_stereo_round_trip() {
    // 64 interleaved samples: L, R, L, R, …
    let samples = vec![0i16; 64];
    let compressed = encode_adpcm(&samples, true);
    let decoded = decode_adpcm(&compressed, true, 0);
    assert_eq!(decoded.len(), samples.len());
    for (i, &s) in decoded.iter().enumerate() {
        assert!(
            s.abs() <= 1,
            "stereo sample {i}: expected near zero, got {s}"
        );
    }
}

/// Encoding a non-trivial waveform then decoding approximates the original.
///
/// Why: ADPCM is lossy, but the reconstruction error should be bounded
/// once the codec's step size has adapted.  A ramp signal lets the step
/// index grow naturally.  We skip the first 16 samples (ramp-up period
/// where the quantiser is too coarse) and verify the remaining samples
/// are within tolerance.
#[test]
fn encode_adpcm_nonempty_round_trip() {
    // Generate a ramp: 0, 100, 200, 300, … then back down.
    let samples: Vec<i16> = (0..64)
        .map(|i| {
            let phase = i % 32;
            if phase < 16 {
                (phase as i16) * 200
            } else {
                ((32 - phase) as i16) * 200
            }
        })
        .collect();
    let compressed = encode_adpcm(&samples, false);
    let decoded = decode_adpcm(&compressed, false, 0);
    assert_eq!(decoded.len(), samples.len());
    // Skip the first 16 samples where the step index is ramping up.
    for (i, (&orig, &dec)) in samples.iter().zip(decoded.iter()).enumerate().skip(16) {
        let diff = (orig as i32 - dec as i32).abs();
        assert!(
            diff <= 500,
            "sample {i}: original={orig}, decoded={dec}, diff={diff} exceeds tolerance 500"
        );
    }
    // Also verify the output is not all zeros (encoder actually produced data).
    assert!(
        decoded.iter().any(|&s| s != 0),
        "decoded output should contain non-zero samples"
    );
}

/// Encoding an empty sample slice returns an empty compressed output.
///
/// Why: an AUD with zero samples is valid.  The encoder must handle
/// the degenerate case without panicking or producing spurious bytes.
#[test]
fn encode_adpcm_empty() {
    let compressed = encode_adpcm(&[], false);
    assert!(compressed.is_empty());
}
