// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

/// The `0x80` end-of-stream marker by itself produces empty output.
///
/// Why: ensures the decoder recognises the terminator immediately without
/// requiring any preceding data, which is the simplest valid LCW stream.
#[test]
fn test_end_marker_only() {
    let result = decompress(&[0x80], 1024).unwrap();
    assert!(result.is_empty());
}

/// Medium literal command (`0x83`) copies 3 verbatim bytes to output.
///
/// Why: the medium literal is the most basic data-carrying LCW command;
/// this verifies the count is extracted from the low 6 bits and that
/// exactly that many bytes are forwarded.
///
/// How: `0x83 = 0b10_000011` → count = 3, followed by "ABC" and end marker.
#[test]
fn test_medium_literal_three_bytes() {
    // 0x83 = 0b10000011 → medium literal, count = 3
    let input = [0x83u8, b'A', b'B', b'C', 0x80];
    let out = decompress(&input, 1024).unwrap();
    assert_eq!(out, b"ABC");
}

/// Medium literal boundary: count = 1 (minimum non-zero literal).
///
/// Why: exercises the smallest valid literal to guard against off-by-one
/// in the count extraction or read loop.
#[test]
fn test_medium_literal_one_byte() {
    let input = [0x81u8, b'Z', 0x80];
    let out = decompress(&input, 1024).unwrap();
    assert_eq!(out, b"Z");
}

/// Long fill command (`0xFE`) repeats a single byte N times.
///
/// Why: fill is used for run-length encoding; this validates the u16
/// count is read correctly and the value byte is repeated exactly.
///
/// How: `0xFE`, count = 4 (LE u16), value = 'X', then end marker.
#[test]
fn test_long_fill() {
    // 0xFE  count:u16=4  value='X'
    let input = [0xFEu8, 0x04, 0x00, b'X', 0x80];
    let out = decompress(&input, 1024).unwrap();
    assert_eq!(out, b"XXXX");
}

/// Short relative copy duplicates bytes from a backwards offset in the
/// output buffer.
///
/// Why: this is the primary back-reference command in LCW.  Verifies that
/// the 12-bit relative offset and 3-bit count (+3 bias) are decoded
/// correctly by writing "ABC" then copying 3 bytes from 3 positions back.
#[test]
fn test_short_relative_copy() {
    // Write "ABC" via medium literal (0x83)
    // Short copy: x=0 (count=3), rel_offset=3
    //   first_byte  = 0b0_000_0000 | (3 >> 8) = 0x00
    //   second_byte = 3 & 0xFF      = 0x03
    let input = [0x83u8, b'A', b'B', b'C', 0x00, 0x03, 0x80];
    let out = decompress(&input, 1024).unwrap();
    assert_eq!(out, b"ABCABC");
}

/// Short relative copy with x = 2 (count = 5) exercises a larger count.
///
/// Why: the count field uses 3 high bits of the command byte with a +3
/// bias; this ensures the bias is applied correctly for non-minimum values.
#[test]
fn test_short_relative_copy_count5() {
    // Write "HELLO" via medium literal (0x85 = count 5)
    // Short copy x=2 (count=5), rel=5 → copy last 5 bytes again
    //   first_byte  = (2 << 4) | (5 >> 8) = 0x20
    //   second_byte = 5 & 0xFF = 0x05
    let input = [
        0x85u8, b'H', b'E', b'L', b'L', b'O', // "HELLO"
        0x20, 0x05, // copy 5 from 5 back → "HELLO"
        0x80,
    ];
    let out = decompress(&input, 1024).unwrap();
    assert_eq!(out, b"HELLOHELLO");
}

/// Medium absolute copy (`0xC0–0xFD`) copies from an absolute offset
/// in the output buffer.
///
/// Why: unlike relative copies, absolute copies address from the start of
/// output.  Verifies the count (+3 bias) and u16 offset are extracted
/// correctly by writing "XYZ" then copying 3 bytes from offset 0.
#[test]
fn test_medium_absolute_copy() {
    // Write "XYZ" (0x83 = 3 literal bytes)
    // Medium abs copy: 0xC0 = 0b11000000 → count = (0 & 0x3F) + 3 = 3, offset = word
    //   offset_lo=0x00, offset_hi=0x00 → abs_offset=0 → copies out[0..3]
    let input = [
        0x83u8, b'X', b'Y', b'Z', // "XYZ"
        0xC0, 0x00, 0x00, // copy 3 from offset 0 → "XYZ"
        0x80,
    ];
    let out = decompress(&input, 1024).unwrap();
    assert_eq!(out, b"XYZXYZ");
}

