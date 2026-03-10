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
//! Format source: `REDALERT/WIN32LIB/SHAPE.H`, `REDALERT/2KEYFRAM.CPP`.

use crate::error::Error;
use crate::lcw;

/// Bitmask in the raw offset table entry signalling uncompressed frame data.
const OFFSET_UNCOMPRESSED_FLAG: u32 = 0x8000_0000;

// ─── Header ──────────────────────────────────────────────────────────────────

/// The 14-byte keyframe animation header at the start of every SHP file.
///
/// Corresponds to `KeyFrameHeaderType` in `REDALERT/2KEYFRAM.CPP`.
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
    pub fn has_embedded_palette(&self) -> bool {
        self.flags & 0x0001 != 0
    }
}

// ─── Frame ───────────────────────────────────────────────────────────────────

/// A single frame extracted from an SHP file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShpFrame {
    /// Raw bytes for this frame as stored on disk.
    ///
    /// When `is_compressed` is `true`, call [`lcw::decompress`] to obtain
    /// the palette-indexed pixel data (width × height bytes).
    pub data: Vec<u8>,
    /// `false` = LCW-compressed; `true` = raw pixel data.
    pub is_uncompressed: bool,
}

impl ShpFrame {
    /// Returns the pixel data for this frame.
    ///
    /// - If `is_uncompressed` is `true`, the raw `data` bytes are the pixels.
    /// - If `is_uncompressed` is `false`, decompresses using LCW and returns
    ///   the result.
    ///
    /// The `expected_size` should be `header.width as usize * header.height as usize`.
    ///
    /// # Errors
    ///
    /// Forwards [`crate::lcw::decompress`] errors for compressed frames.
    pub fn pixels(&self, expected_size: usize) -> Result<Vec<u8>, Error> {
        if self.is_uncompressed {
            Ok(self.data.clone())
        } else {
            lcw::decompress(&self.data, expected_size)
        }
    }
}

// ─── ShpFile ─────────────────────────────────────────────────────────────────

/// A parsed SHP sprite file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShpFile {
    /// File header with frame dimensions and flags.
    pub header: ShpHeader,
    /// Optional embedded palette (768 bytes of 6-bit VGA RGB data).
    pub embedded_palette: Option<Vec<u8>>,
    /// All animation frames, in order.
    pub frames: Vec<ShpFrame>,
}

impl ShpFile {
    /// Parses an SHP file from a byte slice.
    ///
    /// # Errors
    ///
    /// - [`Error::UnexpectedEof`]  — data is too short for the header or offset table.
    /// - [`Error::InvalidOffset`]  — a frame offset points outside the file data.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        // ── Header (14 bytes = 7 × u16) ───────────────────────────────────
        if data.len() < 14 {
            return Err(Error::UnexpectedEof);
        }
        let read_u16 =
            |offset: usize| -> u16 { u16::from_le_bytes([data[offset], data[offset + 1]]) };

        let frame_count = read_u16(0) as usize;
        let x = read_u16(2);
        let y = read_u16(4);
        let width = read_u16(6);
        let height = read_u16(8);
        let largest_frame_size = read_u16(10);
        let flags = read_u16(12);

        let header = ShpHeader {
            frame_count: frame_count as u16,
            x,
            y,
            width,
            height,
            largest_frame_size,
            flags,
        };

        // ── Offset table: (frame_count + 1) × u32 ─────────────────────────
        let offset_table_bytes = (frame_count + 1) * 4;
        let offset_table_start = 14usize;
        if offset_table_start + offset_table_bytes > data.len() {
            return Err(Error::UnexpectedEof);
        }

        let mut raw_offsets = Vec::with_capacity(frame_count + 1);
        for i in 0..=frame_count {
            let pos = offset_table_start + i * 4;
            let raw = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
            raw_offsets.push(raw);
        }

        // ── Optional embedded palette ──────────────────────────────────────
        let has_palette = flags & 0x0001 != 0;
        let palette_start = offset_table_start + offset_table_bytes;
        let palette_end = if has_palette {
            palette_start + 768
        } else {
            palette_start
        };
        if palette_end > data.len() {
            return Err(Error::UnexpectedEof);
        }
        let embedded_palette = if has_palette {
            Some(data[palette_start..palette_end].to_vec())
        } else {
            None
        };

