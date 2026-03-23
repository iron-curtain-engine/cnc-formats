// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Dune II SHP sprite parser (`.shp`).
//!
//! This module handles the **Dune II** SHP format, which is distinct from the
//! Tiberian Dawn / Red Alert 1 keyframe format in [`crate::shp`] and the
//! TS/RA2 scanline-RLE format in [`crate::shp_ts`].
//!
//! ## File Layout
//!
//! ```text
//! [num_frames]         u16 LE
//! [offset table]       (num_frames + 2) × u32 LE
//! [frame data ...]     per-frame headers + optional remap + pixel data
//! ```
//!
//! ## Per-Frame Header (10 bytes)
//!
//! ```text
//! Offset  Size  Field
//! 0       u16   flags          bit 0: has remap, bit 1: uncompressed, bit 2: custom size
//! 2       u8    slices         number of scan line slices (usually == height)
//! 3       u16   width          frame width in pixels
//! 5       u8    height         frame height in pixels
//! 6       u16   file_size      size of this frame's data block
//! 8       u16   data_size      size of the compressed/raw pixel payload
//! ```
//!
//! If bit 0 of flags is set, a 16-byte remap table follows the header before
//! pixel data.  Pixel data is LCW (Format80) compressed unless bit 1 is set.
//!
//! ## References
//!
//! Implemented from community documentation and binary analysis of Dune II
//! game files.

use crate::error::Error;
use crate::lcw;
use crate::read::{read_u16_le, read_u32_le, read_u8};

// ── Constants ────────────────────────────────────────────────────────────────

/// V38: maximum number of frames in one Dune II SHP file.
const MAX_FRAMES: usize = 4096;

/// V38: maximum frame dimension (width or height) in pixels.
const MAX_DIMENSION: usize = 1024;

/// Per-frame header size in bytes:
/// flags(2) + slices(1) + width(2) + height(1) + file_size(2) + data_size(2).
const FRAME_HEADER_SIZE: usize = 10;

/// Size of the optional remap table in bytes.
const REMAP_TABLE_SIZE: usize = 16;

/// Flag bit 0: a 16-byte remap table is present before pixel data.
const FLAG_HAS_REMAP: u16 = 0x0001;

/// Flag bit 1: pixel data is uncompressed (skip LCW decompression).
const FLAG_UNCOMPRESSED: u16 = 0x0002;

// ── Types ────────────────────────────────────────────────────────────────────

/// A single decoded frame from a Dune II SHP file.
#[derive(Debug, Clone)]
pub struct ShpD2Frame {
    /// Frame width in pixels.
    pub width: usize,
    /// Frame height in pixels.
    pub height: usize,
    /// Raw flag bits from the frame header.
    pub flags: u16,
    /// Decompressed palette-indexed pixel data (`width × height` bytes).
    pub pixels: Vec<u8>,
    /// Optional 16-byte remap table (present when flag bit 0 is set).
    pub remap: Option<[u8; 16]>,
}

/// A parsed Dune II SHP sprite file.
///
/// Unlike the TD/RA1 [`crate::shp::ShpFile`] which borrows frame data
/// zero-copy, this parser eagerly decompresses every frame because Dune II
/// SHP frames are small and the per-frame header must be read to determine
/// dimensions.
#[derive(Debug)]
pub struct ShpD2File {
    frames: Vec<ShpD2Frame>,
}

impl ShpD2File {
    /// Parses a Dune II SHP file from a byte slice.
    ///
    /// # Layout
    ///
    /// The file starts with a `u16` frame count, followed by
    /// `(num_frames + 2)` `u32` offset-table entries, then per-frame data.
    /// The two extra offset entries are end-of-data sentinels.
    ///
    /// # Errors
    ///
    /// - [`Error::UnexpectedEof`] — data too short for header, offset table,
    ///   or frame data.
    /// - [`Error::InvalidSize`] — frame count or dimensions exceed safety caps.
    /// - [`Error::InvalidOffset`] — a frame offset points outside the file.
    /// - [`Error::InvalidMagic`] — zero-dimension frame (structural error).
    /// - [`Error::DecompressionError`] — LCW decompression failure (forwarded
    ///   from [`crate::lcw::decompress`]).
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        // ── Header: num_frames (u16 LE at offset 0) ─────────────────────
        let num_frames = read_u16_le(data, 0)? as usize;

        if num_frames > MAX_FRAMES {
            return Err(Error::InvalidSize {
                value: num_frames,
                limit: MAX_FRAMES,
                context: "SHP_D2 header",
            });
        }

        // ── Offset table: (num_frames + 2) × u32 LE ────────────────────
        let total_entries = num_frames.saturating_add(2);
        let offset_table_start = 2usize; // right after the u16 num_frames
        let offset_table_bytes = total_entries.saturating_mul(4);
        let offset_table_end = offset_table_start.saturating_add(offset_table_bytes);

