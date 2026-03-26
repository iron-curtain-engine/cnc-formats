// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! WSA animation parser (`.wsa`).
//!
//! WSA files store full-screen or sprite animations used for menu backgrounds,
//! sidebar art, and in-game cutscenes.  Each frame is delta-compressed
//! against the previous frame using LCW compression + XOR-delta encoding.
//!
//! ## File Layout
//!
//! ```text
//! [WsaHeader]               14 bytes
//! [frame offsets]            (num_frames + 2) × u32   (relative to data start)
//! [compressed frame data]    back-to-back LCW segments
//! ```
//!
//! The extra `+2` offsets: `offsets[0]` is the first frame, `offsets[num_frames]`
//! points to the loop-back delta (XOR delta from the last frame back to
//! frame 0 for seamless looping — also serves as the end boundary for
//! frame N−1), and `offsets[num_frames + 1]` is the end-of-data sentinel
//! (offset past the last byte of frame data).
//!
//! ## Decoding
//!
//! 1. LCW-decompress the frame's raw data into a delta buffer.
//! 2. XOR the delta buffer with the previous frame (or a blank canvas for
//!    frame 0) to produce the current frame.
//!
//! This module parses the header and frame offset table and provides access
//! to per-frame compressed data.  Actual LCW decompression and XOR-delta
//! application are performed by calling [`crate::lcw`] on the frame data.
//!
//! ## References
//!
//! Format source: community documentation from the C&C Modding Wiki,
//! XCC Utilities source code, and binary analysis of game `.mix` archives.

use crate::error::Error;
use crate::read::{read_u16_le, read_u32_le};

// ─── Constants ───────────────────────────────────────────────────────────────

/// Size of the WSA file header in bytes.
const WSA_HEADER_SIZE: usize = 14;

/// V38: maximum number of frames per WSA file.  Real-world WSA files have
/// at most a few hundred frames; 8192 provides generous headroom while
/// capping the offset-table allocation to ~32 KB.
const MAX_FRAME_COUNT: usize = 8192;

/// V38: maximum frame pixel area.  Prevents degenerate dimensions from
/// causing enormous allocations.  4 MB per frame is far beyond anything
/// used by original game files (typical: 320×200 = 64 KB).
const MAX_FRAME_AREA: usize = 4 * 1024 * 1024;

// ─── Header ──────────────────────────────────────────────────────────────────

/// Parsed WSA file header.
///
/// The 14-byte header matches EA's `WSA_FileHeaderType` from
/// `TIBERIANDAWN/WIN32LIB/WSA.CPP`, cross-validated against OpenRA's
/// `WsaVideo.cs`.  Fields at offset 10-13 are two separate `u16` values
/// (`largest_frame_size` and `flags`), not one `u32`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsaHeader {
    /// Number of animation frames.
    pub num_frames: u16,
    /// Horizontal pixel offset for display positioning.
    pub x: u16,
    /// Vertical pixel offset for display positioning.
    pub y: u16,
    /// Frame width in pixels.
    pub width: u16,
    /// Frame height in pixels.
    pub height: u16,
    /// Size of the largest compressed frame delta in bytes.
    /// EA source calls this `LargestFrameSize` (u16, offset 10).
    pub largest_frame_size: u16,
    /// Flags field (u16, offset 12).
    /// Bit 0: embedded palette present (768 bytes of 6-bit VGA palette
    /// immediately after the offset table).
    pub flags: u16,
}

impl WsaHeader {
    /// Returns `true` if the WSA file has an embedded 768-byte palette.
    ///
    /// When set, the palette occupies 768 bytes immediately after the
    /// frame offset table (before the compressed frame data).
    #[inline]
    pub fn has_embedded_palette(&self) -> bool {
        self.flags & 1 != 0
    }
}

// ─── Frame ───────────────────────────────────────────────────────────────────

/// A single animation frame's compressed data from a WSA file.
///
/// The frame data is LCW-compressed.  After decompression, the resulting
/// buffer is XOR'd with the previous frame to produce the current frame's
/// pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsaFrame<'a> {
    /// Zero-based frame index.
    pub index: usize,
    /// Raw LCW-compressed frame data (borrowed from input).
    pub data: &'a [u8],
}

// ─── Parsed File ─────────────────────────────────────────────────────────────