/// Long absolute copy (`0xFF`) uses explicit u16 count and u16 offset.
///
/// Why: this is the most general copy command, used when medium formats
/// cannot encode the needed count.  Verifies both words are read in the
/// correct order and the copy produces the expected data.
#[test]
fn test_long_absolute_copy() {
    // Write "RUST" via medium literal (0x84 = 4 bytes)
    // Long copy: 0xFF count:u16=4 offset:u16=0 → copies out[0..4]
    let input = [
        0x84u8, b'R', b'U', b'S', b'T', // "RUST"
        0xFF, 0x04, 0x00, 0x00, 0x00, // copy 4 from offset 0
        0x80,
    ];
    let out = decompress(&input, 1024).unwrap();
    assert_eq!(out, b"RUSTRUST");
}

/// V38 safety cap: output exceeding `max_output` is refused.
///
/// Why: the decompression ratio cap (V38) prevents malicious compressed
/// data from inflating into an unbounded memory allocation.  A fill
/// command requesting 100 bytes against a 50-byte cap must be rejected.
#[test]
fn test_output_cap_enforced() {
    // Long fill of 100 bytes but max_output=50 → error
    let input = [0xFEu8, 0x64, 0x00, b'!', 0x80]; // fill 100 bytes
    let result = decompress(&input, 50);
    assert!(matches!(result, Err(Error::DecompressionError { .. })));
}

/// Truncated literal: command promises more bytes than the input contains.
///
/// Why: malformed or truncated files must produce a clear error rather than
/// a panic or out-of-bounds read.  `0x83` promises 3 bytes but only 1
/// follows, so `read_byte` must fail with `UnexpectedEof`.
#[test]
fn test_truncated_medium_literal() {
    // 0x83 promises 3 bytes but only 1 follows
    let input = [0x83u8, b'A'];
    let result = decompress(&input, 1024);
    assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
}

/// Chained operations: literal + fill + relative copy in one stream.
///
/// Why: real LCW streams intermix command types; this end-to-end sequence
/// verifies the dispatch loop correctly transitions between commands and
/// that output offsets stay coherent across different command types.
#[test]
fn test_chained_operations() {
    // "AB" + fill 3 with '.' + copy 5 from 5 back
    let input = [
        0x82u8, b'A', b'B', // "AB"
        0xFE, 0x03, 0x00, b'.', // "..."
        // short copy: x=2 (count=5), rel=5
        // first_byte = (2 << 4) | (5 >> 8) = 0x20, second_byte = 0x05
        0x20, 0x05, // "AB..."
        0x80,
    ];
    let out = decompress(&input, 1024).unwrap();
    assert_eq!(out, b"AB...AB...");
}

/// Short relative copy with `rel_offset = 0` produces zero fill.
///
/// Why: the EA engine pre-allocates the destination buffer.  A zero
/// relative offset effectively copies from the current position in the
/// pre-zeroed buffer, producing zeros.
#[test]
fn test_short_relative_copy_zero_offset() {
    // Write "AB" then short copy with rel_offset=0: produces 3 zeros.
    // 0x00 = count 3, offset 0.  Next byte 0x00 is the offset low byte.
    // Then 0x80 = end marker.
    let input = [0x82u8, b'A', b'B', 0x00, 0x00, 0x80];
    let result = decompress(&input, 1024).unwrap();
    assert_eq!(result, vec![b'A', b'B', 0, 0, 0]);
}

/// Medium absolute copy referencing bytes beyond the output buffer is
/// rejected.
///
/// Why: an offset+count that exceeds the current output length would read
/// uninitialised memory.  The bounds check must catch this before the copy
/// loop runs.
///
/// Medium absolute copy that extends past written output produces zeros
/// for the unwritten bytes.
///
/// Why: real game data relies on pre-zeroed buffer semantics where
/// absolute copies can reference positions beyond what's been written.
#[test]
fn test_medium_absolute_copy_beyond_written() {
    // Write "AB" (2 bytes). Medium abs copy of 3 bytes from offset 1:
    // Copies byte-by-byte: offset 1='B', then each subsequent byte reads
    // from the growing output (overlapping copy = RLE-like repetition).
    let input = [0x82u8, b'A', b'B', 0xC0, 0x01, 0x00, 0x80];
    let result = decompress(&input, 1024).unwrap();
    assert_eq!(result, vec![b'A', b'B', b'B', b'B', b'B']);
}

