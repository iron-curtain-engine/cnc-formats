// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! SHP sprite parser (`.shp`).
//!
//! SHP files store one or more palette-indexed sprite frames.  This module
//! implements the **keyframe animation** variant used by Red Alert unit and
//! building sprites (`2KEYFRAM.CPP` / `KEYFRAME.CPP`).
//!
//! ## File Layout
//!
//! ```text
//! [KeyFrameHeader]              14 bytes  (7 × u16)
//! [frame offsets]               (frame_count + 2) × 8 bytes
//! [optional palette]            768 bytes  (present when flags & 0x0001)
//! [frame data ...]              LCW-compressed or XOR-delta pixel data
//! ```
//!
//! Each offset-table entry is 8 bytes: a `u32` whose high byte encodes the
//! [`ShpFrameFormat`] and whose low 24 bits give the absolute file offset,
//! followed by a `u16` reference offset and a `u16` reference format code.
//!
//! The two extra entries beyond `frame_count` are an EOF sentinel (file offset
//! = end of data) and a zero-padding entry, matching the layout written by
//! the original game's `Build_Frame` and OpenRA's `ShpTDSprite.Write`.
//!
//! ## Frame Encoding
//!
//! Three encoding formats are supported, identified by the high byte of
//! each offset-table entry (see `common/keyframe.h` `KeyFrameType` enum):
//!
//! - `0x80` (`KF_KEYFRAME` / LCW) — standalone keyframe, LCW-compressed.
//! - `0x40` (`KF_KEYDELTA` / XOR+LCW) — XOR-delta applied to a remote keyframe.
//! - `0x20` (`KF_DELTA` / XOR+Prev) — XOR-delta applied to the previous frame.
//!
//! ## References
//!
//! Implemented from community documentation (XCC Utilities, C&C Modding
//! Wiki) and binary analysis of game files.  Cross-reference: the original
//! game defines the header in `SHAPE.H` / `2KEYFRAM.CPP`.

use crate::error::Error;
use crate::lcw;
use crate::read::{read_u16_le, read_u32_le};

mod encode;
#[cfg(all(test, feature = "convert"))]
pub(crate) use encode::build_test_shp_helper;
pub use encode::encode_frames;

// V38 safety note: the frame_count field is u16 (max 65535), which inherently
// satisfies a reasonable bound.  No runtime cap constant is needed because
// the offset-table allocation (frame_count × 8 bytes, max ~512 KB) is small
// enough that a malicious header cannot cause a problematic allocation.

/// Number of extra offset-table entries beyond the frame count.
///
/// The offset table has `frame_count + 2` entries: one per frame, plus an
/// EOF sentinel (file offset = end of data) and a zero-padding entry.
/// This matches the layout written by the original game and by OpenRA's
/// `ShpTDSprite.Write`.
const EXTRA_OFFSET_ENTRIES: usize = 2;

/// Size in bytes of each offset-table entry: `u32` (format | offset) +
/// `u16` (ref_offset) + `u16` (ref_format).
const OFFSET_ENTRY_SIZE: usize = 8;

/// Mask for extracting the 24-bit file offset from the first `u32` of an
/// offset-table entry.  The high byte carries the [`ShpFrameFormat`] code.
const OFFSET_MASK: u32 = 0x00FF_FFFF;

/// V38 cap: maximum number of frames in one SHP file.
const MAX_FRAME_COUNT: usize = 8192;

/// V38 cap: maximum pixel area of one SHP frame.
const MAX_FRAME_AREA: usize = 4 * 1024 * 1024;

// ─── Header ──────────────────────────────────────────────────────────────────

