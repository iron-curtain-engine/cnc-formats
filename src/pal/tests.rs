// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

fn all_zero_pal() -> Vec<u8> {
    vec![0u8; PALETTE_BYTES]
}

/// 768 zero bytes → palette with all-black entries.
///
/// Why: validates the minimum well-formed palette.  Every colour
/// should be `(0, 0, 0)` — no garbage leaking from uninitialised memory.
#[test]
fn test_parse_all_zero() {
    let pal = Palette::parse(&all_zero_pal()).unwrap();
    assert_eq!(pal.colors.len(), PALETTE_SIZE);
    for c in &pal.colors {
        assert_eq!(*c, PalColor { r: 0, g: 0, b: 0 });
    }
}

/// Data shorter than 768 bytes → `UnexpectedEof`.
///
/// Why: a PAL file is exactly 768 bytes (256 × 3).  One byte short must
/// be rejected to prevent reading uninitialised entries.
#[test]
fn test_parse_too_short() {
    let result = Palette::parse(&[0u8; 767]);
    assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
}

/// Empty slice → `UnexpectedEof`.
///
/// Why: degenerate zero-length input must be handled cleanly.
#[test]
fn test_parse_empty() {
    let result = Palette::parse(&[]);
    assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
}

/// Known VGA 6-bit values are parsed into the correct colour slots.
///
/// Why: verifies that the byte→colour mapping respects the 3-byte
/// stride and that colours at index 0, 128, and 255 land in the right
/// slots.  These cover the first, middle, and last entries.
#[test]
fn test_parse_known_values() {
    let mut data = all_zero_pal();
    // Color 0: (1, 2, 3)
    data[0] = 1;
    data[1] = 2;
    data[2] = 3;
    // Color 255: (63, 63, 63) — white in VGA 6-bit
    data[255 * 3] = 63;
    data[255 * 3 + 1] = 63;
    data[255 * 3 + 2] = 63;
    // Color 128: (10, 20, 30)
    data[128 * 3] = 10;
    data[128 * 3 + 1] = 20;
    data[128 * 3 + 2] = 30;

    let pal = Palette::parse(&data).unwrap();

    assert_eq!(pal.colors[0], PalColor { r: 1, g: 2, b: 3 });
    assert_eq!(
        pal.colors[255],
        PalColor {
            r: 63,
            g: 63,
            b: 63
        }
    );
    assert_eq!(
        pal.colors[128],
        PalColor {
            r: 10,
            g: 20,
            b: 30
        }
    );
}

/// `to_rgb8` converts 6-bit VGA values to 8-bit via `(v & 0x3F) << 2`.
///
/// Why: C&C PAL files use VGA 6-bit colour (0–63 per channel).  The
/// standard conversion shifts left by 2 to map into 0–252.  We test
/// zero, max (63 → 252), and intermediate values.
#[test]
fn test_to_rgb8_conversion() {
    assert_eq!(PalColor { r: 0, g: 0, b: 0 }.to_rgb8(), [0, 0, 0]);
    // 63 << 2 = 252 (not 255 — VGA 6-bit tops out at 252/255)
    assert_eq!(
        PalColor {
            r: 63,
            g: 63,
            b: 63
        }
        .to_rgb8(),
        [252, 252, 252]
    );
    assert_eq!(PalColor { r: 32, g: 16, b: 8 }.to_rgb8(), [128, 64, 32]);
    // 1 << 2 = 4
    assert_eq!(PalColor { r: 1, g: 2, b: 3 }.to_rgb8(), [4, 8, 12]);
}

/// `to_rgb8_array` converts all 256 entries at once.
///
/// Why: callers typically need the whole palette in 8-bit form for
/// rendering.  We set colour 10 to white-63 and verify it maps to 252.
#[test]
fn test_to_rgb8_array_white_entry() {
    let mut data = all_zero_pal();
    data[10 * 3] = 63;
    data[10 * 3 + 1] = 63;
    data[10 * 3 + 2] = 63;

    let pal = Palette::parse(&data).unwrap();
    let rgb8 = pal.to_rgb8_array();

    assert_eq!(rgb8[10], [252, 252, 252]);
    assert_eq!(rgb8[0], [0, 0, 0]);
}

/// Extra bytes beyond 768 are silently ignored.
///
/// Why: the PAL format has no explicit length field; the parser reads
/// exactly 768 bytes and ignores the rest.  Files padded by container
/// formats (e.g. MIX) must still parse correctly.
#[test]
fn test_parse_extra_bytes_ignored() {
    let mut data = all_zero_pal();
    data[0] = 7;
    // Append some extra bytes — parse should still succeed
    let mut extended = data.clone();
    extended.extend_from_slice(&[0xFFu8; 10]);

    let pal = Palette::parse(&extended).unwrap();
    assert_eq!(pal.colors[0].r, 7);
}

