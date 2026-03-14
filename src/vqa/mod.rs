// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! VQA video container parser (`.vqa`).
//!
//! VQA (Vector Quantized Animation) is Westwood Studios' proprietary FMV
//! format used for in-game cinematics in Tiberian Dawn, Red Alert, and
//! sequels.  The container is a chunk-based IFF structure:
//!
//! ```text
//! "FORM" [size:u32be] "WVQA"            ← outer IFF envelope
//!   "VQHD" [size:u32be] [header data]   ← VQA header (42 bytes)
//!   "FINF" [size:u32be] [frame index]   ← frame offset table
//!   frame chunks ...                     ← VQFR, SND*, VQFL, etc.
//! ```
//!
//! This module parses the container structure (header + chunk directory)
//! without decoding VQ codebooks or audio streams.  Frame-level decoding
//! (LCW decompression, VQ lookup, audio mixing) is a separate concern.
//!
//! ## Chunk Types
//!
//! | FourCC | Description                                      |
//! |--------|--------------------------------------------------|
//! | `FORM` | Outer IFF container                              |
//! | `VQHD` | VQA header (dimensions, frame count, codebook)   |
//! | `FINF` | Frame index (byte offsets × 2 for each frame)    |
//! | `VQFR` | Full video frame (codebook + vectors)            |
//! | `VQFL` | Loop frame (optional)                            |
//! | `SND0` | Uncompressed audio chunk                         |
//! | `SND1` | Westwood ADPCM audio chunk                       |
//! | `SND2` | IMA ADPCM audio chunk                            |
//!
//! ## References
//!
//! Format source: community documentation from the C&C Modding Wiki,
//! Valery V. Anisimovsky's VQA format description, and binary analysis.

use crate::error::Error;
use crate::read::{read_u16_le, read_u32_le, read_u8};

mod decode;
mod encode;
mod snd;
pub use decode::{VqaAudio, VqaFrame};
pub use encode::{encode_vqa, VqaAudioInput, VqaEncodeParams};

// ─── Constants ───────────────────────────────────────────────────────────────

/// FourCC for the outer IFF container: "FORM".
const FOURCC_FORM: [u8; 4] = *b"FORM";

/// IFF form type identifying a VQA file: "WVQA".
const FOURCC_WVQA: [u8; 4] = *b"WVQA";

/// FourCC for the VQA header chunk: "VQHD".
const FOURCC_VQHD: [u8; 4] = *b"VQHD";

/// FourCC for the frame index chunk: "FINF".
const FOURCC_FINF: [u8; 4] = *b"FINF";

/// Minimum size of the outer envelope: "FORM" (4) + size (4) + "WVQA" (4) = 12.
const FORM_ENVELOPE_SIZE: usize = 12;

/// Size of the fixed VQHD header structure.
const VQHD_SIZE: usize = 42;

/// V38: maximum number of frames per VQA file.  Real-world VQA files have
/// at most a few thousand frames; 65535 covers the u16 range.
const MAX_FRAME_COUNT: usize = 65535;

/// V38: maximum chunk size.  256 MB prevents a malicious chunk size from
/// causing an enormous allocation.
const MAX_CHUNK_SIZE: u32 = 256 * 1024 * 1024;

// ─── Header ──────────────────────────────────────────────────────────────────