/// The 14-byte keyframe animation header at the start of every SHP file.
///
/// Layout matches the original game's `KeyFrameHeaderType` (14 bytes, LE
/// fields) from `common/keyframe.cpp` in the Vanilla-Conquer source.
///
/// ```text
/// Offset  Type    Field
/// 0       u16     frames
/// 2       u16     x
/// 4       u16     y
/// 6       u16     width
/// 8       u16     height
/// 10      u16     largest_frame_size
/// 12      i16     flags        (stored as u16, treated as bitmask)
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShpHeader {
    /// Number of animation frames.
    pub frame_count: u16,
    /// X display offset (usually 0; repurposed at runtime by the game engine
    /// for uncompressed-shape-cache bookkeeping).
    pub x: u16,
    /// Y display offset (usually 0; repurposed at runtime like `x`).
    pub y: u16,
    /// Frame width in pixels.
    pub width: u16,
    /// Frame height in pixels.
    pub height: u16,
    /// Largest single frame size in bytes (used for buffer allocation).
    pub largest_frame_size: u16,
    /// Format flags.  Bit 0 (`0x0001`) indicates an embedded 768-byte palette
    /// that precedes frame pixel data.
    pub flags: u16,
}

impl ShpHeader {
    /// Returns `true` if this SHP file contains an embedded palette.
    #[inline]
    pub fn has_embedded_palette(&self) -> bool {
        self.flags & 0x0001 != 0
    }
}

// ─── Frame Format ────────────────────────────────────────────────────────────

/// Encoding format of a single SHP frame, stored in the high byte of the
/// offset-table entry's first `u32`.
///
/// Values correspond to the `KeyFrameType` enum in `common/keyframe.h`:
/// `KF_KEYFRAME = 0x80`, `KF_KEYDELTA = 0x40`, `KF_DELTA = 0x20`.
///
/// OpenRA names these `LCW`, `XORLCW`, and `XORPrev` respectively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ShpFrameFormat {
    /// `0x80` — standalone keyframe, LCW-compressed.
    ///
    /// Decompress directly with [`crate::lcw::decompress`] to obtain the
    /// full `width × height` pixel buffer.
    Lcw = 0x80,
    /// `0x40` — key-delta: XOR-delta applied to a remote keyframe.
    ///
    /// First decompress the reference frame (identified by `ref_offset`),
    /// then apply the XOR-delta stream at this frame's file offset.
    XorLcw = 0x40,
    /// `0x20` — inter-frame delta: XOR-delta applied to the previous frame.
    ///
    /// Decode the previous frame first, then apply this frame's XOR-delta
    /// stream on top.
    XorPrev = 0x20,
}

// ─── Frame ───────────────────────────────────────────────────────────────────

/// A single frame extracted from an SHP file.
///
/// Borrows frame data from the input slice (zero-copy parse).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShpFrame<'input> {
    /// Raw bytes for this frame as stored on disk.
    ///
    /// For [`ShpFrameFormat::Lcw`] frames, pass to [`crate::lcw::decompress`].
    /// For XOR-delta frames ([`ShpFrameFormat::XorLcw`] and
    /// [`ShpFrameFormat::XorPrev`]), the bytes are a **raw** (uncompressed)
    /// XOR mask applied to a decoded reference frame — no LCW decompression.
    ///
    /// This is a borrow into the original input slice — no heap copy is made
    /// during parsing.
    pub data: &'input [u8],
    /// Encoding format of this frame.
    pub format: ShpFrameFormat,
    /// Absolute byte offset of this frame's data within the source file.
    ///
    /// Used by [`ShpFile::decode_frames`] to look up the reference keyframe
    /// for [`ShpFrameFormat::XorLcw`] frames (matched against `ref_offset`).
    pub file_offset: u32,
    /// For [`ShpFrameFormat::XorLcw`]: the file offset of the reference
    /// keyframe whose decoded pixels serve as the base for the XOR delta.
    /// For other formats this value is not meaningful.
    pub ref_offset: u16,
    /// Format code stored for the reference frame (high 16 bits of the
    /// second `u32` in the offset-table entry).  Informational; the actual
    /// reference frame format is determined by its own offset-table entry.
    pub ref_format: u16,
}