/// Parsing the same PAL data twice yields identical results.
///
/// Why: the parser is a pure function of its input; any hidden state
/// that leaked between calls would break reproducibility.
#[test]
fn test_parse_deterministic() {
    let mut data = all_zero_pal();
    for (i, byte) in data.iter_mut().enumerate().take(PALETTE_BYTES) {
        *byte = (i % 64) as u8;
    }
    let p1 = Palette::parse(&data).unwrap();
    let p2 = Palette::parse(&data).unwrap();
    assert_eq!(p1, p2);
}

// ── Error field & Display verification ────────────────────────────────

/// `UnexpectedEof` for a too-short input carries the exact byte counts.
///
/// Why: structured error fields let callers generate precise diagnostics.
/// A 100-byte input needs 768 and has only 100 available.
#[test]
fn eof_error_carries_byte_counts() {
    let err = Palette::parse(&[0u8; 100]).unwrap_err();
    match err {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, 768, "PAL file is exactly 768 bytes");
            assert_eq!(available, 100);
        }
        other => panic!("Expected UnexpectedEof, got: {other}"),
    }
}

/// Empty input also produces the correct `needed` count.
///
/// Why: same as `eof_error_carries_byte_counts` but with 0 available;
/// exercises the `available = 0` branch.
#[test]
fn eof_error_empty_input_carries_byte_counts() {
    let err = Palette::parse(&[]).unwrap_err();
    match err {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, 768);
            assert_eq!(available, 0);
        }
        other => panic!("Expected UnexpectedEof, got: {other}"),
    }
}

/// `Error::Display` embeds the numeric context for human-readable output.
///
/// Why: the Display message is the user-facing diagnostic; it must
/// include `needed` (768) and `available` (100).
#[test]
fn eof_display_contains_byte_counts() {
    let err = Palette::parse(&[0u8; 100]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("768"), "should mention needed bytes: {msg}");
    assert!(msg.contains("100"), "should mention available bytes: {msg}");
}

// ── Boundary tests ──────────────────────────────────────────────────

/// Exactly 768 bytes is the minimum valid PAL file.
///
/// Why: boundary test — one byte more than the failing case (767).
#[test]
fn parse_exactly_768_bytes_succeeds() {
    let data = vec![0u8; 768];
    let pal = Palette::parse(&data).unwrap();
    assert_eq!(pal.colors.len(), 256);
}

/// 767 bytes is one short of valid.
///
/// Why: boundary test — one byte fewer than the minimum.
/// Confirms the parser uses `>=` not `>` for the length check.
#[test]
fn parse_767_bytes_fails() {
    let data = vec![0u8; 767];
    let err = Palette::parse(&data).unwrap_err();
    match err {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, 768);
            assert_eq!(available, 767);
        }
        other => panic!("Expected UnexpectedEof, got: {other}"),
    }
}

// ── Integer overflow safety ──────────────────────────────────────────

/// `to_rgb8` masks to 6 bits, so out-of-range components don't panic.
///
/// Why: raw PAL bytes > 63 are technically invalid per the VGA spec,
/// but the VGA DAC hardware ignores the top two bits.  Our `& 0x3F`
/// mask reproduces this hardware truncation.
///
/// How: `255 & 0x3F = 63 → 252`; `128 & 0x3F = 0 → 0`; `64 & 0x3F = 0 → 0`.
#[test]
fn to_rgb8_out_of_range_does_not_panic() {
    // Components > 63 are technically invalid per the VGA 6-bit spec,
    // but to_rgb8 masks to 6 bits (matching VGA DAC hardware behaviour)
    // so this must not panic.
    let color = PalColor {
        r: 255,
        g: 128,
        b: 64,
    };
    let rgb = color.to_rgb8();
    // 255 & 0x3F = 63, 63 << 2 = 252
    assert_eq!(rgb[0], 252);
    // 128 & 0x3F = 0, 0 << 2 = 0
    assert_eq!(rgb[1], 0);
    // 64 & 0x3F = 0, 0 << 2 = 0
    assert_eq!(rgb[2], 0);
}

/// `to_rgb8` is correct for both 6-bit boundary values (0 and 63).
///
/// Why: 0 maps to 0 and 63 maps to 252; these are the valid extremes
/// of the VGA colour space.
#[test]
fn to_rgb8_boundary_values() {
    assert_eq!(PalColor { r: 0, g: 0, b: 0 }.to_rgb8(), [0, 0, 0]);
    assert_eq!(
        PalColor {
            r: 63,
            g: 63,
            b: 63
        }
        .to_rgb8(),
        [252, 252, 252]
    );
}