/// Parsed VQA file header (VQHD chunk).
///
/// This is the 42-byte structure inside the VQHD chunk that describes the
/// video's core properties.  Field layout matches the canonical EA
/// `VQAHeader` struct from `VQ/INCLUDE/VQA32/VQAFILE.H` (CnC_Red_Alert
/// repo), cross-validated against OpenRA's `VqaVideo.cs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VqaHeader {
    /// VQA format version (typically 2 for RA, 3 for TS).
    pub version: u16,
    /// Video flags.  Bit 0 = has audio, Bit 1 = has alt audio.
    pub flags: u16,
    /// Number of video frames.
    pub num_frames: u16,
    /// Video width in pixels.
    pub width: u16,
    /// Video height in pixels.
    pub height: u16,
    /// VQ block width (pixels per codebook block, typically 4).
    pub block_w: u8,
    /// VQ block height (pixels per codebook block, typically 2 or 4).
    pub block_h: u8,
    /// Frames per second (typically 15).
    pub fps: u8,
    /// VQ codebook group size (frames per partial codebook update cycle).
    pub groupsize: u8,
    /// Number of 1-color blocks.
    pub num_1_colors: u16,
    /// Total codebook entries.
    pub cb_entries: u16,
    /// Horizontal display position (0xFFFF = center).
    pub x_pos: u16,
    /// Vertical display position (0xFFFF = center).
    pub y_pos: u16,
    /// Maximum frame data size in bytes (used for buffer pre-allocation).
    pub max_frame_size: u16,
    // ── Audio fields ──
    /// Audio sample rate in Hz (0 = no audio).
    pub freq: u16,
    /// Number of audio channels (1 = mono, 2 = stereo).
    pub channels: u8,
    /// Audio bits per sample (8 or 16).
    pub bits: u8,
    // ── Extended fields ──
    /// Remaining 14 bytes of the VQHD (alt audio stream + reserved).
    /// Bytes 28–31: AltSampleRate(u16), AltChannels(u8), AltBitsPerSample(u8).
    /// Bytes 32–41: FutureUse (5 × u16, version-dependent).
    pub reserved: [u8; 14],
}

impl VqaHeader {
    /// Returns `true` if the file includes audio data.
    #[inline]
    pub fn has_audio(&self) -> bool {
        self.freq > 0 && self.channels > 0
    }

    /// Returns `true` if audio is stereo.
    #[inline]
    pub fn is_stereo(&self) -> bool {
        self.channels >= 2
    }
}

// ─── Chunk ───────────────────────────────────────────────────────────────────

/// A parsed IFF chunk within the VQA file.
///
/// Each chunk has a 4-byte FourCC identifier and a payload.  The parser
/// borrows the payload from the input slice to avoid copying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VqaChunk<'a> {
    /// Four-character code identifying the chunk type.
    pub fourcc: [u8; 4],
    /// Raw chunk payload (excluding the 8-byte chunk header).
    pub data: &'a [u8],
}

// ─── Parsed File ─────────────────────────────────────────────────────────────

/// Parsed VQA file: header, frame index, and chunk directory.
///
/// This structure captures the container-level metadata.  Actual video/audio
/// decoding is not performed — callers iterate `chunks` to find VQFR/SND*
/// data and decode on demand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VqaFile<'a> {
    /// The VQHD header.
    pub header: VqaHeader,
    /// Raw frame-info entries from the FINF chunk.  Each `u32` is stored
    /// as-is from the file: bits 31–28 carry per-frame flags (KEY, PAL,
    /// SYNC) and bits 27–0 encode the file offset in WORDs — callers must
    /// multiply by 2 to get the actual byte offset from the start of the
    /// FORM data.  `None` if no FINF chunk was found.
    pub frame_index: Option<Vec<u32>>,
    /// All chunks in file order (including VQHD and FINF if present).
    pub chunks: Vec<VqaChunk<'a>>,
}

impl<'a> VqaFile<'a> {
    /// Parses a VQA file from a raw byte slice.
    ///
    /// Validates the FORM/WVQA envelope, then iterates through the IFF
    /// chunks.  The VQHD chunk (if found) populates the header.  The FINF
    /// chunk (if found) is decoded into the frame index.
    ///
    /// # Errors
    ///
    /// - [`Error::UnexpectedEof`] if the input is truncated at any point.
    /// - [`Error::InvalidMagic`] if the FORM/WVQA signature is missing.
    /// - [`Error::InvalidSize`] if frame count or chunk sizes exceed V38 caps.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        // ── FORM envelope ────────────────────────────────────────────────
        if data.len() < FORM_ENVELOPE_SIZE {
            return Err(Error::UnexpectedEof {
                needed: FORM_ENVELOPE_SIZE,
                available: data.len(),
            });
        }

