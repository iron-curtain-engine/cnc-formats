// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Format40 XOR-delta decoder.
//!
//! Format40 is a byte-level XOR-delta encoding used by several Westwood file
//! formats (SHP keyframe deltas, WSA animation frames).  The command stream
//! encodes only the pixels that differ from a reference frame, using skip/XOR
//! commands to avoid touching unchanged regions.
//!
//! ## Command Table
//!
//! | First byte                                       | Meaning                                              |
//! |--------------------------------------------------|------------------------------------------------------|
//! | `0x81`–`0xFF`                                    | **Small skip** — advance dest by `cmd & 0x7F` pixels |
//! | `0x80` + u16le `w` = 0                           | **End of stream**                                    |
//! | `0x80` + u16le `w` (bits 15-14 = `00` or `01`)   | **Big skip** — advance dest by `w` pixels            |
//! | `0x80` + u16le `w` (bits 15-14 = `10`)            | **Big XOR** — read `w & 0x3FFF` bytes, XOR into dest |
//! | `0x80` + u16le `w` (bits 15-14 = `11`) + byte `v` | **Big XOR value** — XOR `w & 0x3FFF` dest pixels with `v` |
//! | `0x01`–`0x7F`                                    | **Small XOR** — read `cmd` bytes, XOR each into dest |
//! | `0x00` + byte `n` + byte `v`                     | **Repeated XOR** — XOR `n` dest pixels with `v`      |
//!
//! ## References
//!
//! Documented in community resources: C&C Modding Wiki "XOR Delta" page,
//! ModEnc "Format40" page, XCC Utilities source code, and Olaf van der Spek's
//! format descriptions.  Clean-room implementation from publicly available
//! format specifications.

use crate::error::Error;

