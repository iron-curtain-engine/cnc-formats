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
///
/// Why (V38): without a ratio cap, a malicious 4-byte input could claim
/// gigabytes of output.  Real C&C assets never exceed ~50:1; 256:1 is
/// generous enough for any legitimate file while preventing DoS.
const MAX_RATIO: usize = 256;

/// Internal decoder state for LCW decompression.
///
/// ## Design
///
/// The decoder is a struct (not a free function) so that each command type
/// can be a separate method.  This keeps the main dispatch loop (`run`)
/// small and each command handler independently testable.
///
/// ## Safety Invariants
///
/// - `pos` only advances forward; it is never decremented.
/// - `out.len()` is checked against `max_output` before every push.
/// - All source reads go through `read_byte` / `read_word`, which return
///   `UnexpectedEof` rather than indexing past the end.
struct LcwDecoder<'a> {
    src: &'a [u8],
    pos: usize,
    out: Vec<u8>,
    max_output: usize,
}

impl<'a> LcwDecoder<'a> {
    /// Creates a new decoder.
    ///
    /// The initial allocation is capped at `min(max_output, src.len() * 256)`
    /// to avoid over-allocating when `max_output` is much larger than what
    /// the input can possibly produce.
    fn new(src: &'a [u8], max_output: usize) -> Self {
        let cap = max_output.min(src.len().saturating_mul(MAX_RATIO));
        Self {
            src,
            pos: 0,
            out: Vec::with_capacity(cap),
            max_output,
        }
    }

    /// Reads one byte from the compressed source, advancing `pos`.
    ///
    /// Returns `UnexpectedEof` if the source is exhausted.  This is the
    /// *only* read path for source bytes — no direct indexing elsewhere.
    fn read_byte(&mut self) -> Result<u8, Error> {
        let b = *self.src.get(self.pos).ok_or(Error::UnexpectedEof {
            needed: self.pos.saturating_add(1),
            available: self.src.len(),
        })?;
        self.pos += 1;
        Ok(b)
    }

    /// Reads a little-endian u16 from the compressed source.
    ///
    /// How: two consecutive bytes, low byte first.  Advances `pos` by 2.
    fn read_word(&mut self) -> Result<u16, Error> {
        let end = self.pos.checked_add(2).ok_or(Error::UnexpectedEof {
            needed: usize::MAX,
            available: self.src.len(),
        })?;
        let slice = self.src.get(self.pos..end).ok_or(Error::UnexpectedEof {
            needed: end,
            available: self.src.len(),
        })?;
        self.pos = end;
        // Safe: .get() above guarantees exactly 2 bytes in `slice`.
        Ok(u16::from_le_bytes([slice[0], slice[1]]))
    }

    /// V38 output-size guard: checks that appending `n` bytes would not
    /// exceed `max_output`.
    ///
    /// Uses `saturating_add` to prevent `out.len() + n` from wrapping on
    /// 32-bit platforms when `n` comes from untrusted input.
    fn ensure_room(&self, n: usize) -> Result<(), Error> {
        if self.out.len().saturating_add(n) > self.max_output {
            return Err(Error::DecompressionError {
                reason: "output would exceed max_output",
            });
        }
        Ok(())
    }

    /// Short relative copy: `0b0xxx_yyyy yyyyyyyy`.
    ///
    /// Copies `count` (3–10) bytes from `out[write_pos - rel_offset ..]`.
    /// The offset is relative to the *current* write position, not absolute.
    ///
    /// Why relative: this encoding is more compact for recently-written data,
    /// which is common in RLE-like sprite patterns.
    fn short_relative_copy(&mut self, cmd: u8) -> Result<(), Error> {
        // Extract count (bits 6–4, plus implicit +3) and 12-bit relative offset.
        let count = (((cmd >> 4) & 0x07) as usize) + 3;
        let rel_hi = (cmd & 0x0F) as usize;
        let rel_lo = self.read_byte()? as usize;
        let rel_offset = (rel_hi << 8) | rel_lo;

        if rel_offset == 0 {
            return Err(Error::DecompressionError {
                reason: "short copy relative offset is zero",
            });
        }
        if rel_offset > self.out.len() {
            return Err(Error::DecompressionError {
                reason: "short copy relative offset exceeds output length",
            });
        }
        self.ensure_room(count)?;
        let start = self.out.len() - rel_offset;
        if count <= rel_offset {
            // Non-overlapping: source is fully within existing output.
            // extend_from_within avoids per-byte bounds checks.
            self.out.extend_from_within(start..start + count);
        } else {
            // Overlapping (RLE-like): source extends into bytes being written,
            // so we must copy byte-by-byte to reproduce the repeating pattern.
            //
            // Safety of direct indexing: `start = out.len() - rel_offset` and
            // `rel_offset > 0` (checked above).  At iteration `i`, the output
            // length is `original_len + i`, so `start + i` is always strictly
            // less than the current length.  This is the decoder's own output
            // buffer, not untrusted input, and the invariant is loop-local.
            for i in 0..count {
                let byte = self.out[start + i];
                self.out.push(byte);
            }
        }
        Ok(())
    }

