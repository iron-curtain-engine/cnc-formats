// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! SHP sprite parser for Tiberian Sun and Red Alert 2 (`.shp`).
//!
//! This module handles the "SHP v2" format used by TS/RA2, which is distinct
//! from the Tiberian Dawn / Red Alert 1 SHP keyframe format in [`crate::shp`].
//! Key differences:
//!
//! - First u16 is always 0 (TD SHP starts with the frame count).
//! - Frame data uses scanline RLE compression (types 1/2) instead of
//!   full-frame LCW.
//! - Each frame has an independent crop rectangle (x, y, cx, cy) within a
//!   shared canvas of (width × height).
//!
//! ## Layout
//!
//! ```text
//! [File Header]     8 bytes   (zero, width, height, num_frames)
//! [Frame Headers]   num_frames × 24 bytes
//! [Frame Data]      variable  (compressed or raw pixel data)
//! ```
//!
//! ## Compression Types
//!
//! | ID | Method                                                  |
//! |----|---------------------------------------------------------|
//! | 0  | Uncompressed: `cx × cy` raw palette-indexed pixels      |
//! | 1  | Scanline RLE: per-scanline u16 length + RLE byte pairs  |
//! | 2  | Scanline RLE (detect/shadow variant, same decode logic) |
//! | 3  | LCW (Format80) compressed, same as TD SHP               |
//!
//! ## References
//!
//! Format source: ModEnc wiki, XCC Utilities source, community SHP editors.

use crate::error::Error;
use crate::read::{read_u16_le, read_u32_le, read_u8};

// ── Constants ─────────────────────────────────────────────────────────────────

/// File header size in bytes.
const FILE_HEADER_SIZE: usize = 8;

/// Per-frame header size in bytes.
const FRAME_HEADER_SIZE: usize = 24;

/// V38: maximum number of frames.
const MAX_FRAMES: usize = 4096;

/// V38: maximum canvas dimension.
const MAX_DIMENSION: usize = 4096;

/// V38: maximum pixel area per frame (to cap allocation).
const MAX_PIXEL_AREA: usize = 4096 * 4096;

// ── Types ─────────────────────────────────────────────────────────────────────

/// Parsed file header for a TS/RA2 SHP file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShpTsHeader {
    /// Canvas width in pixels (shared by all frames).
    pub width: u16,
    /// Canvas height in pixels (shared by all frames).
    pub height: u16,
    /// Number of animation frames.
    pub num_frames: u16,
}

/// Per-frame header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShpTsFrameHeader {
    /// X offset of the cropped frame within the canvas.
    pub x: u16,
    /// Y offset of the cropped frame within the canvas.
    pub y: u16,
    /// Width of the cropped frame (actual pixel columns).
    pub cx: u16,
    /// Height of the cropped frame (actual pixel rows).
    pub cy: u16,
    /// Compression type (0 = none, 1/2 = scanline RLE, 3 = LCW).
    pub compression: u8,
    /// Absolute byte offset to this frame's pixel data within the file.
    pub file_offset: u32,
}

/// A single animation frame from a TS/RA2 SHP file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShpTsFrame<'input> {
    /// Frame header (crop rectangle, compression, offset).
    pub header: ShpTsFrameHeader,
    /// Raw frame data slice (compressed or uncompressed), borrowed from input.
    pub raw_data: &'input [u8],
}

impl ShpTsFrame<'_> {
    /// Decodes the frame's pixel data based on its compression type.
    ///
    /// Returns a `Vec<u8>` of `cx × cy` palette-indexed pixels.
    /// Pixel value 0 typically represents transparency.
    ///
    /// # Errors
    ///
    /// Returns an error if decompression fails or data is truncated.
    pub fn pixels(&self) -> Result<Vec<u8>, Error> {
        let cx = self.header.cx as usize;
        let cy = self.header.cy as usize;
        let area = cx.saturating_mul(cy);

        if area == 0 {
            return Ok(Vec::new());
        }

        match self.header.compression {
            0 => {
                // Uncompressed: raw pixels.
                if self.raw_data.len() < area {
                    return Err(Error::UnexpectedEof {
                        needed: area,
                        available: self.raw_data.len(),
                    });
                }
                let slice = self.raw_data.get(..area).ok_or(Error::UnexpectedEof {
                    needed: area,
                    available: self.raw_data.len(),
                })?;
                Ok(slice.to_vec())
            }
            1 | 2 => {
                // Scanline RLE.
                decode_scanline_rle(self.raw_data, cx, cy)
            }
            3 => {
                // LCW (Format80) compression.
                crate::lcw::decompress(self.raw_data, area)
            }
            other => Err(Error::InvalidMagic {
                context: if other == 255 {
                    "SHP TS frame compression type 0xFF"
                } else {
                    "SHP TS unknown frame compression type"
                },
            }),
        }
    }
}

/// Parsed Tiberian Sun / Red Alert 2 SHP file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShpTsFile<'input> {
    /// File header (canvas dimensions, frame count).
    pub header: ShpTsHeader,
    /// Animation frames.
    pub frames: Vec<ShpTsFrame<'input>>,
}

impl<'input> ShpTsFile<'input> {
    /// Parses a TS/RA2 SHP file from a raw byte slice.
    ///
    /// # Layout
    ///
    /// The 8-byte file header is followed by `num_frames` × 24-byte frame
    /// headers.  Each frame header contains an absolute file offset to the
    /// frame's pixel data.
    ///
    /// # Errors
    ///
    /// Returns errors for truncated input, invalid magic (first u16 != 0),
    /// or frame/dimension counts exceeding V38 safety caps.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        // ── File Header (8 bytes) ────────────────────────────────────────
        if data.len() < FILE_HEADER_SIZE {
            return Err(Error::UnexpectedEof {
                needed: FILE_HEADER_SIZE,
                available: data.len(),
            });
        }