/// Apply a Format40-encoded XOR-delta stream to `dest`.
///
/// `dest` must already contain the reference frame pixels.  The delta stream
/// is XOR'd onto the existing contents — skip commands leave pixels untouched,
/// XOR commands flip only the changed bits.
///
/// # Errors
///
/// Returns [`Error::DecompressionError`] if the delta stream references
/// pixels beyond the bounds of `dest` or if the stream is truncated.
pub fn apply_xor_delta(dest: &mut [u8], delta: &[u8]) -> Result<(), Error> {
    let mut di = 0usize; // dest index
    let mut si = 0usize; // delta stream index

    while si < delta.len() {
        let cmd = delta[si];
        si += 1;

        if (cmd & 0x80) != 0 {
            if cmd == 0x80 {
                // Multi-byte command: read u16le word.
                if si + 2 > delta.len() {
                    return Err(Error::DecompressionError {
                        reason: "Format40: truncated multi-byte command",
                    });
                }
                let word = u16::from_le_bytes([delta[si], delta[si + 1]]) as usize;
                si += 2;

                if word == 0 {
                    break; // end of stream
                }

                match (word >> 14) & 3 {
                    0 | 1 => {
                        // Big skip: advance dest by `word` pixels.
                        di += word;
                        if di > dest.len() {
                            return Err(Error::DecompressionError {
                                reason: "Format40: big skip past end of dest",
                            });
                        }
                    }
                    2 => {
                        // Big XOR from stream.
                        let count = word & 0x3FFF;
                        if si + count > delta.len() || di + count > dest.len() {
                            return Err(Error::DecompressionError {
                                reason: "Format40: big XOR overruns buffer",
                            });
                        }
                        for k in 0..count {
                            dest[di + k] ^= delta[si + k];
                        }
                        di += count;
                        si += count;
                    }
                    _ => {
                        // Big XOR with single value.
                        let count = word & 0x3FFF;
                        if si >= delta.len() || di + count > dest.len() {
                            return Err(Error::DecompressionError {
                                reason: "Format40: big XOR value overruns buffer",
                            });
                        }
                        let value = delta[si];
                        si += 1;
                        for k in 0..count {
                            dest[di + k] ^= value;
                        }
                        di += count;
                    }
                }
            } else {
                // Small skip: cmd & 0x7F pixels.
                di += (cmd & 0x7F) as usize;
                if di > dest.len() {
                    return Err(Error::DecompressionError {
                        reason: "Format40: small skip past end of dest",
                    });
                }
            }
        } else if cmd == 0 {
            // Repeated XOR: count + value.
            if si + 2 > delta.len() {
                return Err(Error::DecompressionError {
                    reason: "Format40: truncated repeated XOR",
                });
            }
            let count = delta[si] as usize;
            let value = delta[si + 1];
            si += 2;
            if di + count > dest.len() {
                return Err(Error::DecompressionError {
                    reason: "Format40: repeated XOR overruns dest",
                });
            }
            for k in 0..count {
                dest[di + k] ^= value;
            }
            di += count;
        } else {
            // Small XOR from stream: count = cmd.
            let count = cmd as usize;
            if si + count > delta.len() || di + count > dest.len() {
                return Err(Error::DecompressionError {
                    reason: "Format40: small XOR overruns buffer",
                });
            }
            for k in 0..count {
                dest[di + k] ^= delta[si + k];
            }
            di += count;
            si += count;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_xor_from_stream() {
        let mut dest = [0xAAu8; 4];
        // cmd=4 (small XOR: 4 bytes from stream), then data
        let delta = [0x04, 0x11, 0x22, 0x33, 0x44];
        apply_xor_delta(&mut dest, &delta).unwrap();
        assert_eq!(dest, [0xAA ^ 0x11, 0xAA ^ 0x22, 0xAA ^ 0x33, 0xAA ^ 0x44]);
    }

    #[test]
    fn small_skip() {
        let mut dest = [0xAAu8; 4];
        // skip 2, then XOR 2 bytes
        let delta = [0x82, 0x02, 0x11, 0x22];
        apply_xor_delta(&mut dest, &delta).unwrap();
        assert_eq!(dest, [0xAA, 0xAA, 0xAA ^ 0x11, 0xAA ^ 0x22]);
    }

    #[test]
    fn end_of_stream() {
        let mut dest = [0xAAu8; 4];
        // end marker: 0x80 + u16le(0)
        let delta = [0x80, 0x00, 0x00];
        apply_xor_delta(&mut dest, &delta).unwrap();
        assert_eq!(dest, [0xAA; 4]); // unchanged
    }

    #[test]
    fn repeated_xor() {
        let mut dest = [0x00u8; 4];
        // cmd=0x00, count=4, value=0xFF
        let delta = [0x00, 0x04, 0xFF];
        apply_xor_delta(&mut dest, &delta).unwrap();
        assert_eq!(dest, [0xFF; 4]);
    }

    #[test]
    fn overrun_returns_error() {
        let mut dest = [0u8; 2];
        // cmd=4: wants 4 bytes but dest is only 2
        let delta = [0x04, 0x11, 0x22, 0x33, 0x44];
        let result = apply_xor_delta(&mut dest, &delta);
        assert!(result.is_err());
    }

    #[test]
    fn big_skip_overrun_returns_error() {
        let mut dest = [0u8; 4];
        // 0x80 + u16le(0x7FFF) = big skip of 32767 pixels — way past dest
        let delta = [0x80, 0xFF, 0x7F];
        let result = apply_xor_delta(&mut dest, &delta);
        assert!(result.is_err());
    }

    #[test]
    fn small_skip_overrun_returns_error() {
        let mut dest = [0u8; 4];
        // 0xFF = small skip of 127 pixels — past dest
        let delta = [0xFF];
        let result = apply_xor_delta(&mut dest, &delta);
        assert!(result.is_err());
    }

    #[test]
    fn big_xor_from_stream() {
        let mut dest = [0x00u8; 4];
        // 0x80 + u16le(0x8004) = big XOR, bits 15-14 = 10, count = 4
        let delta = [0x80, 0x04, 0x80, 0xAA, 0xBB, 0xCC, 0xDD];
        apply_xor_delta(&mut dest, &delta).unwrap();
        assert_eq!(dest, [0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn big_xor_value() {
        let mut dest = [0x00u8; 4];
        // 0x80 + u16le(0xC004) = big XOR value, bits 15-14 = 11, count = 4, value = 0xFF
        let delta = [0x80, 0x04, 0xC0, 0xFF];
        apply_xor_delta(&mut dest, &delta).unwrap();
        assert_eq!(dest, [0xFF; 4]);
    }

    // ── Security Edge Cases (V38) ──────────────────────────────────────

    /// `apply_xor_delta` on 256 bytes of `0xFF` must not panic.
    ///
    /// Why (V38): all-ones input maximises command bytes and word values,
    /// exercising skip bounds, XOR count caps, and truncation guards.
    #[test]
    fn adversarial_all_ff_no_panic() {
        let mut dest = vec![0u8; 256];
        let delta = vec![0xFFu8; 256];
        let _ = apply_xor_delta(&mut dest, &delta);
    }

    /// `apply_xor_delta` on 256 bytes of `0x00` must not panic.
    ///
    /// Why (V38): all-zero input exercises the repeated-XOR command path
    /// (cmd=0x00) with zero counts, potential infinite loops, and edge
    /// cases in the end-of-stream detection.
    #[test]
    fn adversarial_all_zero_no_panic() {
        let mut dest = vec![0u8; 256];
        let delta = vec![0x00u8; 256];
        let _ = apply_xor_delta(&mut dest, &delta);
    }

    /// Truncated multi-byte command (0x80 without following u16).
    #[test]
    fn truncated_multi_byte_command() {
        let mut dest = [0u8; 4];
        let delta = [0x80, 0x01]; // only 1 byte after 0x80, need 2
        let result = apply_xor_delta(&mut dest, &delta);
        assert!(result.is_err());
    }

    /// Empty delta stream is a no-op.
    #[test]
    fn empty_delta_no_op() {
        let mut dest = [0xAAu8; 4];
        apply_xor_delta(&mut dest, &[]).unwrap();
        assert_eq!(dest, [0xAA; 4]);
    }
}