        if offset_table_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: offset_table_end,
                available: data.len(),
            });
        }

        let mut offsets = Vec::with_capacity(total_entries);
        for i in 0..total_entries {
            let pos = offset_table_start.saturating_add(i.saturating_mul(4));
            let offset = read_u32_le(data, pos)? as usize;
            offsets.push(offset);
        }

        // ── Parse each frame ────────────────────────────────────────────
        let mut frames = Vec::with_capacity(num_frames);

        for &frame_offset in offsets.iter().take(num_frames) {
            // Validate that the frame offset is within bounds and leaves
            // room for at least the 10-byte frame header.
            if frame_offset.saturating_add(FRAME_HEADER_SIZE) > data.len() {
                return Err(Error::InvalidOffset {
                    offset: frame_offset.saturating_add(FRAME_HEADER_SIZE),
                    bound: data.len(),
                });
            }

            // ── Frame header (10 bytes) ─────────────────────────────────
            let flags = read_u16_le(data, frame_offset)?;
            let _slices = read_u8(data, frame_offset.saturating_add(2))?;
            let width = read_u16_le(data, frame_offset.saturating_add(3))? as usize;
            let height = read_u8(data, frame_offset.saturating_add(5))? as usize;
            let _file_size = read_u16_le(data, frame_offset.saturating_add(6))?;
            let data_size = read_u16_le(data, frame_offset.saturating_add(8))? as usize;

            // Validate dimensions.
            if width == 0 || height == 0 {
                return Err(Error::InvalidMagic {
                    context: "SHP_D2 frame",
                });
            }
            if width > MAX_DIMENSION {
                return Err(Error::InvalidSize {
                    value: width,
                    limit: MAX_DIMENSION,
                    context: "SHP_D2 frame",
                });
            }
            if height > MAX_DIMENSION {
                return Err(Error::InvalidSize {
                    value: height,
                    limit: MAX_DIMENSION,
                    context: "SHP_D2 frame",
                });
            }

            let pixel_count = width.saturating_mul(height);

            // ── Optional remap table ────────────────────────────────────
            let has_remap = flags & FLAG_HAS_REMAP != 0;
            let pixel_data_offset = frame_offset
                .saturating_add(FRAME_HEADER_SIZE)
                .saturating_add(if has_remap { REMAP_TABLE_SIZE } else { 0 });

            let remap = if has_remap {
                let remap_start = frame_offset.saturating_add(FRAME_HEADER_SIZE);
                let remap_end = remap_start.saturating_add(REMAP_TABLE_SIZE);
                if remap_end > data.len() {
                    return Err(Error::UnexpectedEof {
                        needed: remap_end,
                        available: data.len(),
                    });
                }
                let slice = data
                    .get(remap_start..remap_end)
                    .ok_or(Error::UnexpectedEof {
                        needed: remap_end,
                        available: data.len(),
                    })?;
                let mut table = [0u8; 16];
                table.copy_from_slice(slice);
                Some(table)
            } else {
                None
            };

            // ── Pixel data ──────────────────────────────────────────────
            let pixel_data_end = pixel_data_offset.saturating_add(data_size);
            if pixel_data_end > data.len() {
                return Err(Error::UnexpectedEof {
                    needed: pixel_data_end,
                    available: data.len(),
                });
            }

            let raw_pixels =
                data.get(pixel_data_offset..pixel_data_end)
                    .ok_or(Error::UnexpectedEof {
                        needed: pixel_data_end,
                        available: data.len(),
                    })?;

            let is_uncompressed = flags & FLAG_UNCOMPRESSED != 0;
            let pixels = if is_uncompressed {
                // Uncompressed: raw pixel data, must be at least pixel_count bytes.
                if raw_pixels.len() < pixel_count {
                    return Err(Error::UnexpectedEof {
                        needed: pixel_count,
                        available: raw_pixels.len(),
                    });
                }
                raw_pixels[..pixel_count].to_vec()
            } else {
                // LCW (Format80) compressed.
                lcw::decompress(raw_pixels, pixel_count)?
            };

            frames.push(ShpD2Frame {
                width,
                height,
                flags,
                pixels,
                remap,
            });
        }

        Ok(ShpD2File { frames })
    }

    /// Returns a slice of all frames.
    #[inline]
    pub fn frames(&self) -> &[ShpD2Frame] {
        &self.frames
    }

    /// Returns the frame at `index`, or `None` if out of bounds.
    #[inline]
    pub fn frame(&self, index: usize) -> Option<&ShpD2Frame> {
        self.frames.get(index)
    }

    /// Returns the number of frames in the file.
    #[inline]
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
}

#[cfg(test)]
mod tests;
