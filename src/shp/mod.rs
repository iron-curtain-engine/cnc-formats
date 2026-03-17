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
pub struct ShpFrame<'a> {
    /// Raw bytes for this frame as stored on disk.
    ///
    /// For [`ShpFrameFormat::Lcw`] frames, pass to [`crate::lcw::decompress`].
    /// For XOR-delta frames ([`ShpFrameFormat::XorLcw`] and
    /// [`ShpFrameFormat::XorPrev`]), the bytes are an XOR-delta stream that
    /// must be applied to a decoded reference frame.
    ///
    /// This is a borrow into the original input slice — no heap copy is made
    /// during parsing.
    pub data: &'a [u8],
    /// Encoding format of this frame.
    pub format: ShpFrameFormat,
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
pub struct ShpFile<'a> {
    /// File header with frame dimensions and flags.
    pub header: ShpHeader,
    /// Optional embedded palette (768 bytes of 6-bit VGA RGB data).
    /// Borrows directly from the input slice.
    pub embedded_palette: Option<&'a [u8]>,
    /// All animation frames, in order.
    pub frames: Vec<ShpFrame<'a>>,
}

impl<'a> ShpFile<'a> {
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
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
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
        if eof_entry.format_byte != 0 || eof_entry.ref_offset != 0 || eof_entry.ref_format != 0 {
            return Err(Error::InvalidMagic {
                context: "SHP EOF sentinel",
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
        if padding_entry.file_offset != 0
            || padding_entry.format_byte != 0
            || padding_entry.ref_offset != 0
            || padding_entry.ref_format != 0
        {
            return Err(Error::InvalidMagic {
                context: "SHP zero-padding entry",
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
    /// palette-indexed pixels.  Frames are decoded sequentially:
    ///
    /// - **Lcw (0x80):** LCW-decompress to produce standalone pixel buffer.
    /// - **XorPrev (0x20):** LCW-decompress the delta, then XOR with the
    ///   previous frame's decoded pixels.
    /// - **XorLcw (0x40):** LCW-decompress the delta, then XOR with the
    ///   previous frame's decoded pixels (same as XorPrev in practice —
    ///   both reference the immediately preceding frame in sequential decode).
    ///
    /// ## Why sequential decode
    ///
    /// Real-world C&C SHP files use sequential delta chains: frame 0 is a
    /// keyframe, subsequent frames are XOR-deltas against the previous frame.
    /// The `ref_offset` field in the offset table could theoretically point
    /// to an arbitrary earlier frame, but no known game file does this.
    /// Sequential decode handles all known game assets correctly.
    ///
    /// # Errors
    ///
    /// - [`Error::DecompressionError`] if the first frame is not an LCW
    ///   keyframe, or if LCW decompression fails for any frame.
    pub fn decode_frames(&self) -> Result<Vec<Vec<u8>>, Error> {
        let pixel_count = self.frame_pixel_count();
        let mut decoded = Vec::with_capacity(self.frames.len());

        for (i, frame) in self.frames.iter().enumerate() {
            match frame.format {
                ShpFrameFormat::Lcw => {
                    // Standalone keyframe: decompress directly.
                    let pixels = lcw::decompress(frame.data, pixel_count)?;
                    decoded.push(pixels);
                }
                ShpFrameFormat::XorLcw | ShpFrameFormat::XorPrev => {
                    // XOR-delta: decompress the delta, then XOR with previous.
                    if i == 0 {
                        return Err(Error::DecompressionError {
                            reason: "first frame must be an LCW keyframe, not XOR-delta",
                        });
                    }
                    let delta = lcw::decompress(frame.data, pixel_count)?;
                    let prev = decoded.get(i - 1).ok_or(Error::DecompressionError {
                        reason: "missing previous frame for XOR-delta",
                    })?;
                    // XOR the delta with the previous frame to produce current.
                    let mut pixels = prev.clone();
                    for (dst, src) in pixels.iter_mut().zip(delta.iter()) {
                        *dst ^= *src;
                    }
                    decoded.push(pixels);
                }
            }
        }

        Ok(decoded)
    }
}

// ── SHP Encoder ──────────────────────────────────────────────────────────────
//
// Builds a valid SHP binary from palette-indexed pixel frames.  All frames
// are encoded as LCW keyframes (format 0x80) for simplicity and maximum
// compatibility.  XOR-delta encoding would reduce file size but is not
// required for correctness, and LCW-only files are accepted by all known
// C&C tools and engines.
//
// This is a clean-room implementation based on the publicly documented SHP
// layout (see module-level docs and binary-codecs.md).

/// Encodes palette-indexed pixel frames into a complete SHP file.
///
/// Each frame in `frames` must be exactly `width × height` bytes of
/// palette-indexed pixel data.  All frames are encoded as LCW keyframes
/// (`ShpFrameFormat::Lcw`, format code 0x80).
///
/// Returns the complete SHP file as `Vec<u8>` that [`ShpFile::parse`] can
/// round-trip.
///
/// # Errors
///
/// Returns [`Error::InvalidSize`] if any frame has the wrong number of pixels.
pub fn encode_frames(frames: &[&[u8]], width: u16, height: u16) -> Result<Vec<u8>, Error> {
    let pixel_count = (width as usize).saturating_mul(height as usize);
    for (i, frame) in frames.iter().enumerate() {
        if frame.len() != pixel_count {
            return Err(Error::InvalidSize {
                value: frame.len(),
                limit: pixel_count,
                context: "SHP frame pixel count mismatch",
            });
        }
        let _ = i; // suppress unused warning; index kept for future diagnostics.
    }

    let frame_count = frames.len() as u16;
    let total_entries = (frame_count as usize).saturating_add(EXTRA_OFFSET_ENTRIES);
    let offset_table_size = total_entries.saturating_mul(OFFSET_ENTRY_SIZE);
    let header_size = 14usize;

    // LCW-compress each frame.
    let compressed: Vec<Vec<u8>> = frames.iter().map(|f| lcw::compress(f)).collect();

    let largest = compressed.iter().map(|c| c.len()).max().unwrap_or(0);
    let data_start = header_size.saturating_add(offset_table_size);

    // Build the file.
    let total_data: usize = compressed.iter().map(|c| c.len()).sum();
    let mut out = Vec::with_capacity(data_start.saturating_add(total_data));

    // ── Header (14 bytes) ────────────────────────────────────────────
    out.extend_from_slice(&frame_count.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // x
    out.extend_from_slice(&0u16.to_le_bytes()); // y
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&(largest as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // flags (no embedded palette)

    // ── Offset table ─────────────────────────────────────────────────
    // Each entry: u32 (format_byte << 24 | file_offset), u16 ref_offset, u16 ref_format.
    let mut file_offset = data_start as u32;
    for c in &compressed {
        let raw = ((ShpFrameFormat::Lcw as u32) << 24) | (file_offset & OFFSET_MASK);
        out.extend_from_slice(&raw.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // ref_offset
        out.extend_from_slice(&0u16.to_le_bytes()); // ref_format
        file_offset = file_offset.saturating_add(c.len() as u32);
    }
    // EOF sentinel: offset = end of all frame data.
    let eof_raw = file_offset & OFFSET_MASK;
    out.extend_from_slice(&eof_raw.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    // Zero-padding entry.
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());

    // ── Frame data ───────────────────────────────────────────────────
    for c in &compressed {
        out.extend_from_slice(c);
    }

    Ok(out)
}

/// Builds a minimal SHP binary for cross-module testing.
///
/// Creates a `width × height` SHP with one LCW keyframe that fills all
/// pixels with `fill_value`.
#[cfg(all(test, feature = "convert"))]
pub(crate) fn build_test_shp_helper(width: u16, height: u16, fill_value: u8) -> Vec<u8> {
    let pixel_count = (width as usize) * (height as usize);
    // LCW fill command: 0xFE, count_lo, count_hi, value, 0x80 (end).
    let lcw = [
        0xFEu8,
        pixel_count as u8,
        (pixel_count >> 8) as u8,
        fill_value,
        0x80,
    ];
    let frame_count: u16 = 1;
    let total_entries = frame_count as usize + EXTRA_OFFSET_ENTRIES;
    let offset_table_size = total_entries * OFFSET_ENTRY_SIZE;
    let data_start = (14 + offset_table_size) as u32;

    let mut out = Vec::new();
    let push_u16 = |v: u16, buf: &mut Vec<u8>| buf.extend_from_slice(&v.to_le_bytes());
    push_u16(frame_count, &mut out);
    push_u16(0, &mut out);
    push_u16(0, &mut out);
    push_u16(width, &mut out);
    push_u16(height, &mut out);
    push_u16(lcw.len() as u16, &mut out);
    push_u16(0, &mut out);

    // Offset entry for frame 0.
    let raw = ((ShpFrameFormat::Lcw as u32) << 24) | (data_start & OFFSET_MASK);
    out.extend_from_slice(&raw.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    // EOF sentinel.
    let eof = (data_start + lcw.len() as u32) & OFFSET_MASK;
    out.extend_from_slice(&eof.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    // Zero padding.
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());

    out.extend_from_slice(&lcw);
    out
}

#[cfg(test)]
mod tests;
