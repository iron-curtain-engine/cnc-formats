// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! LCW (Lempel-Castle-Welch) decompression.
//!
//! LCW is Westwood's primary compression algorithm, used for SHP frame data,
//! VQA video chunks, icon set data, and other compressed resources.
//!
//! ## Command Encoding
//!
//! Each command byte selects one of six operations:
//!
//! | First byte        | Operation                                              |
//! |-------------------|--------------------------------------------------------|
//! | `0x80`            | End-of-stream marker                                   |
//! | `0x81`–`0xBF`     | Medium literal: copy next `byte & 0x3F` bytes verbatim |
//! | `0x00`–`0x7F`     | Short relative copy from output history               |
//! | `0xC0`–`0xFD`     | Medium absolute copy from output buffer               |
//! | `0xFE bb bb bb`   | Long fill: repeat byte `bb` for `word` count          |
//! | `0xFF bb bb bb bb`| Long absolute copy from output buffer                 |
//!
//! Short copies use *relative* offsets from the current write position.
//! Medium and long copies use *absolute* offsets from the start of the output.

use crate::error::Error;

/// Maximum decompression expansion ratio (256:1) used as a safety cap.
const MAX_RATIO: usize = 256;

/// Decompresses an LCW-compressed byte slice into a fresh `Vec<u8>`.
///
/// `max_output` is the expected decompressed size; it is used both to
/// pre-allocate and as an upper bound (returns [`Error::DecompressionError`]
/// if the output would exceed `max_output`).
pub fn decompress(src: &[u8], max_output: usize) -> Result<Vec<u8>, Error> {
    // Sanity cap: refuse to allocate absurd buffers.
    let cap = max_output.min(src.len().saturating_mul(MAX_RATIO));
    let mut out: Vec<u8> = Vec::with_capacity(cap);
    let mut pos = 0usize; // read cursor in src

    macro_rules! read_byte {
        () => {{
            if pos >= src.len() {
                return Err(Error::UnexpectedEof);
            }
            let b = src[pos];
            pos += 1;
            b
        }};
    }

    macro_rules! read_word {
        () => {{
            if pos + 1 >= src.len() {
                return Err(Error::UnexpectedEof);
            }
            let lo = src[pos] as u16;
            let hi = src[pos + 1] as u16;
            pos += 2;
            (hi << 8) | lo
        }};
    }

    macro_rules! ensure_output_room {
        ($n:expr) => {
            if out.len() + $n > max_output {
                return Err(Error::DecompressionError);
            }
        };
    }

    loop {
        let cmd = read_byte!();

        if cmd == 0x80 {
            // End-of-stream marker.
            break;
        } else if (cmd & 0x80) == 0 {
            // ── Short relative copy ──────────────────────────────────────────
            // Format: 0b0xxx_yyyy yyyyyyyy
            //   count       = (bits 6-4) + 3  →  3..10
            //   rel_offset  = (bits 3-0) << 8 | next byte  (12-bit, 1..4095)
            let count = (((cmd >> 4) & 0x07) as usize) + 3;
            let rel_hi = (cmd & 0x0F) as usize;
            let rel_lo = read_byte!() as usize;
            let rel_offset = (rel_hi << 8) | rel_lo;

            if rel_offset == 0 {
                return Err(Error::DecompressionError);
            }
            if rel_offset > out.len() {
                return Err(Error::DecompressionError);
            }
            ensure_output_room!(count);
            let start = out.len() - rel_offset;
            for i in 0..count {
                let byte = out[start + i];
                out.push(byte);
            }
        } else if (cmd & 0xC0) == 0x80 {
            // ── Medium literal ───────────────────────────────────────────────
            // Format: 0b10xx_xxxx  (0x81..0xBF)
            //   count = byte & 0x3F  (1..63)
            let count = (cmd & 0x3F) as usize;
            // count == 0 means 0x80, handled above as end marker.
            ensure_output_room!(count);
            for _ in 0..count {
                let b = read_byte!();
                out.push(b);
            }
        } else {
            // bits 7-6 are 11  →  0xC0..0xFF
            match cmd {
                0xFF => {
                    // ── Long absolute copy ───────────────────────────────────
                    // Format: 0xFF  count:u16  offset:u16
                    let count = read_word!() as usize;
                    let abs_offset = read_word!() as usize;
                    if abs_offset + count > out.len() {
                        return Err(Error::DecompressionError);
                    }
                    ensure_output_room!(count);
                    for i in 0..count {
                        let byte = out[abs_offset + i];
                        out.push(byte);
                    }
                }
                0xFE => {
                    // ── Long fill ────────────────────────────────────────────
                    // Format: 0xFE  count:u16  value:u8
                    let count = read_word!() as usize;
                    let value = read_byte!();
                    ensure_output_room!(count);
                    for _ in 0..count {
                        out.push(value);
                    }
                }
                _ => {
                    // ── Medium absolute copy ─────────────────────────────────
                    // Format: 0b11xx_xxxx  offset:u16    (0xC0..0xFD)
                    //   count      = (byte & 0x3F) + 3   →  3..66
                    //   abs_offset = next word (little-endian)
                    let count = ((cmd & 0x3F) as usize) + 3;
                    let abs_offset = read_word!() as usize;
                    if abs_offset + count > out.len() {
                        return Err(Error::DecompressionError);
                    }
                    ensure_output_room!(count);
                    for i in 0..count {
                        let byte = out[abs_offset + i];
                        out.push(byte);
                    }
                }
            }
        }
    }

    Ok(out)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// End-of-stream marker alone → empty output.
    #[test]
    fn test_end_marker_only() {
        let result = decompress(&[0x80], 1024).unwrap();
        assert!(result.is_empty());
    }

    /// Medium literal: 0x83 = copy 3 literal bytes.
    #[test]
    fn test_medium_literal_three_bytes() {
        // 0x83 = 0b10000011 → medium literal, count = 3
        let input = [0x83u8, b'A', b'B', b'C', 0x80];
        let out = decompress(&input, 1024).unwrap();
        assert_eq!(out, b"ABC");
    }

    /// Medium literal: 0x81 = copy 1 byte.
    #[test]
    fn test_medium_literal_one_byte() {
        let input = [0x81u8, b'Z', 0x80];
        let out = decompress(&input, 1024).unwrap();
        assert_eq!(out, b"Z");
    }

    /// Long fill: 0xFE, count=4, value='X' → "XXXX".
    #[test]
    fn test_long_fill() {
        // 0xFE  count:u16=4  value='X'
        let input = [0xFEu8, 0x04, 0x00, b'X', 0x80];
        let out = decompress(&input, 1024).unwrap();
        assert_eq!(out, b"XXXX");
    }

    /// Short relative copy: write "ABC" then copy 3 from 3 back → "ABCABC".
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

    /// Short relative copy with larger count: x=2 → count=5.
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

    /// Medium absolute copy: write "XYZ" then copy 3 bytes from offset 0.
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

    /// Long absolute copy: 0xFF, copy N bytes from absolute offset.
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

    /// Output size cap: refuse to exceed max_output.
    #[test]
    fn test_output_cap_enforced() {
        // Long fill of 100 bytes but max_output=50 → error
        let input = [0xFEu8, 0x64, 0x00, b'!', 0x80]; // fill 100 bytes
        let result = decompress(&input, 50);
        assert_eq!(result, Err(Error::DecompressionError));
    }

    /// Truncated input returns UnexpectedEof.
    #[test]
    fn test_truncated_medium_literal() {
        // 0x83 promises 3 bytes but only 1 follows
        let input = [0x83u8, b'A'];
        let result = decompress(&input, 1024);
        assert_eq!(result, Err(Error::UnexpectedEof));
    }

    /// Chained operations: literal + fill + relative copy.
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
}
