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
//! [frame offsets]               (frame_count + 1) × u32
//! [optional palette]            768 bytes  (present when flags & 0x0001)
//! [frame data ...]              raw or LCW-compressed pixel data
//! ```
//!
//! Frame `i` occupies bytes `offsets[i]..offsets[i+1]` within the file.
//! The final extra offset (`offsets[frame_count]`) acts as a sentinel
//! pointing one byte past the last frame's data.
//!
//! ## Compression
//!
//! Frame data may be stored uncompressed or LCW-compressed.  The high bit
//! (`0x8000_0000`) of each offset entry signals the frame type:
//! - High bit **clear** → frame data is LCW-compressed (call [`crate::lcw`]).
//! - High bit **set**   → frame data is raw/uncompressed.
//!
//! Callers decompress on demand; this parser exposes the raw bytes and the
//! compression flag via [`ShpFrame`].
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
// the offset-table allocation (frame_count × 4 bytes, max ~256 KB) is small
// enough that a malicious header cannot cause a problematic allocation.

/// Bitmask in the raw offset table entry signalling uncompressed frame data.
///
/// When set, the frame's bytes are raw palette-indexed pixels; when clear,
/// the bytes are LCW-compressed and must be decompressed before use.
const OFFSET_UNCOMPRESSED_FLAG: u32 = 0x8000_0000;

// ─── Header ──────────────────────────────────────────────────────────────────

/// The 14-byte keyframe animation header at the start of every SHP file.
///
/// Layout matches the original game's `KeyFrameHeaderType` (14 bytes, LE fields).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShpHeader {
    /// Number of animation frames.
    pub frame_count: u16,
    /// X display offset (usually 0).
    pub x: u16,
    /// Y display offset (usually 0).
    pub y: u16,
    /// Frame width in pixels.
    pub width: u16,
    /// Frame height in pixels.
    pub height: u16,
    /// Largest single frame size in bytes (used for buffer allocation).
    pub largest_frame_size: u16,
    /// Format flags.  Bit 0 (`0x0001`) indicates an embedded 768-byte palette
    /// immediately following the offset table.
    pub flags: u16,
}

impl ShpHeader {
    /// Returns `true` if this SHP file contains an embedded palette.
    #[inline]
    pub fn has_embedded_palette(&self) -> bool {
        self.flags & 0x0001 != 0
    }
}

// ─── Frame ───────────────────────────────────────────────────────────────────

/// A single frame extracted from an SHP file.
///
/// Borrows frame data from the input slice (zero-copy parse).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShpFrame<'a> {
    /// Raw bytes for this frame as stored on disk.
    ///
    /// When `is_compressed` is `true`, call [`lcw::decompress`] to obtain
    /// the palette-indexed pixel data (width × height bytes).
    ///
    /// This is a borrow into the original input slice — no heap copy is made
    /// during parsing.
    pub data: &'a [u8],
    /// `false` = LCW-compressed; `true` = raw pixel data.
    pub is_uncompressed: bool,
}