/// Parsed WSA animation file.
///
/// Provides access to the header and per-frame compressed data.  Callers
/// decompress frames using [`crate::lcw`] and apply XOR-delta to reconstruct
/// each frame's pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsaFile<'a> {
    /// File header.
    pub header: WsaHeader,
    /// Compressed frame data segments.
    pub frames: Vec<WsaFrame<'a>>,
    /// True if the file contains a looping delta at `offsets[num_frames]`.
    /// Determined by checking whether the end-of-data sentinel
    /// `offsets[num_frames + 1]` is non-zero (indicating data exists
    /// beyond the last normal frame).
    pub has_loop_frame: bool,
    /// Compressed loop-back delta data (XOR delta from last frame back to
    /// frame 0 for seamless looping).  Present when `has_loop_frame` is true.
    /// Callers should LCW-decompress and XOR-apply this delta onto the last
    /// decoded frame to produce a seamless loop-back to frame 0.
    pub loop_frame: Option<WsaFrame<'a>>,
    /// Embedded 6-bit VGA palette (768 bytes), if `header.flags & 1` is set.
    pub palette: Option<&'a [u8]>,
}

impl<'a> WsaFile<'a> {
    /// Parses a WSA file from a raw byte slice.
    ///
    /// # Errors
    ///
    /// - [`Error::UnexpectedEof`] if the input is truncated.
    /// - [`Error::InvalidSize`] if frame count or pixel area exceed V38 caps.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        // ── Header ───────────────────────────────────────────────────────
        if data.len() < WSA_HEADER_SIZE {
            return Err(Error::UnexpectedEof {
                needed: WSA_HEADER_SIZE,
                available: data.len(),
            });
        }

        let num_frames = read_u16_le(data, 0)?;
        let x = read_u16_le(data, 2)?;
        let y = read_u16_le(data, 4)?;
        let width = read_u16_le(data, 6)?;
        let height = read_u16_le(data, 8)?;
        // Offsets 10-11 and 12-13: two separate u16 fields per EA WSA.CPP.
        let largest_frame_size = read_u16_le(data, 10)?;
        let flags = read_u16_le(data, 12)?;

        // V38: cap frame count.
        if (num_frames as usize) > MAX_FRAME_COUNT {
            return Err(Error::InvalidSize {
                value: num_frames as usize,
                limit: MAX_FRAME_COUNT,
                context: "WSA frame count",
            });
        }

        // V38: cap frame pixel area.
        let frame_area = (width as usize).saturating_mul(height as usize);
        if frame_area > MAX_FRAME_AREA {
            return Err(Error::InvalidSize {
                value: frame_area,
                limit: MAX_FRAME_AREA,
                context: "WSA frame area",
            });
        }

        let header = WsaHeader {
            num_frames,
            x,
            y,
            width,
            height,
            largest_frame_size,
            flags,
        };