// ── Adversarial security tests ───────────────────────────────────────

/// `Palette::parse` on 768 bytes of `0xFF` must not panic.
///
/// Why (V38): 768 is the exact minimum valid size.  All-`0xFF` bytes
/// set every colour component to 255, which is technically out of range
/// for 6-bit VGA (valid range 0–63) but the parser must accept it
/// permissively.  The `to_rgb8` mask (`& 0x3F`) handles the truncation.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 768];
    let pal = Palette::parse(&data).unwrap();
    // All components masked to 63 → rgb8 = 252
    assert_eq!(pal.colors[0].to_rgb8(), [252, 252, 252]);
    assert_eq!(pal.colors[255].to_rgb8(), [252, 252, 252]);
}

/// `Palette::parse` on 769 bytes accepts the first 768 and ignores trailing.
///
/// Why: real PAL files may have trailing metadata or padding.  The parser
/// must consume exactly 768 bytes and ignore the rest.
#[test]
fn adversarial_trailing_data_ignored() {
    let mut data = vec![0u8; 769];
    data[768] = 0xDE; // trailing byte
    let pal = Palette::parse(&data).unwrap();
    assert_eq!(pal.colors.len(), 256);
}

/// `Palette::parse` on a sub-valid-size all-zero buffer must not panic.
///
/// Why (V38): a zero-length or near-zero buffer exercises the early EOF
/// path.  The parser must return an error, not panic.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = [0u8; 100];
    let _ = Palette::parse(&data);
}

// ── Encoder round-trip tests ─────────────────────────────────────────

/// `encode` produces bytes that `parse` round-trips to an identical palette.
///
/// Why: the encoder must emit the same 768-byte format the parser reads.
/// A mismatch means either the encoder omits/reorders bytes or the parser
/// misinterprets the encoded output.
#[test]
fn encode_round_trip() {
    let mut data = all_zero_pal();
    // Set a few known colours to exercise non-zero values.
    data[0] = 1;
    data[1] = 2;
    data[2] = 3;
    data[128 * 3] = 10;
    data[128 * 3 + 1] = 20;
    data[128 * 3 + 2] = 30;
    data[255 * 3] = 63;
    data[255 * 3 + 1] = 63;
    data[255 * 3 + 2] = 63;

    let original = Palette::parse(&data).unwrap();
    let encoded = original.encode();
    assert_eq!(encoded.len(), PALETTE_BYTES);

    let reparsed = Palette::parse(&encoded).unwrap();
    assert_eq!(original, reparsed);
}

/// `from_rgb8` → `encode` round-trip: 8-bit RGB in, 6-bit VGA bytes out.
///
/// Why: `from_rgb8` divides by 4 (`>> 2`) and `encode` writes the 6-bit
/// values verbatim.  We verify the encoded bytes match the expected
/// 6-bit representation of the input.
#[test]
fn from_rgb8_round_trip() {
    // Build 768 bytes of 8-bit RGB data with known values.
    let mut rgb8 = vec![0u8; PALETTE_BYTES];
    // Color 0: (252, 128, 64) → 6-bit: (63, 32, 16)
    rgb8[0] = 252;
    rgb8[1] = 128;
    rgb8[2] = 64;
    // Color 255: (0, 4, 8) → 6-bit: (0, 1, 2)
    rgb8[255 * 3] = 0;
    rgb8[255 * 3 + 1] = 4;
    rgb8[255 * 3 + 2] = 8;

    let pal = Palette::from_rgb8(&rgb8).unwrap();
    let encoded = pal.encode();
    assert_eq!(encoded.len(), PALETTE_BYTES);

    // Verify expected 6-bit values at color 0.
    assert_eq!(encoded[0], 63); // 252 >> 2
    assert_eq!(encoded[1], 32); // 128 >> 2
    assert_eq!(encoded[2], 16); // 64 >> 2

    // Verify expected 6-bit values at color 255.
    assert_eq!(encoded[255 * 3], 0); // 0 >> 2
    assert_eq!(encoded[255 * 3 + 1], 1); // 4 >> 2
    assert_eq!(encoded[255 * 3 + 2], 2); // 8 >> 2
}

/// `from_rgb8` with input shorter than 768 bytes returns `UnexpectedEof`.
///
/// Why: the function requires exactly 256 RGB triples (768 bytes).  A
/// short buffer must be rejected to prevent reading uninitialised entries.
#[test]
fn from_rgb8_too_short() {
    let short = vec![0u8; 767];
    let result = Palette::from_rgb8(&short);
    assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
}