impl ShpFrame<'_> {
    /// Returns the pixel data for this frame.
    ///
    /// - If `is_uncompressed` is `true`, returns a copy of `data` (raw pixels).
    /// - If `is_uncompressed` is `false`, decompresses the data using LCW.
    ///
    /// The `expected_size` should be `header.width as usize * header.height as usize`.
    /// It is passed to [`lcw::decompress`] as the output-size cap (V38).
    ///
    /// # Errors
    ///
    /// Forwards [`crate::lcw::decompress`] errors for compressed frames.
    pub fn pixels(&self, expected_size: usize) -> Result<Vec<u8>, Error> {
        if self.is_uncompressed {
            Ok(self.data.to_vec())
        } else {
            lcw::decompress(self.data, expected_size)
        }
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
    /// # Errors
    ///
    /// - [`Error::UnexpectedEof`]  — data is too short for the header or offset table.
    /// - [`Error::InvalidOffset`]  — a frame offset points outside the file data.
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

        let header = ShpHeader {
            frame_count: frame_count as u16,
            x,
            y,
            width,
            height,
            largest_frame_size,
            flags,
        };

        // ── Offset table: (frame_count + 1) × u32 ─────────────────────
        // The extra entry is a sentinel pointing past the last frame.
        // Each raw offset carries OFFSET_UNCOMPRESSED_FLAG in its high bit.
        let offset_table_bytes = (frame_count + 1) * 4;
        let offset_table_start = 14usize;
        if offset_table_start + offset_table_bytes > data.len() {
            return Err(Error::UnexpectedEof {
                needed: offset_table_start + offset_table_bytes,
                available: data.len(),
            });
        }

        let mut raw_offsets = Vec::with_capacity(frame_count + 1);
        for i in 0..=frame_count {
            let pos = offset_table_start + i * 4;
            let raw = read_u32_le(data, pos)?;
            raw_offsets.push(raw);
        }

        // ── Optional embedded palette ──────────────────────────────────
        // When flags bit 0 is set, a 768-byte palette (256 × 3 RGB, 6-bit VGA)
        // appears between the offset table and the frame data.  This palette
        // can be fed to `pal::Palette::parse()` for colour lookup.
        let has_palette = flags & 0x0001 != 0;
        let palette_start = offset_table_start + offset_table_bytes;
        let palette_end = if has_palette {
            palette_start + 768
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

        // ── Frame data ─────────────────────────────────────────────────────
        // Offsets are absolute (relative to the start of the whole file).
        // The high bit flags compression: set = uncompressed, clear = LCW.
        // We mask the flag off before slicing.
        let mut frames = Vec::with_capacity(frame_count);
        for i in 0..frame_count {
            let raw_start = raw_offsets[i];
            let raw_end = raw_offsets[i + 1];
            let is_uncompressed = (raw_start & OFFSET_UNCOMPRESSED_FLAG) != 0;
            let start = (raw_start & !OFFSET_UNCOMPRESSED_FLAG) as usize;
            let end = (raw_end & !OFFSET_UNCOMPRESSED_FLAG) as usize;

            // Validate structural integrity: start must not exceed end,
            // and end must not exceed the file length.
            if start > end || end > data.len() {
                return Err(Error::InvalidOffset {
                    offset: end,
                    bound: data.len(),
                });
            }
            let frame_data = data.get(start..end).ok_or(Error::InvalidOffset {
                offset: end,
                bound: data.len(),
            })?;
            frames.push(ShpFrame {
                data: frame_data,
                is_uncompressed,
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
}

// ─── Test helpers ─────────────────────────────────────────────────────────────

/// Builds a minimal SHP binary with the given raw (uncompressed) frame data.
///
/// Each frame in `frames` is stored with the uncompressed flag set in the
/// offset table entry.
#[cfg(test)]
pub(crate) fn build_shp(
    width: u16,
    height: u16,
    flags: u16,
    frames: &[&[u8]],
    embedded_palette: Option<&[u8]>,
) -> Vec<u8> {
    let frame_count = frames.len() as u16;
    let largest = frames.iter().map(|f| f.len()).max().unwrap_or(0) as u16;

    // Header
    let mut out = Vec::new();
    let push_u16 = |v: u16, buf: &mut Vec<u8>| buf.extend_from_slice(&v.to_le_bytes());
    push_u16(frame_count, &mut out);
    push_u16(0, &mut out); // x
    push_u16(0, &mut out); // y
    push_u16(width, &mut out);
    push_u16(height, &mut out);
    push_u16(largest, &mut out);
    push_u16(flags, &mut out);

    // Offset table start = header (14) + offset_table (frame_count+1)*4 + palette
    let offset_table_size = (frame_count as usize + 1) * 4;
    let palette_size = if flags & 0x0001 != 0 { 768 } else { 0 };
    let data_start = 14 + offset_table_size + palette_size;

    // Build offset table with OFFSET_UNCOMPRESSED_FLAG set on each.
    let mut cur = data_start as u32;
    for frame in frames {
        let raw = cur | OFFSET_UNCOMPRESSED_FLAG;
        out.extend_from_slice(&raw.to_le_bytes());
        cur += frame.len() as u32;
    }
    // Sentinel
    let sentinel = cur | OFFSET_UNCOMPRESSED_FLAG;
    out.extend_from_slice(&sentinel.to_le_bytes());

    // Optional palette
    if let Some(pal) = embedded_palette {
        out.extend_from_slice(pal);
    }

    // Frame data
    for frame in frames {
        out.extend_from_slice(frame);
    }

    out
}

#[cfg(test)]
mod tests;