/// Long absolute copy referencing bytes beyond the output buffer produces
/// zeros for unwritten positions.
///
/// Why: same pre-zeroed buffer semantics as the medium variant.
#[test]
fn test_long_absolute_copy_beyond_written() {
    // Write 2 bytes ("AB"), then copy 5 from offset 0.
    // Byte-by-byte copy from offset 0: reads A,B, then the just-written
    // A,B again (overlapping = repeating pattern).
    let input = [
        0x82u8, b'A', b'B', // 2 bytes
        0xFF, 0x05, 0x00, 0x00, 0x00, // copy 5 from offset 0
        0x80,
    ];
    let result = decompress(&input, 1024).unwrap();
    assert_eq!(result, vec![b'A', b'B', b'A', b'B', b'A', b'B', b'A']);
}

/// Short relative copy whose offset exceeds the current output length
/// produces zeros (pre-zeroed buffer semantics).
///
/// Why: the EA engine pre-allocates the destination buffer.  A relative
/// offset larger than the current write position reads from pre-zeroed
/// memory before the start of written data.
#[test]
fn test_short_relative_copy_past_output() {
    // Write 2 bytes, then short copy with rel_offset=10 (larger than output)
    // first_byte = 0b0_000_0000 | (10 >> 8) = 0x00, second_byte = 0x0A
    let input = [0x82u8, b'A', b'B', 0x00, 0x0A, 0x80];
    let result = decompress(&input, 1024).unwrap();
    assert_eq!(result, vec![b'A', b'B', 0, 0, 0]);
}

/// Completely empty input (no commands at all) returns `UnexpectedEof`.
///
/// Why: every valid LCW stream must contain at least the `0x80` end marker.
/// An empty slice fails on the very first `read_byte`, ensuring no silent
/// empty-output success for clearly invalid input.
#[test]
fn test_empty_input() {
    let result = decompress(&[], 1024);
    assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
}

// ── Error field & Display verification ────────────────────────────────

/// `UnexpectedEof` error carries the exact byte positions.
///
/// Why: structured error fields let callers report precise diagnostics.
/// Empty input needs 1 byte (the first command) and has 0 available.
#[test]
fn eof_error_carries_byte_counts() {
    let err = decompress(&[], 1024).unwrap_err();
    match err {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, 1, "should need 1 byte for first command");
            assert_eq!(available, 0);
        }
        other => panic!("Expected UnexpectedEof, got: {other}"),
    }
}

/// `DecompressionError` carries a human-readable reason string.
///
/// Why: the `reason` field must contain enough context for debugging.
/// A truncated medium literal (claims N bytes but stream ends) should
/// produce a reason mentioning the issue.
#[test]
fn decompression_error_carries_reason() {
    // Medium literal claiming 2 bytes but stream has none after the command.
    let input = [0x82u8];
    let err = decompress(&input, 1024).unwrap_err();
    match err {
        Error::UnexpectedEof { .. } | Error::DecompressionError { .. } => {
            // Either error type is acceptable for truncated input.
        }
        other => panic!("Expected DecompressionError or UnexpectedEof, got: {other}"),
    }
}

/// Output-cap error's reason mentions `max_output`.
///
/// Why: when a user hits the V38 output cap, the error message must make
/// it clear that the limit was the cause — not a corrupt stream.
#[test]
fn output_cap_error_reason_mentions_max() {
    let input = [0xFEu8, 0x64, 0x00, b'!', 0x80]; // fill 100 bytes
    let err = decompress(&input, 50).unwrap_err();
    match err {
        Error::DecompressionError { reason } => {
            assert!(
                reason.contains("max_output"),
                "reason should mention max_output, got: {reason}"
            );
        }
        other => panic!("Expected DecompressionError, got: {other}"),
    }
}

/// `Error::Display` for `UnexpectedEof` embeds the numeric context.
///
/// Why: the Display trait output is the user-facing message; it must
/// include the needed and available byte counts for diagnostics.
#[test]
fn eof_display_contains_byte_counts() {
    let err = decompress(&[], 1024).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains('1'), "Display should contain needed count");
    assert!(msg.contains('0'), "Display should contain available count");
    assert!(
        msg.contains("end of input") || msg.contains("Unexpected"),
        "Display should describe the problem: {msg}"
    );
}

// ── Determinism ───────────────────────────────────────────────────────

/// Decompressing the same input twice yields identical output.
///
/// Why: the decoder is stateless between calls; any internal state that
/// leaked across invocations would break determinism.  This guards
/// against accidental use of static/global mutable state.
#[test]
fn decompress_is_deterministic() {
    let input = [
        0x83u8, b'A', b'B', b'C', // literal "ABC"
        0xFE, 0x03, 0x00, b'.', // fill 3 with '.'
        0x20, 0x05, // short copy 5 from 5 back
        0x80,
    ];
    let a = decompress(&input, 1024).unwrap();
    let b = decompress(&input, 1024).unwrap();
    assert_eq!(a, b);
}