        let form_tag = data.get(0..4).ok_or(Error::UnexpectedEof {
            needed: 4,
            available: data.len(),
        })?;
        if form_tag != FOURCC_FORM {
            return Err(Error::InvalidMagic {
                context: "VQA FORM",
            });
        }

        // FORM size is big-endian in IFF.
        let form_size = read_u32_be(data, 4)? as usize;
        let form_type = data.get(8..12).ok_or(Error::UnexpectedEof {
            needed: 12,
            available: data.len(),
        })?;
        if form_type != FOURCC_WVQA {
            return Err(Error::InvalidMagic {
                context: "VQA WVQA type",
            });
        }

        // The FORM size field counts bytes after itself (i.e. after the
        // first 8 bytes).  Actual data runs from byte 8 to 8 + form_size.
        let form_end = 8usize.saturating_add(form_size).min(data.len());

        // ── Chunk iteration ──────────────────────────────────────────────
        let mut pos = FORM_ENVELOPE_SIZE; // first chunk starts after "FORM" + size + "WVQA"
        let mut chunks = Vec::new();
        let mut header: Option<VqaHeader> = None;
        let mut frame_index: Option<Vec<u32>> = None;

        while pos.saturating_add(8) <= form_end {
            let fourcc_end = pos.saturating_add(4);
            let fourcc_slice = data.get(pos..fourcc_end).ok_or(Error::UnexpectedEof {
                needed: fourcc_end,
                available: data.len(),
            })?;
            let mut fourcc = [0u8; 4];
            fourcc.copy_from_slice(fourcc_slice);

            // Chunk sizes are big-endian in IFF.
            let chunk_size = read_u32_be(data, fourcc_end)?;

            // V38: reject absurdly large chunk sizes.
            if chunk_size > MAX_CHUNK_SIZE {
                return Err(Error::InvalidSize {
                    value: chunk_size as usize,
                    limit: MAX_CHUNK_SIZE as usize,
                    context: "VQA chunk size",
                });
            }

            let payload_start = pos.saturating_add(8);
            let payload_end = payload_start.saturating_add(chunk_size as usize);

            // Strict structural check: reject chunks whose declared size
            // extends past the FORM boundary.  Silent truncation would allow
            // structurally malformed containers to be accepted.
            if payload_end > form_end {
                return Err(Error::InvalidOffset {
                    offset: payload_end,
                    bound: form_end,
                });
            }

            let payload = data
                .get(payload_start..payload_end)
                .ok_or(Error::UnexpectedEof {
                    needed: payload_end,
                    available: data.len(),
                })?;

            // ── VQHD: parse the VQA header ──
            if fourcc == FOURCC_VQHD && header.is_none() {
                header = Some(parse_vqhd(payload)?);
            }

            // ── FINF: parse the frame index ──
            if fourcc == FOURCC_FINF && frame_index.is_none() {
                if let Some(ref hdr) = header {
                    frame_index = Some(parse_finf(payload, hdr.num_frames)?);
                }
            }

            chunks.push(VqaChunk {
                fourcc,
                data: payload,
            });

            // IFF chunks are padded to even size.
            let padded_size = (chunk_size as usize).saturating_add(chunk_size as usize & 1);
            pos = payload_start.saturating_add(padded_size);
        }

        let header = header.ok_or(Error::InvalidMagic {
            context: "VQA missing VQHD chunk",
        })?;

        Ok(VqaFile {
            header,
            frame_index,
            chunks,
        })
    }
}

// ─── Internal Helpers ────────────────────────────────────────────────────────

/// Reads a big-endian `u32` at the given offset.
///
/// IFF containers use big-endian sizes, unlike the rest of the C&C binary
/// formats which are little-endian.
#[inline]
fn read_u32_be(data: &[u8], offset: usize) -> Result<u32, Error> {
    let end = offset.checked_add(4).ok_or(Error::UnexpectedEof {
        needed: usize::MAX,
        available: data.len(),
    })?;
    let slice = data.get(offset..end).ok_or(Error::UnexpectedEof {
        needed: end,
        available: data.len(),
    })?;
    // Safe: .get() above guarantees exactly 4 bytes.
    let mut buf = [0u8; 4];
    buf.copy_from_slice(slice);
    Ok(u32::from_be_bytes(buf))
}