        // ── Offset Table ─────────────────────────────────────────────────
        // The offset table has (num_frames + 2) entries:
        //   offsets[0..num_frames]     — frame start offsets
        //   offsets[num_frames]        — loop-back delta start (also bounds last frame)
        //   offsets[num_frames + 1]    — end-of-data sentinel (0 = no loop)
        let num_offsets = (num_frames as usize).saturating_add(2);
        let offsets_size = num_offsets.saturating_mul(4);
        let offsets_end = WSA_HEADER_SIZE.saturating_add(offsets_size);
        if offsets_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: offsets_end,
                available: data.len(),
            });
        }

        // Read the offset table.
        let mut offsets = Vec::with_capacity(num_offsets);
        for i in 0..num_offsets {
            let off_pos = WSA_HEADER_SIZE.saturating_add(i.saturating_mul(4));
            let raw = read_u32_le(data, off_pos)?;
            offsets.push(raw);
        }

        // WSA offsets are relative to the start of the data area (after
        // the header + offset table + optional palette).
        // If flags bit 0 is set, a 768-byte palette follows the offset table.
        let palette_size = if header.has_embedded_palette() {
            768
        } else {
            0
        };
        let palette_end = offsets_end.saturating_add(palette_size);
        if palette_size > 0 && palette_end > data.len() {
            return Err(Error::UnexpectedEof {
                needed: palette_end,
                available: data.len(),
            });
        }
        let palette = if palette_size > 0 {
            Some(
                data.get(offsets_end..palette_end)
                    .ok_or(Error::UnexpectedEof {
                        needed: palette_end,
                        available: data.len(),
                    })?,
            )
        } else {
            None
        };
        let data_base = palette_end;

        // ── Frames ───────────────────────────────────────────────────────
        let fc = num_frames as usize;
        let mut frames = Vec::with_capacity(fc);

        for i in 0..fc {
            // Safe: offsets Vec has num_frames + 2 entries, so [i] and [i+1]
            // are within bounds for i in 0..num_frames.
            let start_rel = offsets.get(i).copied().ok_or(Error::InvalidOffset {
                offset: i,
                bound: offsets.len(),
            })? as usize;
            let end_rel = offsets.get(i + 1).copied().ok_or(Error::InvalidOffset {
                offset: i + 1,
                bound: offsets.len(),
            })? as usize;

            // An offset of 0 means the frame is empty (no delta for this frame).
            if start_rel == 0 && end_rel == 0 {
                frames.push(WsaFrame {
                    index: i,
                    data: &[],
                });
                continue;
            }

            let abs_start = data_base.saturating_add(start_rel);
            let abs_end = data_base.saturating_add(end_rel);

            if abs_end < abs_start {
                return Err(Error::InvalidOffset {
                    offset: abs_end,
                    bound: abs_start,
                });
            }

            let frame_data = data.get(abs_start..abs_end).ok_or(Error::UnexpectedEof {
                needed: abs_end,
                available: data.len(),
            })?;

            frames.push(WsaFrame {
                index: i,
                data: frame_data,
            });
        }

        // Check for loop frame: per the WSA spec the loop-back delta lives
        // between offsets[num_frames] and offsets[num_frames + 1].  A non-zero
        // sentinel at offsets[num_frames + 1] means loop delta data exists.
        let loop_sentinel = offsets.get(fc.saturating_add(1)).copied().unwrap_or(0);
        let has_loop_frame = loop_sentinel != 0;
        let loop_frame = if has_loop_frame {
            let loop_start_rel = offsets.get(fc).copied().unwrap_or(0) as usize;
            let loop_end_rel = loop_sentinel as usize;
            if loop_end_rel > loop_start_rel {
                let abs_start = data_base.saturating_add(loop_start_rel);
                let abs_end = data_base.saturating_add(loop_end_rel);
                data.get(abs_start..abs_end)
                    .map(|d| WsaFrame { index: fc, data: d })
            } else {
                None
            }
        } else {
            None
        };

        Ok(WsaFile {
            header,
            frames,
            has_loop_frame,
            loop_frame,
            palette,
        })
    }

    /// Decodes all animation frames into palette-indexed pixel buffers.
    ///
    /// WSA frames are LCW-compressed XOR-deltas applied sequentially:
    /// frame 0 is XOR'd against a zero-filled canvas, each subsequent
    /// frame is XOR'd against the previous frame's output.
    ///
    /// The decoder uses a size-based heuristic to distinguish between raw
    /// XOR deltas (decompressed size == pixel count, used by community
    /// tools and our encoder) and Format40 command streams (any other
    /// size, used by the original game engine).  This maximises
    /// compatibility with both original game files and community-created
    /// WSA files.
    ///
    /// Returns one `Vec<u8>` per frame, each containing `width × height`
    /// palette-indexed pixels.
    ///
    /// # Errors
    ///
    /// - Forwards LCW decompression errors for corrupt frame data.
    pub fn decode_frames(&self) -> Result<Vec<Vec<u8>>, crate::error::Error> {
        let pixel_count = (self.header.width as usize).saturating_mul(self.header.height as usize);
        let mut canvas = vec![0u8; pixel_count];
        let mut decoded = Vec::with_capacity(self.frames.len());

        // V38: cap LCW output.  Format40 command streams are typically
        // smaller than pixel_count; raw XOR deltas equal pixel_count.
        // Allow up to 2× pixel_count to handle both cases with headroom.
        let max_lcw_output = pixel_count.saturating_mul(2).max(pixel_count);

        for frame in &self.frames {
            if frame.data.is_empty() {
                // Empty frame: no delta, canvas unchanged.
                decoded.push(canvas.clone());
                continue;
            }
            let delta = crate::lcw::decompress(frame.data, max_lcw_output)?;

            // Discriminate between raw XOR deltas and Format40 command
            // streams using the decompressed size:
            //
            // - Raw XOR delta: exactly `pixel_count` bytes — one XOR byte
            //   per pixel.  Used by our encoder and community tools.
            // - Format40 command stream: any other length — typically
            //   shorter than pixel_count (commands encode runs/skips
            //   compactly).  Used by the original game engine.
            //
            // This heuristic is reliable because raw XOR deltas are always
            // exactly pixel_count bytes, while Format40 streams are almost
            // never exactly that length (they'd need a pathological input
            // where command overhead exactly equals pixel_count).
            if delta.len() == pixel_count {
                // Raw XOR delta: byte-for-byte XOR.
                for (dst, src) in canvas.iter_mut().zip(delta.iter()) {
                    *dst ^= *src;
                }
            } else {
                // Format40 command stream.
                crate::xor_delta::apply_xor_delta(&mut canvas, &delta)?;
            }
            decoded.push(canvas.clone());
        }

        Ok(decoded)
    }
}