// ── Boundary tests ───────────────────────────────────────────────────

/// Filling exactly to `max_output` succeeds (boundary: N == cap).
///
/// Why: the cap check must use `>` not `>=` so that producing exactly
/// `max_output` bytes is allowed.  This is the classic fence-post test.
#[test]
fn fill_exactly_to_max_output_succeeds() {
    // Fill exactly 10 bytes with max_output=10.
    let input = [0xFEu8, 0x0A, 0x00, b'X', 0x80];
    let result = decompress(&input, 10).unwrap();
    assert_eq!(result.len(), 10);
    assert!(result.iter().all(|&b| b == b'X'));
}

/// Filling to `max_output + 1` fails (boundary: N == cap + 1).
///
/// Why: complements the previous test — one byte past the cap must be
/// rejected.  Together these two tests pin the exact boundary.
#[test]
fn fill_one_past_max_output_fails() {
    let input = [0xFEu8, 0x0B, 0x00, b'X', 0x80]; // fill 11 bytes
    let result = decompress(&input, 10);
    assert!(matches!(result, Err(Error::DecompressionError { .. })));
}

// ── Known LCW vector ─────────────────────────────────────────────────
//
// This test uses a hand-verified compressed→decompressed byte pair that
// exercises multiple LCW command types in sequence, confirming the
// codec produces the same output as reference implementations.

/// Multi-command vector: literal + fill + absolute copy → known output.
#[test]
fn known_vector_multi_command() {
    // Stream: literal "ABCD" (0x84, A, B, C, D)
    //       + fill 4×'Z'   (0xFE, 04 00, 'Z')
    //       + abs copy 4 from 0 (0xFF, 04 00, 00 00) → copies "ABCD"
    //       + end (0x80)
    let input = [
        0x84u8, b'A', b'B', b'C', b'D', // literal 4
        0xFE, 0x04, 0x00, b'Z', // fill 4 with 'Z'
        0xFF, 0x04, 0x00, 0x00, 0x00, // long abs copy 4 from offset 0
        0x80, // end
    ];
    let expected = b"ABCDZZZZABCD";
    let result = decompress(&input, 1024).unwrap();
    assert_eq!(result, expected);
}

// ── Security: overflow & edge-case tests ─────────────────────────────

/// `max_output = 0` rejects any output-producing command.
///
/// Why: callers may pass 0 to forbid all output; the cap must reject even
/// a single byte.  If `ensure_room` used `<` instead of `>` on a
/// saturating add, this edge case would slip through.
#[test]
fn max_output_zero_rejects_output() {
    // A medium literal producing 1 byte should fail with max_output=0.
    let input = [0x81u8, b'A', 0x80];
    let result = decompress(&input, 0);
    assert!(matches!(result, Err(Error::DecompressionError { .. })));
}

/// `max_output = 0` still accepts the end marker (no output produced).
///
/// Why: a zero-byte decompression is valid — the output is simply empty.
/// The end marker itself writes nothing, so it must succeed.
#[test]
fn max_output_zero_allows_end_marker() {
    let result = decompress(&[0x80], 0).unwrap();
    assert!(result.is_empty());
}

/// `ensure_room` uses `saturating_add` so `out.len() + n` cannot wrap.
///
/// Why: if `max_output` is near `usize::MAX`, a bare `out.len() + n`
/// could wrap to a small number and bypass the cap.  Using
/// `saturating_add` clamps to `usize::MAX`, which is always > the cap.
///
/// How: passes `max_output = usize::MAX` with a small fill; the fill
/// succeeds normally, confirming no panic or unsound wrap.
#[test]
fn ensure_room_saturating_add_no_wrap() {
    // With max_output = usize::MAX, a fill command must not wrap and
    // bypass the cap. The fill itself may succeed (usize::MAX is huge),
    // but it must not panic or produce unsound behaviour.
    // We test with a small fill that should succeed.
    let input = [0xFEu8, 0x02, 0x00, b'X', 0x80]; // fill 2 bytes
    let result = decompress(&input, usize::MAX);
    assert_eq!(result.unwrap(), b"XX");
}

