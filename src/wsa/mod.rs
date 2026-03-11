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
//! acts as a sentinel for the last frame's end, and `offsets[num_frames + 1]`
//! (if non-zero) points to a "looping delta" that transforms the last frame
//! back into the first frame for seamless looping.
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
/// The header describes the animation dimensions, frame count, and the
/// size of the palette (0 if the animation uses an external palette).
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
    /// Size of the delta buffer needed for one frame (usually `width × height`).
    /// Also called `delta_buffer_size` in some documentation.
    pub buffer_size: u32,
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
    /// True if the file contains a looping delta (the extra offset at
    /// `offsets[num_frames + 1]` is non-zero).
    pub has_loop_frame: bool,
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
        let buffer_size = read_u32_le(data, 10)?;

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
            buffer_size,
        };

        // ── Offset Table ─────────────────────────────────────────────────
        // The offset table has (num_frames + 2) entries:
        //   offsets[0..num_frames]     — frame start offsets
        //   offsets[num_frames]        — sentinel (end of last frame)
        //   offsets[num_frames + 1]    — loop delta offset (0 = no loop)
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
        // the header + offset table).  We compute an absolute base.
        let data_base = offsets_end;

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

        // Check for loop frame: the last offset entry (offsets[num_frames + 1])
        // is non-zero if the animation loops.
        let loop_offset = offsets.get(fc.saturating_add(1)).copied().unwrap_or(0);
        let has_loop_frame = loop_offset != 0;

        Ok(WsaFile {
            header,
            frames,
            has_loop_frame,
        })
    }
}

#[cfg(test)]
mod tests;
