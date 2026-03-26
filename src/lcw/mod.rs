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
struct LcwDecoder<'input> {
    src: &'input [u8],
    pos: usize,
    out: Vec<u8>,
    max_output: usize,
}

impl<'input> LcwDecoder<'input> {
    /// Creates a new decoder.
    ///
    /// The initial allocation is capped at `min(max_output, src.len() * 256)`
    /// to avoid over-allocating when `max_output` is much larger than what
    /// the input can possibly produce.
    fn new(src: &'input [u8], max_output: usize) -> Self {
        let cap = max_output.min(src.len().saturating_mul(MAX_RATIO));
        Self {
            src,
            pos: 0,
            out: Vec::with_capacity(cap),
            max_output,
        }
    }

    /// Creates a decoder that reuses an existing allocation as output buffer.
    ///
    /// `dst` is cleared before use; its capacity is preserved.  This avoids
    /// a heap allocation when the caller already has a suitably-sized buffer
    /// (e.g. reusing the codebook Vec across frame updates).
    fn with_buffer(src: &'input [u8], mut dst: Vec<u8>, max_output: usize) -> Self {
        dst.clear();
        let cap = max_output.min(src.len().saturating_mul(MAX_RATIO));
        if dst.capacity() < cap {
            dst.reserve(cap - dst.capacity());
        }
        Self {
            src,
            pos: 0,
            out: dst,
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
        // Safe via .get(): slice is guaranteed exactly 2 bytes by .get(pos..end).
        let lo = slice.first().copied().unwrap_or(0);
        let hi = slice.get(1).copied().unwrap_or(0);
        Ok(u16::from_le_bytes([lo, hi]))
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
        // EA's encoding (from LCW.CPP):
        //   count    = (op_code >> 4) + 3;
        //   copy_ptr = dest_ptr - (*source_ptr + ((op_code & 0x0f) << 8));
        //
        // Format: 0CCC_RRRR LLLLLLLL
        //   C = count - 3 (3 bits, range 0–7 → count 3–10)
        //   R = high 4 bits of relative offset
        //   L = low 8 bits of relative offset (next byte)
        let count = (((cmd >> 4) & 0x07) as usize) + 3;
        let rel_lo = self.read_byte()? as usize;
        let rel_hi = (cmd & 0x0F) as usize;
        let rel_offset = (rel_hi << 8) | rel_lo;

        if rel_offset == 0 {
            // Zero offset = copy from current position (no-op in practice,
            // but some streams use it).  Treat as zero fill.
            self.ensure_room(count)?;
            for _ in 0..count {
                self.out.push(0);
            }
        } else if rel_offset > self.out.len() {
            // EA's engine pre-allocates the destination buffer.  A relative
            // offset exceeding the current write position reads from the
            // pre-zeroed region before the start of written data.  We
            // replicate this by emitting zeros.
            self.ensure_room(count)?;
            for _ in 0..count {
                self.out.push(0);
            }
        } else {
            self.ensure_room(count)?;
            let start = self.out.len() - rel_offset;
            // Byte-by-byte copy handles both overlapping (RLE) and
            // non-overlapping cases correctly.
            for i in 0..count {
                let byte = self.out.get(start + i).copied().unwrap_or(0);
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
        self.ensure_room(count)?;
        // The original C&C engine pre-allocates the destination buffer and
        // doesn't bounds-check copies.  Real game data may reference bytes
        // beyond what's been written so far (they read as zero from the
        // pre-zeroed buffer).  We replicate this by treating unwritten
        // positions as 0x00.
        for i in 0..count {
            let byte = self.out.get(abs_offset + i).copied().unwrap_or(0);
            self.out.push(byte);
        }
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
        self.ensure_room(count)?;
        // Same as long_absolute_copy: real game data may reference bytes
        // beyond what's been written so far (pre-zeroed buffer semantics).
        for i in 0..count {
            let byte = self.out.get(abs_offset + i).copied().unwrap_or(0);
            self.out.push(byte);
        }
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
            match cmd >> 6 {
                0 | 1 => self.short_relative_copy(cmd)?, // 0x00–0x7F
                2 => {
                    if cmd == 0x80 {
                        break;
                    } // end-of-stream
                    self.medium_literal(cmd)?; // 0x81–0xBF
                }
                _ => match cmd {
                    // 0xC0–0xFF
                    0xFF => self.long_absolute_copy()?,
                    0xFE => self.long_fill()?,
                    _ => self.medium_absolute_copy(cmd)?,
                },
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

/// Decompresses an LCW-compressed byte slice into an existing `Vec<u8>`,
/// reusing its heap allocation.
///
/// `dst` is cleared before decompression begins; its capacity is preserved
/// so callers that call this repeatedly (e.g. per-codebook-update) avoid
/// repeated heap allocation.
///
/// `max_output` is both the pre-allocation hint and the safety cap.
pub fn decompress_into(src: &[u8], dst: &mut Vec<u8>, max_output: usize) -> Result<(), Error> {
    let buf = std::mem::take(dst);
    match LcwDecoder::with_buffer(src, buf, max_output).run() {
        Ok(out) => {
            *dst = out;
            Ok(())
        }
        Err(e) => {
            *dst = Vec::new();
            Err(e)
        }
    }
}

// ── LCW Compressor ───────────────────────────────────────────────────────────
//
// Produces a valid LCW byte stream from raw pixel data.  The compressor uses
// three LCW command types:
//
// - **Long fill (0xFE):** for runs of 3+ identical bytes (RLE).
// - **Short relative copy:** for back-references within a 4095-byte window
//   (length 3–10, 12-bit relative offset).
// - **Medium literal (0x81–0xBF):** for non-compressible spans (up to 63
//   bytes at a time).
//
// The compressor scans greedily: at each position it first tries a fill run,
// then a back-reference, and falls back to a literal.  This produces output
// comparable to Westwood's original compressor on typical sprite data.
//
// The algorithm is clean-room: implemented from the publicly documented LCW
// command encoding (see module-level docs) without reference to any EA code.

/// Maximum relative-copy back-reference distance (12-bit offset field).
const MAX_REL_OFFSET: usize = 4095;

/// Maximum relative-copy length (3-bit count field + 3 = 3..10).
const MAX_REL_COUNT: usize = 10;

/// Minimum match length that justifies a back-reference (shorter matches
/// expand to more bytes than a literal copy).
const MIN_MATCH_LEN: usize = 3;

/// Maximum literal chunk size per medium-literal command (6-bit field).
const MAX_LITERAL_CHUNK: usize = 63;

/// Maximum fill run length per long-fill command (16-bit count field).
const MAX_FILL_LEN: usize = 65535;

/// Compresses raw pixel data into an LCW byte stream.
///
/// The output is a valid LCW stream that [`decompress`] can round-trip back
/// to the original bytes.  Uses fill, relative-copy, and literal commands.
///
/// Returns the compressed bytes including the `0x80` end-of-stream marker.
pub fn compress(input: &[u8]) -> Vec<u8> {
    // Worst case: every byte is a literal → ~(len/63 + len + 1) bytes.
    let mut out = Vec::with_capacity(input.len().saturating_add(input.len() / 63 + 16));
    let mut pos = 0;

    while pos < input.len() {
        // ── Try fill run (0xFE) ──────────────────────────────────────
        // Count consecutive bytes equal to input[pos].
        let fill_val = input.get(pos).copied().unwrap_or(0);
        let mut fill_len = 1usize;
        while pos + fill_len < input.len()
            && input.get(pos + fill_len).copied() == Some(fill_val)
            && fill_len < MAX_FILL_LEN
        {
            fill_len += 1;
        }
        if fill_len >= MIN_MATCH_LEN {
            // Emit long fill: 0xFE, count:u16 LE, value:u8.
            out.push(0xFE);
            out.extend_from_slice(&(fill_len as u16).to_le_bytes());
            out.push(fill_val);
            pos += fill_len;
            continue;
        }

        // ── Try short relative copy ──────────────────────────────────
        // Search backwards in the sliding window for a matching run.
        let best = find_best_match(input, pos);
        if best.len >= MIN_MATCH_LEN {
            // Emit short relative copy: 0b0CCC_OOOO OOOOOOOO
            // where CCC = (count-3), O…O = 12-bit relative offset.
            let count_field = (best.len - 3) as u8;
            let rel_hi = ((best.offset >> 8) & 0x0F) as u8;
            let rel_lo = (best.offset & 0xFF) as u8;
            out.push((count_field << 4) | rel_hi);
            out.push(rel_lo);
            pos += best.len;
            continue;
        }

        // ── Fall back to literal ─────────────────────────────────────
        // Accumulate non-compressible bytes until we hit a fill or match.
        let lit_start = pos;
        let mut lit_end = pos + 1;
        while lit_end < input.len() && (lit_end - lit_start) < MAX_LITERAL_CHUNK {
            // Peek ahead: if the next position starts a fill ≥ 3 or a
            // match ≥ 3, stop the literal here.
            let peek_val = input.get(lit_end).copied().unwrap_or(0);
            let mut peek_run = 1usize;
            while lit_end + peek_run < input.len()
                && input.get(lit_end + peek_run).copied() == Some(peek_val)
                && peek_run < MIN_MATCH_LEN
            {
                peek_run += 1;
            }
            if peek_run >= MIN_MATCH_LEN {
                break;
            }
            let peek_match = find_best_match(input, lit_end);
            if peek_match.len >= MIN_MATCH_LEN {
                break;
            }
            lit_end += 1;
        }
        // Emit medium literal: 0x80 | count, followed by `count` raw bytes.
        let count = lit_end - lit_start;
        out.push(0x80 | (count as u8));
        if let Some(slice) = input.get(lit_start..lit_end) {
            out.extend_from_slice(slice);
        }
        pos = lit_end;
    }

    // End-of-stream marker.
    out.push(0x80);
    out
}

/// A back-reference match result.
struct Match {
    /// Relative distance back from the current write position.
    offset: usize,
    /// Number of matching bytes (0 if no match found).
    len: usize,
}

/// Searches for the best (longest) back-reference match within the
/// relative-copy window.
///
/// Returns the match offset and length.  If no match of at least
/// `MIN_MATCH_LEN` is found, returns len=0.
fn find_best_match(input: &[u8], pos: usize) -> Match {
    let mut best = Match { offset: 0, len: 0 };
    let window_start = pos.saturating_sub(MAX_REL_OFFSET);
    let remaining = input.len() - pos;
    let max_len = remaining.min(MAX_REL_COUNT);

    if max_len < MIN_MATCH_LEN {
        return best;
    }

    let mut candidate = if pos > 0 { pos - 1 } else { return best };
    loop {
        // Compare bytes at candidate vs pos.
        let mut match_len = 0;
        while match_len < max_len && input.get(candidate + match_len) == input.get(pos + match_len)
        {
            match_len += 1;
        }
        if match_len > best.len {
            best.len = match_len;
            best.offset = pos - candidate;
            if match_len == max_len {
                break; // Can't do better.
            }
        }
        if candidate == window_start {
            break;
        }
        candidate -= 1;
    }
    best
}

#[cfg(test)]
mod tests;