        let zero = read_u16_le(data, 0)?;
        if zero != 0 {
            return Err(Error::InvalidMagic {
                context: "SHP TS first u16 must be 0",
            });
        }

        let width = read_u16_le(data, 2)?;
        let height = read_u16_le(data, 4)?;
        let num_frames = read_u16_le(data, 6)?;

        // V38: validate dimensions and frame count.
        if (num_frames as usize) > MAX_FRAMES {
            return Err(Error::InvalidSize {
                value: num_frames as usize,
                limit: MAX_FRAMES,
                context: "SHP TS frame count",
            });
        }
        if (width as usize) > MAX_DIMENSION || (height as usize) > MAX_DIMENSION {
            return Err(Error::InvalidSize {
                value: width.max(height) as usize,
                limit: MAX_DIMENSION,
                context: "SHP TS canvas dimensions",
            });
        }

        let header = ShpTsHeader {
            width,
            height,
            num_frames,
        };

        // ── Frame Headers ────────────────────────────────────────────────
        let headers_end = FILE_HEADER_SIZE
            .saturating_add((num_frames as usize).saturating_mul(FRAME_HEADER_SIZE));
        if headers_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: headers_end,
                available: data.len(),
            });
        }

        let mut frames = Vec::with_capacity(num_frames as usize);
        for i in 0..num_frames as usize {
            let off = FILE_HEADER_SIZE.saturating_add(i.saturating_mul(FRAME_HEADER_SIZE));

            let x = read_u16_le(data, off)?;
            let y = read_u16_le(data, off.saturating_add(2))?;
            let cx = read_u16_le(data, off.saturating_add(4))?;
            let cy = read_u16_le(data, off.saturating_add(6))?;
            let compression = read_u8(data, off.saturating_add(8))?;
            let file_offset = read_u32_le(data, off.saturating_add(20))?;

            // V38: validate frame pixel area.
            let area = (cx as usize).saturating_mul(cy as usize);
            if area > MAX_PIXEL_AREA {
                return Err(Error::InvalidSize {
                    value: area,
                    limit: MAX_PIXEL_AREA,
                    context: "SHP TS frame pixel area",
                });
            }

            let fh = ShpTsFrameHeader {
                x,
                y,
                cx,
                cy,
                compression,
                file_offset,
            };

            // Borrow raw frame data from input.  For empty frames (cx=0 or
            // cy=0 or offset=0), borrow an empty slice.
            let raw_data = if file_offset == 0 || cx == 0 || cy == 0 {
                &[] as &[u8]
            } else {
                let fo = file_offset as usize;
                // Borrow from file_offset to end of data — the exact compressed
                // size is unknown at parse time for RLE frames.
                data.get(fo..).ok_or(Error::InvalidOffset {
                    offset: fo,
                    bound: data.len(),
                })?
            };

            frames.push(ShpTsFrame {
                header: fh,
                raw_data,
            });
        }

        Ok(ShpTsFile { header, frames })
    }
}

// ── Scanline RLE Decoder ──────────────────────────────────────────────────────

/// Decodes scanline-RLE compressed frame data.
///
/// Each scanline starts with a `u16` byte-length (including the length field
/// itself), followed by RLE-encoded pixels:
///
/// - Byte `0x00` followed by a count byte: skip (transparent) run of that
///   many pixels (pixel value 0).
/// - Any other byte: literal palette index pixel.
///
/// The output buffer is `cx × cy` bytes.  If a scanline decodes fewer than
/// `cx` pixels, the remainder is filled with 0 (transparent).
fn decode_scanline_rle(data: &[u8], cx: usize, cy: usize) -> Result<Vec<u8>, Error> {
    let area = cx.saturating_mul(cy);
    let mut pixels = Vec::with_capacity(area);
    let mut offset = 0usize;

    for _row in 0..cy {
        // Each scanline: u16 length (total bytes including this u16).
        if offset.saturating_add(2) > data.len() {
            // Remaining scanlines are transparent.
            pixels.resize(area, 0);
            return Ok(pixels);
        }
        let scan_len = read_u16_le(data, offset)? as usize;
        offset = offset.saturating_add(2);

        // Bytes of RLE data in this scanline (excluding the length field).
        let rle_bytes = scan_len.saturating_sub(2);
        let scan_end = offset.saturating_add(rle_bytes).min(data.len());

        let row_start = pixels.len();
        let mut pos = offset;

        while pos < scan_end && (pixels.len() - row_start) < cx {
            let byte = match data.get(pos) {
                Some(&b) => b,
                None => break,
            };
            pos = pos.saturating_add(1);

            if byte == 0 {
                // Transparent run: next byte is count.
                let count = match data.get(pos) {
                    Some(&c) => c as usize,
                    None => break,
                };
                pos = pos.saturating_add(1);
                let remaining = cx.saturating_sub(pixels.len() - row_start);
                let fill = count.min(remaining);
                pixels.resize(pixels.len().saturating_add(fill), 0);
            } else {
                // Literal pixel.
                pixels.push(byte);
            }
        }

        // Pad scanline to full width if fewer pixels were decoded.
        let decoded_in_row = pixels.len().saturating_sub(row_start);
        if decoded_in_row < cx {
            pixels.resize(pixels.len().saturating_add(cx - decoded_in_row), 0);
        }

        offset = scan_end;
    }

    // Ensure output is exactly area bytes.
    pixels.resize(area, 0);
    Ok(pixels)
}

#[cfg(test)]
mod tests;