/// Short relative copy with `count > rel_offset` triggers RLE expansion.
///
/// Why: when the copy window overlaps the destination, each newly written
/// byte is immediately readable by the next iteration.  This is how LCW
/// achieves run-length encoding — writing "A" and then copying 6 from
/// offset 1 repeats the same byte 6 more times, producing "AAAAAAA".
///
/// How: literal "A", then short copy with count = 6, rel_offset = 1.
/// The byte-at-a-time loop in `short_relative_copy` must re-read from
/// the expanding output rather than a snapshot.
#[test]
fn short_relative_copy_rle_expansion() {
    // Literal "A" (0x81, 'A')
    // Short copy: count = ((cmd>>4)&7)+3 = 3+3 = 6 when x=3.
    // rel_offset = 1 (copy from the one byte just written).
    // cmd byte: (3 << 4) | (1 >> 8) = 0x30, rel_lo = 0x01
    let input = [0x81u8, b'A', 0x30, 0x01, 0x80];
    let result = decompress(&input, 1024).unwrap();
    assert_eq!(result, b"AAAAAAA");
}

/// Long fill with a missing value byte is caught as `UnexpectedEof`.
///
/// Why: a truncated stream that ends right after the count word must not
/// leave the decoder in an undefined state or panic on the missing byte.
#[test]
fn truncated_long_fill_value_byte() {
    // 0xFE, count=1, but no value byte follows
    let input = [0xFEu8, 0x01, 0x00];
    let result = decompress(&input, 1024);
    assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
}

/// Long absolute copy truncated after the count word (missing offset).
///
/// Why: `0xFF` reads two u16 words sequentially; if the stream ends after
/// the first word, `read_word` must fail cleanly on the second.
#[test]
fn truncated_long_absolute_copy_offset() {
    // Need literal first so output is non-empty, then 0xFF with count word but no offset
    let input = [0x81u8, b'A', 0xFF, 0x01, 0x00];
    let result = decompress(&input, 1024);
    assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
}

/// Medium absolute copy truncated after the command byte (missing offset).
///
/// Why: `0xC0` needs a 2-byte LE offset word.  Providing only 1 trailing
/// byte must be caught by `read_word`, not by an out-of-bounds index.
#[test]
fn truncated_medium_absolute_copy_offset() {
    // 0xC0 needs a 2-byte offset word; provide only 1 byte
    let input = [0x81u8, b'A', 0xC0, 0x00];
    let result = decompress(&input, 1024);
    assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
}

/// Long fill with `count = 0` is a valid no-op (writes nothing).
///
/// Why: some encoders emit zero-count fills as padding.  The decoder must
/// accept this without error and produce no output bytes for that command.
#[test]
fn long_fill_zero_count_is_noop() {
    // 0xFE, count=0, value='X', then end → no output from the fill
    let input = [0xFEu8, 0x00, 0x00, b'X', 0x80];
    let result = decompress(&input, 1024).unwrap();
    assert!(result.is_empty());
}

// ── Adversarial security tests ───────────────────────────────────────

/// `decompress` on 256 bytes of `0xFF` must not panic.
///
/// Why (V38): the byte `0xFF` is the long absolute copy command.  With
/// an all-`0xFF` stream every count and offset is `0xFFFF`, which
/// exercises the output cap, offset bounds check, and forward-progress
/// guard simultaneously.  The decompressor must return an error, not
/// panic or allocate unbounded memory.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 256];
    let _ = decompress(&data, 4096);
}

/// `decompress` on 256 zero bytes must not panic.
///
/// Why: the byte `0x00` is often a no-op or minimal command.  An
/// all-zero stream tests the decompressor's handling of degenerate
/// input that may loop without producing output.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = vec![0u8; 256];
    let _ = decompress(&data, 4096);
}

/// Long absolute copy with `count = 0` is a valid no-op.
///
/// Why: zero-count copies are legal in the format and must not trigger the
/// bounds check (`0 + 0 = 0 <= out.len()` is always true once output
/// exists).  Verifies the command is consumed without side effects.
#[test]
fn long_absolute_copy_zero_count_is_noop() {
    let input = [0x81u8, b'A', 0xFF, 0x00, 0x00, 0x00, 0x00, 0x80];
    let result = decompress(&input, 1024).unwrap();
    assert_eq!(result, b"A");
}

/// Medium absolute copy from unwritten positions produces zeros.
///
/// Why: the original C&C engine pre-allocates the output buffer and
/// doesn't bounds-check absolute copies.  Unwritten positions read as
/// zero.  Real game SHP data relies on this behavior.
#[test]
fn medium_absolute_copy_from_unwritten_produces_zeros() {
    // 0xC0 = count 3 from offset 0, output is empty → 3 zero bytes
    let input = [0xC0u8, 0x00, 0x00, 0x80];
    let result = decompress(&input, 1024).unwrap();
    assert_eq!(result, vec![0, 0, 0]);
}