    /// Medium literal: `0b10xx_xxxx` followed by `count` literal bytes.
    ///
    /// Copies `count` bytes verbatim from the source stream into the output.
    /// This is the only command that reads raw pixel data.
    ///
    /// Uses `extend_from_slice` for a single bounds check + memcpy instead
    /// of `count` individual `read_byte` + `push` calls.
    fn medium_literal(&mut self, cmd: u8) -> Result<(), Error> {
        let count = (cmd & 0x3F) as usize;
        self.ensure_room(count)?;
        let end = self.pos.checked_add(count).ok_or(Error::UnexpectedEof {
            needed: usize::MAX,
            available: self.src.len(),
        })?;
        let slice = self.src.get(self.pos..end).ok_or(Error::UnexpectedEof {
            needed: end,
            available: self.src.len(),
        })?;
        self.out.extend_from_slice(slice);
        self.pos = end;
        Ok(())
    }

    /// Long absolute copy: `0xFF count:u16 offset:u16`.
    ///
    /// Copies `count` bytes starting at *absolute* offset in the output buffer.
    /// Used for back-references to any position in the decompressed output,
    /// unlike short copies which are limited to a 12-bit relative window.
    fn long_absolute_copy(&mut self) -> Result<(), Error> {
        let count = self.read_word()? as usize;
        let abs_offset = self.read_word()? as usize;
        if abs_offset.saturating_add(count) > self.out.len() {
            return Err(Error::DecompressionError {
                reason: "long absolute copy source exceeds output length",
            });
        }
        self.ensure_room(count)?;
        // Source range is fully within existing output (validated above),
        // so extend_from_within is safe and avoids per-byte bounds checks.
        self.out.extend_from_within(abs_offset..abs_offset + count);
        Ok(())
    }

    /// Long fill: `0xFE count:u16 value:u8`.
    ///
    /// Fills `count` bytes of the output with the single repeated `value`.
    /// This is the primary RLE command — commonly used for large spans of a
    /// single colour in sprite data.
    fn long_fill(&mut self) -> Result<(), Error> {
        let count = self.read_word()? as usize;
        let value = self.read_byte()?;
        self.ensure_room(count)?;
        // Vec::resize is backed by memset for u8 — much faster than N pushes.
        self.out.resize(self.out.len() + count, value);
        Ok(())
    }

    /// Medium absolute copy: `0b11xx_xxxx offset:u16` (0xC0..0xFD).
    ///
    /// Like long absolute copy but the count is encoded in the command byte
    /// itself (6 bits, plus implicit +3, giving a range of 3–66).  The
    /// smaller encoding makes this more compact for typical back-references.
    fn medium_absolute_copy(&mut self, cmd: u8) -> Result<(), Error> {
        let count = ((cmd & 0x3F) as usize) + 3;
        let abs_offset = self.read_word()? as usize;
        if abs_offset.saturating_add(count) > self.out.len() {
            return Err(Error::DecompressionError {
                reason: "medium absolute copy source exceeds output length",
            });
        }
        self.ensure_room(count)?;
        // Source range is fully within existing output (validated above),
        // so extend_from_within is safe and avoids per-byte bounds checks.
        self.out.extend_from_within(abs_offset..abs_offset + count);
        Ok(())
    }

    /// Main dispatch loop — reads commands until the `0x80` end-of-stream marker.
    ///
    /// ## Command Priority
    ///
    /// The dispatch order matches Westwood's original decoder:
    /// 1. `0x80` → end of stream (terminates loop)
    /// 2. `0x00–0x7F` → short relative copy (bit 7 clear)
    /// 3. `0x81–0xBF` → medium literal (bits 7–6 = `10`)
    /// 4. `0xFF` → long absolute copy
    /// 5. `0xFE` → long fill
    /// 6. `0xC0–0xFD` → medium absolute copy
    ///
    /// V38 forward-progress: every iteration either advances `pos` (reads at
    /// least one source byte) or terminates, preventing infinite loops.
    fn run(mut self) -> Result<Vec<u8>, Error> {
        loop {
            let cmd = self.read_byte()?;

            if cmd == 0x80 {
                break;
            } else if (cmd & 0x80) == 0 {
                self.short_relative_copy(cmd)?;
            } else if (cmd & 0xC0) == 0x80 {
                self.medium_literal(cmd)?;
            } else {
                match cmd {
                    0xFF => self.long_absolute_copy()?,
                    0xFE => self.long_fill()?,
                    _ => self.medium_absolute_copy(cmd)?,
                }
            }
        }
        Ok(self.out)
    }
}

/// Decompresses an LCW-compressed byte slice into a fresh `Vec<u8>`.
///
/// `max_output` is the expected decompressed size; it is used both to
/// pre-allocate and as an upper bound (returns [`Error::DecompressionError`]
/// if the output would exceed `max_output`).
pub fn decompress(src: &[u8], max_output: usize) -> Result<Vec<u8>, Error> {
    LcwDecoder::new(src, max_output).run()
}

#[cfg(test)]
mod tests;