// ── WSA Encoder ──────────────────────────────────────────────────────────────
//
// Builds a valid WSA binary from palette-indexed pixel frames.  Each frame
// is XOR-delta'd against the previous frame (or a zero canvas for frame 0),
// then the delta is LCW-compressed.  This matches the standard WSA decode
// pipeline in reverse.
//
// Clean-room implementation based on the publicly documented WSA layout.

/// Encodes palette-indexed pixel frames into a complete WSA file.
///
/// Each frame in `frames` must be exactly `width × height` bytes.  The
/// encoder computes XOR-deltas between consecutive frames and LCW-compresses
/// each delta, matching the standard WSA decode pipeline.
///
/// Returns the complete WSA file as `Vec<u8>` that [`WsaFile::parse`] can
/// round-trip.
///
/// # Errors
///
/// Returns [`Error::InvalidSize`] if any frame has the wrong pixel count.
pub fn encode_frames(frames: &[&[u8]], width: u16, height: u16) -> Result<Vec<u8>, Error> {
    let pixel_count = (width as usize).saturating_mul(height as usize);
    for frame in frames {
        if frame.len() != pixel_count {
            return Err(Error::InvalidSize {
                value: frame.len(),
                limit: pixel_count,
                context: "WSA frame pixel count mismatch",
            });
        }
    }

    let num_frames = frames.len() as u16;
    let num_offsets = (num_frames as usize).saturating_add(2);
    let offsets_size = num_offsets.saturating_mul(4);
    let header_size = WSA_HEADER_SIZE;

    // Compute XOR-deltas and LCW-compress each frame.
    let mut prev = vec![0u8; pixel_count]; // zero canvas for frame 0
    let mut compressed_frames = Vec::with_capacity(frames.len());
    let mut largest = 0usize;

    for frame in frames {
        // XOR-delta: diff between current frame and previous.
        let mut delta = Vec::with_capacity(pixel_count);
        for (i, &px) in frame.iter().enumerate() {
            delta.push(px ^ prev.get(i).copied().unwrap_or(0));
        }
        let compressed = crate::lcw::compress(&delta);
        if compressed.len() > largest {
            largest = compressed.len();
        }
        compressed_frames.push(compressed);
        prev = frame.to_vec();
    }

    let data_base = header_size + offsets_size; // no embedded palette
    let total_data: usize = compressed_frames.iter().map(|c| c.len()).sum();
    let mut out = Vec::with_capacity(data_base + total_data);

    // ── Header (14 bytes) ────────────────────────────────────────────
    out.extend_from_slice(&num_frames.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // x
    out.extend_from_slice(&0u16.to_le_bytes()); // y
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&(largest as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // flags (no palette)

    // ── Offset table ((num_frames + 2) × u32) ───────────────────────
    // Offsets are relative to data_base.
    let mut data_offset = 0u32;
    for c in &compressed_frames {
        out.extend_from_slice(&data_offset.to_le_bytes());
        data_offset = data_offset.saturating_add(c.len() as u32);
    }
    // Sentinel: end of last frame.
    out.extend_from_slice(&data_offset.to_le_bytes());
    // Loop delta offset: 0 = no loop.
    out.extend_from_slice(&0u32.to_le_bytes());

    // ── Compressed frame data ────────────────────────────────────────
    for c in &compressed_frames {
        out.extend_from_slice(c);
    }

    Ok(out)
}

#[cfg(test)]
mod tests;