impl ShpFrame<'_> {
    /// Returns the pixel data for an LCW keyframe.
    ///
    /// This method only handles [`ShpFrameFormat::Lcw`] frames.  For
    /// XOR-delta frames, use [`ShpFile::decode_frames`] which resolves
    /// cross-frame references during sequential decode.
    ///
    /// The `expected_size` should be `header.width as usize * header.height as usize`.
    /// It is passed to [`lcw::decompress`] as the output-size cap (V38).
    ///
    /// # Errors
    ///
    /// - Returns [`Error::DecompressionError`] if the frame is not an LCW
    ///   keyframe (XOR-delta frames need the full `ShpFile` context).
    /// - Forwards [`crate::lcw::decompress`] errors for corrupt LCW data.
    pub fn pixels(&self, expected_size: usize) -> Result<Vec<u8>, Error> {
        if self.format != ShpFrameFormat::Lcw {
            return Err(Error::DecompressionError {
                reason: "XOR-delta frames require ShpFile::decode_frames",
            });
        }
        lcw::decompress(self.data, expected_size)
    }
}

// ─── ShpFile ─────────────────────────────────────────────────────────────────

/// A parsed SHP sprite file.
///
/// Borrows all variable-size data (frame bytes, embedded palette) from the
/// input slice.  Parsing allocates only the `Vec` of frame descriptors and
/// the offset table — the pixel data itself is zero-copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShpFile<'input> {
    /// File header with frame dimensions and flags.
    pub header: ShpHeader,
    /// Optional embedded palette (768 bytes of 6-bit VGA RGB data).
    /// Borrows directly from the input slice.
    pub embedded_palette: Option<&'input [u8]>,
    /// All animation frames, in order.
    pub frames: Vec<ShpFrame<'input>>,
}

impl<'input> ShpFile<'input> {
    /// Parses an SHP file from a byte slice.
    ///
    /// ## Offset Table Layout
    ///
    /// After the 14-byte header, the file contains `(frame_count + 2)`
    /// offset-table entries, each 8 bytes:
    ///
    /// ```text
    /// u32  format_and_offset   high byte = ShpFrameFormat code,
    ///                          low 24 bits = file byte offset
    /// u16  ref_offset          reference frame file offset (for XOR delta)
    /// u16  ref_format          format code of reference frame
    /// ```
    ///
    /// The extra two entries are an EOF sentinel (whose file offset equals
    /// the total file length) and a zero-padding entry.
    ///
    /// ## Palette
    ///
    /// When `flags & 0x0001`, an embedded 768-byte VGA palette sits between
    /// the offset table and the first frame's pixel data.  The original game
    /// skips 768 bytes past the first keyframe's file offset to reach the
    /// actual LCW data.
    ///
    /// # Errors
    ///
    /// - [`Error::UnexpectedEof`] — data too short for header or offset table.
    /// - [`Error::InvalidOffset`] — a frame offset points outside the file.
    /// - [`Error::InvalidMagic`]  — an offset entry has an unrecognised
    ///   format code in its high byte.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        // ── Header (14 bytes = 7 × u16) ───────────────────────────────────
        // Fields are read as raw little-endian u16.  No field is rejected
        // at this stage — validation happens when offsets are resolved.
        if data.len() < 14 {
            return Err(Error::UnexpectedEof {
                needed: 14,
                available: data.len(),
            });
        }
        // Safe reads via helpers (defense-in-depth over the upfront check).
        let frame_count = read_u16_le(data, 0)? as usize;
        let x = read_u16_le(data, 2)?;
        let y = read_u16_le(data, 4)?;
        let width = read_u16_le(data, 6)?;
        let height = read_u16_le(data, 8)?;
        let largest_frame_size = read_u16_le(data, 10)?;
        let flags = read_u16_le(data, 12)?;