/// Parses the 42-byte VQHD chunk payload into a [`VqaHeader`].
fn parse_vqhd(data: &[u8]) -> Result<VqaHeader, Error> {
    if data.len() < VQHD_SIZE {
        return Err(Error::UnexpectedEof {
            needed: VQHD_SIZE,
            available: data.len(),
        });
    }

    let version = read_u16_le(data, 0)?;
    let flags = read_u16_le(data, 2)?;
    let num_frames = read_u16_le(data, 4)?;
    let width = read_u16_le(data, 6)?;
    let height = read_u16_le(data, 8)?;
    let block_w = read_u8(data, 10)?;
    let block_h = read_u8(data, 11)?;
    // Offset 12–23: field order matches EA VQAFILE.H VQAHeader.
    let fps = read_u8(data, 12)?;
    let groupsize = read_u8(data, 13)?;
    let num_1_colors = read_u16_le(data, 14)?;
    let cb_entries = read_u16_le(data, 16)?;
    let x_pos = read_u16_le(data, 18)?;
    let y_pos = read_u16_le(data, 20)?;
    let max_frame_size = read_u16_le(data, 22)?;
    // Offset 24–27: primary audio stream.
    let freq = read_u16_le(data, 24)?;
    let channels = read_u8(data, 26)?;
    let bits = read_u8(data, 27)?;

    // Bytes 28–41: alt audio stream + FutureUse.  Stored opaquely
    // for round-trip fidelity — callers can decode if needed.
    let mut reserved = [0u8; 14];
    let res_slice = data.get(28..42).ok_or(Error::UnexpectedEof {
        needed: 42,
        available: data.len(),
    })?;
    reserved.copy_from_slice(res_slice);

    // V38: cap frame count.
    if (num_frames as usize) > MAX_FRAME_COUNT {
        return Err(Error::InvalidSize {
            value: num_frames as usize,
            limit: MAX_FRAME_COUNT,
            context: "VQA frame count",
        });
    }

    Ok(VqaHeader {
        version,
        flags,
        num_frames,
        width,
        height,
        block_w,
        block_h,
        fps,
        groupsize,
        num_1_colors,
        cb_entries,
        x_pos,
        y_pos,
        max_frame_size,
        freq,
        channels,
        bits,
        reserved,
    })
}

/// Parses the FINF chunk into a vector of raw frame-info entries.
///
/// Each FINF entry is a **little-endian `u32`** (4 bytes per frame).  Per
/// `binary-codecs.md`, bits 31–28 carry per-frame flags (KEY, PAL, SYNC)
/// and bits 27–0 encode the file offset in WORDs (multiply by 2 for byte
/// offset).  We store the raw `u32` and let callers decode flags / apply
/// the ×2 scaling based on VQA version.
fn parse_finf(data: &[u8], num_frames: u16) -> Result<Vec<u32>, Error> {
    let count = num_frames as usize;
    if count > MAX_FRAME_COUNT {
        return Err(Error::InvalidSize {
            value: count,
            limit: MAX_FRAME_COUNT,
            context: "VQA FINF frame count",
        });
    }

    let needed = count.saturating_mul(4);
    if data.len() < needed {
        return Err(Error::UnexpectedEof {
            needed,
            available: data.len(),
        });
    }

    let mut offsets = Vec::with_capacity(count);
    for i in 0..count {
        let raw = read_u32_le(data, i.saturating_mul(4))?;
        // FINF offsets are stored as (actual_offset / 2) in some versions,
        // or as direct offsets in others.  We store the raw value and let
        // callers interpret based on VQA version.
        offsets.push(raw);
    }

    Ok(offsets)
}

#[cfg(test)]
mod tests;