        // ── Frame data ─────────────────────────────────────────────────────
        // Offsets in the table are relative to the start of the whole file.
        let mut frames = Vec::with_capacity(frame_count);
        for i in 0..frame_count {
            let raw_start = raw_offsets[i];
            let raw_end = raw_offsets[i + 1];
            let is_uncompressed = (raw_start & OFFSET_UNCOMPRESSED_FLAG) != 0;
            let start = (raw_start & !OFFSET_UNCOMPRESSED_FLAG) as usize;
            let end = (raw_end & !OFFSET_UNCOMPRESSED_FLAG) as usize;

            if start > end || end > data.len() {
                return Err(Error::InvalidOffset);
            }
            let frame_data = data[start..end].to_vec();
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
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Returns the pixel area of a single frame (width × height).
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

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Too-short data returns UnexpectedEof.
    #[test]
    fn test_parse_too_short() {
        assert_eq!(ShpFile::parse(&[]).unwrap_err(), Error::UnexpectedEof);
        assert_eq!(
            ShpFile::parse(&[0u8; 13]).unwrap_err(),
            Error::UnexpectedEof
        );
    }

    /// Parse a zero-frame SHP (header only + 1 sentinel offset).
    #[test]
    fn test_parse_zero_frames() {
        let bytes = build_shp(8, 8, 0, &[], None);
        let shp = ShpFile::parse(&bytes).unwrap();
        assert_eq!(shp.frame_count(), 0);
        assert_eq!(shp.header.width, 8);
        assert_eq!(shp.header.height, 8);
        assert!(shp.embedded_palette.is_none());
    }

    /// Parse a single-frame uncompressed SHP.
    #[test]
    fn test_parse_single_frame() {
        let pixels: Vec<u8> = (0u8..64).collect(); // 8×8
        let bytes = build_shp(8, 8, 0, &[&pixels], None);
        let shp = ShpFile::parse(&bytes).unwrap();

        assert_eq!(shp.frame_count(), 1);
        assert_eq!(shp.header.frame_count, 1);
        assert_eq!(shp.header.width, 8);
        assert_eq!(shp.header.height, 8);
        assert!(!shp.header.has_embedded_palette());
        assert!(shp.frames[0].is_uncompressed);
        assert_eq!(shp.frames[0].data, pixels);
    }

    /// pixels() on an uncompressed frame returns the raw bytes unchanged.
    #[test]
    fn test_frame_pixels_uncompressed() {
        let pixels: Vec<u8> = (0u8..16).collect(); // 4×4
        let bytes = build_shp(4, 4, 0, &[&pixels], None);
        let shp = ShpFile::parse(&bytes).unwrap();

        let out = shp.frames[0].pixels(16).unwrap();
        assert_eq!(out, pixels);
    }

    /// Parse a multi-frame SHP; each frame's content is correct.
    #[test]
    fn test_parse_multiple_frames() {
        let f0: Vec<u8> = vec![0xAAu8; 16];
        let f1: Vec<u8> = vec![0xBBu8; 16];
        let f2: Vec<u8> = vec![0xCCu8; 16];
        let bytes = build_shp(4, 4, 0, &[&f0, &f1, &f2], None);
        let shp = ShpFile::parse(&bytes).unwrap();

        assert_eq!(shp.frame_count(), 3);
        assert_eq!(shp.frames[0].data, f0);
        assert_eq!(shp.frames[1].data, f1);
        assert_eq!(shp.frames[2].data, f2);
    }

    /// SHP with embedded palette: palette bytes are captured.
    #[test]
    fn test_parse_embedded_palette() {
        let mut pal = vec![0u8; 768];
        pal[0] = 63; // red channel of color 0 = 63
        let pixels: Vec<u8> = vec![0u8; 4];
        let bytes = build_shp(2, 2, 0x0001, &[&pixels], Some(&pal));
        let shp = ShpFile::parse(&bytes).unwrap();

        assert!(shp.header.has_embedded_palette());
        let ep = shp.embedded_palette.as_ref().unwrap();
        assert_eq!(ep.len(), 768);
        assert_eq!(ep[0], 63);
    }

    /// frame_pixel_count returns width × height.
    #[test]
    fn test_frame_pixel_count() {
        let bytes = build_shp(16, 24, 0, &[&vec![0u8; 384]], None);
        let shp = ShpFile::parse(&bytes).unwrap();
        assert_eq!(shp.frame_pixel_count(), 384);
    }

    /// LCW-compressed frame: pixels() decompresses correctly.
    #[test]
    fn test_compressed_frame_pixels() {
        // Build a small LCW stream that decompresses to 4 bytes of 0xAB.
        // 0xFE = long fill, count=4, value=0xAB, 0x80 = end
        let lcw_data: Vec<u8> = vec![0xFEu8, 0x04, 0x00, 0xAB, 0x80];

        // Build SHP manually without the uncompressed flag.
        // We'll construct the byte stream directly.
        let frame_count: u16 = 1;
        let width: u16 = 2;
        let height: u16 = 2;
        let flags: u16 = 0;
        let offset_table_size = (frame_count as usize + 1) * 4;
        let data_start = 14 + offset_table_size;

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&frame_count.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes()); // x
        bytes.extend_from_slice(&0u16.to_le_bytes()); // y
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&(lcw_data.len() as u16).to_le_bytes()); // largest
        bytes.extend_from_slice(&flags.to_le_bytes());

        // Offset table: offset[0] = data_start (no flag), offset[1] = sentinel
        let off0 = data_start as u32; // compressed: no OFFSET_UNCOMPRESSED_FLAG
        let off1 = (data_start + lcw_data.len()) as u32;
        bytes.extend_from_slice(&off0.to_le_bytes());
        bytes.extend_from_slice(&off1.to_le_bytes());

        // Frame data
        bytes.extend_from_slice(&lcw_data);

        let shp = ShpFile::parse(&bytes).unwrap();
        assert_eq!(shp.frame_count(), 1);
        assert!(!shp.frames[0].is_uncompressed);

        let pixels = shp.frames[0].pixels(4).unwrap();
        assert_eq!(pixels, vec![0xABu8; 4]);
    }
}