        if frame_count > MAX_FRAME_COUNT {
            return Err(Error::InvalidSize {
                value: frame_count,
                limit: MAX_FRAME_COUNT,
                context: "SHP frame count",
            });
        }

        let frame_area = (width as usize).saturating_mul(height as usize);
        if frame_area > MAX_FRAME_AREA {
            return Err(Error::InvalidSize {
                value: frame_area,
                limit: MAX_FRAME_AREA,
                context: "SHP frame area",
            });
        }

        let header = ShpHeader {
            frame_count: frame_count as u16,
            x,
            y,
            width,
            height,
            largest_frame_size,
            flags,
        };

        // ── Offset table: (frame_count + 2) × 8 bytes ─────────────────
        // Each entry is 8 bytes: u32 (format|offset) + u16 ref_offset +
        // u16 ref_format.  The two extra entries are EOF + zero-padding.
        let total_entries = frame_count.saturating_add(EXTRA_OFFSET_ENTRIES);
        let offset_table_bytes = total_entries.saturating_mul(OFFSET_ENTRY_SIZE);
        let offset_table_start = 14usize;
        let offset_table_end = offset_table_start.saturating_add(offset_table_bytes);
        if offset_table_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: offset_table_end,
                available: data.len(),
            });
        }

        // Read all offset entries.
        struct OffsetEntry {
            file_offset: u32,
            format_byte: u8,
            ref_offset: u16,
            ref_format: u16,
        }
        let mut entries = Vec::with_capacity(total_entries);
        for i in 0..total_entries {
            let base = offset_table_start.saturating_add(i.saturating_mul(OFFSET_ENTRY_SIZE));
            let raw = read_u32_le(data, base)?;
            let ref_off = read_u16_le(data, base.saturating_add(4))?;
            let ref_fmt = read_u16_le(data, base.saturating_add(6))?;
            entries.push(OffsetEntry {
                file_offset: raw & OFFSET_MASK,
                format_byte: (raw >> 24) as u8,
                ref_offset: ref_off,
                ref_format: ref_fmt,
            });
        }

        // ── Optional embedded palette ──────────────────────────────────
        // When flags bit 0 is set, a 768-byte palette (256 × 3 RGB, 6-bit
        // VGA) sits after the offset table.  The frame offset-table entries
        // already account for the palette: the first frame's file offset
        // points past the 768 palette bytes to the actual compressed data.
        // We simply extract the palette here for callers who need it.
        let has_palette = flags & 0x0001 != 0;
        let palette_start = offset_table_end;
        let palette_end = if has_palette {
            palette_start.saturating_add(768)
        } else {
            palette_start
        };
        if palette_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: palette_end,
                available: data.len(),
            });
        }
        let embedded_palette = if has_palette {
            Some(
                data.get(palette_start..palette_end)
                    .ok_or(Error::UnexpectedEof {
                        needed: palette_end,
                        available: data.len(),
                    })?,
            )
        } else {
            None
        };

        let eof_entry = entries.get(frame_count).ok_or(Error::InvalidOffset {
            offset: frame_count,
            bound: entries.len(),
        })?;
        // The sentinel's ref_offset and ref_format carry no meaning, and
        // original Westwood tools wrote non-zero garbage into any field
        // of this slot on some RA1 assets (e.g. MOUSE.SHP, EDMOUSE.SHP).
        // Only reject if format_byte is a valid frame code (0x20/0x40/
        // 0x80), which would genuinely indicate a mis-parsed header
        // (we miscounted frames and the sentinel is actually a real
        // frame).  Any other non-zero value is benign garbage.
        let eof_fmt = eof_entry.format_byte;
        if eof_fmt == ShpFrameFormat::Lcw as u8
            || eof_fmt == ShpFrameFormat::XorLcw as u8
            || eof_fmt == ShpFrameFormat::XorPrev as u8
        {
            return Err(Error::InvalidMagic {
                context: "SHP EOF sentinel has frame format code",
            });
        }
        let eof_offset = eof_entry.file_offset as usize;
        if eof_offset < palette_end || eof_offset > data.len() {
            return Err(Error::InvalidOffset {
                offset: eof_offset,
                bound: data.len(),
            });
        }

        let padding_entry = entries.get(frame_count + 1).ok_or(Error::InvalidOffset {
            offset: frame_count + 1,
            bound: entries.len(),
        })?;
        // The padding entry's fields carry no meaningful data.  Some real
        // Westwood-authored RA1 files have non-zero garbage in any field of
        // this slot, so we only reject if the format_byte looks like a valid
        // frame code (0x20 / 0x40 / 0x80), which would indicate a mis-parsed
        // header rather than benign garbage.
        let pad_fmt = padding_entry.format_byte;
        if pad_fmt == ShpFrameFormat::Lcw as u8
            || pad_fmt == ShpFrameFormat::XorLcw as u8
            || pad_fmt == ShpFrameFormat::XorPrev as u8
        {
            return Err(Error::InvalidMagic {
                context: "SHP zero-padding entry has frame format code",
            });
        }

        // ── Frame data ─────────────────────────────────────────────────
        // File offsets in the offset table are absolute from the start of
        // the file.  When an embedded palette is present, the offset table
        // entries already account for the 768-byte palette — the first
        // frame's file offset points past the palette to the actual LCW
        // data.  We use consecutive file offsets to determine each frame's
        // byte range: frame[i] spans from entries[i].file_offset to
        // entries[i+1].file_offset.  The EOF entry (index frame_count)
        // provides the endpoint for the last frame.
        let mut frames = Vec::with_capacity(frame_count);
        for i in 0..frame_count {
            let entry = entries.get(i).ok_or(Error::InvalidOffset {
                offset: i,
                bound: entries.len(),
            })?;
            let next = entries.get(i + 1).ok_or(Error::InvalidOffset {
                offset: i + 1,
                bound: entries.len(),
            })?;

            // Decode the format code from the high byte.
            let format = match entry.format_byte {
                0x80 => ShpFrameFormat::Lcw,
                0x40 => ShpFrameFormat::XorLcw,
                0x20 => ShpFrameFormat::XorPrev,
                _ => {
                    return Err(Error::InvalidMagic {
                        context: "SHP frame format code",
                    })
                }
            };

            let start = entry.file_offset as usize;
            let end = (next.file_offset & OFFSET_MASK) as usize;

            // Validate structural integrity.
            if start < palette_end || end < palette_end || start > end || end > data.len() {
                return Err(Error::InvalidOffset {
                    offset: end,
                    bound: data.len(),
                });
            }
            if i == 0 && format != ShpFrameFormat::Lcw {
                return Err(Error::InvalidMagic {
                    context: "SHP first frame format",
                });
            }
            let frame_data = data.get(start..end).ok_or(Error::InvalidOffset {
                offset: end,
                bound: data.len(),
            })?;

            frames.push(ShpFrame {
                data: frame_data,
                format,
                file_offset: entry.file_offset,
                ref_offset: entry.ref_offset,
                ref_format: entry.ref_format,
            });
        }

        Ok(ShpFile {
            header,
            embedded_palette,
            frames,
        })
    }

    /// Returns the number of animation frames.
    #[inline]
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Returns the pixel area of a single frame (width × height).
    #[inline]
    pub fn frame_pixel_count(&self) -> usize {
        self.header.width as usize * self.header.height as usize
    }

    /// Decodes all frames, resolving XOR-delta references.
    ///
    /// Returns one `Vec<u8>` per frame, each containing `width × height`
    /// palette-indexed pixels.
    ///
    /// ## Encoding formats
    ///
    /// - **Lcw (0x80) / `KF_KEYFRAME`:** LCW-decompress the data to produce a
    ///   standalone pixel buffer.
    /// - **XorLcw (0x40) / `KF_KEYDELTA`:** Apply a **Format40-encoded**
    ///   XOR-delta stream against the decoded reference keyframe (identified by
    ///   [`ShpFrame::ref_offset`]).  The delta is a command stream, not raw
    ///   bytes — see `apply_xor_delta` for the command encoding.
    /// - **XorPrev (0x20) / `KF_DELTA`:** Apply a **Format40-encoded**
    ///   XOR-delta stream against the immediately preceding decoded frame.
    ///
    /// # Errors
    ///
    /// - [`Error::DecompressionError`] if the first frame is not an LCW
    ///   keyframe, if a reference frame cannot be found, if LCW
    ///   decompression of a keyframe fails, or if a Format40 delta stream
    ///   is malformed.
    pub fn decode_frames(&self) -> Result<Vec<Vec<u8>>, Error> {
        let pixel_count = self.frame_pixel_count();

        // Degenerate SHP files (e.g. SIDEBAR.SHP from RA1's HIRES.MIX) carry
        // width=0 / height=0 in the header, making pixel_count=0.  There is
        // nothing useful to decompress; return one empty buffer per frame so
        // callers that iterate frames do not crash.
        if pixel_count == 0 {
            return Ok(vec![vec![]; self.frames.len()]);
        }

        let mut decoded: Vec<Vec<u8>> = Vec::with_capacity(self.frames.len());
        // Tracks which decoded-frame index corresponds to each keyframe's file
        // offset, so XorLcw frames can find their reference by `ref_offset`.
        let mut keyframe_by_offset: std::collections::HashMap<u32, usize> =
            std::collections::HashMap::new();

        for (i, frame) in self.frames.iter().enumerate() {
            match frame.format {
                ShpFrameFormat::Lcw => {
                    // KF_KEYFRAME (0x80): LCW-compressed standalone frame.
                    let pixels = lcw::decompress(frame.data, pixel_count)?;
                    keyframe_by_offset.insert(frame.file_offset, i);
                    decoded.push(pixels);
                }
                ShpFrameFormat::XorLcw => {
                    // KF_KEYDELTA (0x40): Format40 XOR-delta against a reference keyframe.
                    // Look up the decoded reference frame by file_offset
                    // (matching frame.ref_offset).
                    if i == 0 {
                        return Err(Error::DecompressionError {
                            reason: "first frame must be an LCW keyframe, not XOR-delta",
                        });
                    }
                    let ref_idx = keyframe_by_offset
                        .get(&(frame.ref_offset as u32))
                        .copied()
                        .unwrap_or(i - 1);
                    let base = decoded.get(ref_idx).ok_or(Error::DecompressionError {
                        reason: "missing reference keyframe for XorLcw delta",
                    })?;
                    let mut pixels = base.clone();
                    apply_xor_delta(&mut pixels, frame.data)?;
                    decoded.push(pixels);
                }
                ShpFrameFormat::XorPrev => {
                    // KF_DELTA (0x20): Format40 XOR-delta against the previous decoded frame.
                    if i == 0 {
                        return Err(Error::DecompressionError {
                            reason: "first frame must be an LCW keyframe, not XOR-delta",
                        });
                    }
                    let prev = decoded.get(i - 1).ok_or(Error::DecompressionError {
                        reason: "missing previous frame for XorPrev delta",
                    })?;
                    let mut pixels = prev.clone();
                    apply_xor_delta(&mut pixels, frame.data)?;
                    decoded.push(pixels);
                }
            }
        }

        Ok(decoded)
    }
}

/// Apply a Format40-encoded XOR-delta stream to `dest`.
///
/// Delegates to [`crate::xor_delta::apply_xor_delta`].  See that module for
/// the full command table and documentation.
fn apply_xor_delta(dest: &mut [u8], delta: &[u8]) -> Result<(), Error> {
    crate::xor_delta::apply_xor_delta(dest, delta)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_validation;
