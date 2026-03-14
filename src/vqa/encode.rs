// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! VQA file encoder — builds a version 2 VQA container from raw frames + audio.
//!
//! ## Encoding Strategy
//!
//! This encoder produces a valid Version 2 VQA file using a simplified
//! encoding approach:
//!
//! 1. **Palette**: the first frame's palette is used as the global CPL.
//! 2. **Codebook**: a full CBF is emitted at the start of each `groupsize`
//!    cycle (no partial CBP updates).  The codebook is built by collecting
//!    all unique pixel blocks across the frame and selecting up to
//!    `cb_entries` representative blocks.
//! 3. **VPT**: each screen block is mapped to its nearest codebook entry
//!    (or marked as a solid fill if all pixels match).
//! 4. **Audio**: IMA ADPCM encoded and distributed as SND2 chunks (one per
//!    frame, interleaved before each VQFR).
//!
//! The output is designed for compatibility with the C&C engine's VQA
//! player and with this crate's own `VqaFile::parse` + `decode_frames()`.
//!
//! ## References
//!
//! IFF container format (FORM/WVQA); VQHD/FINF/VQFR chunk structure per
//! VQA_INFO.TXT (Gordan Ugarkovic, 2004).  Clean-room implementation.

use crate::aud;
use crate::error::Error;
use crate::lcw;

// ─── Constants ───────────────────────────────────────────────────────────────

/// Default block width for VQ encoding.
const DEFAULT_BLOCK_W: u8 = 4;
/// Default block height for VQ encoding.
const DEFAULT_BLOCK_H: u8 = 2;
/// Default codebook entry count.
const DEFAULT_CB_ENTRIES: u16 = 256;
/// Default groupsize (frames per codebook cycle).
const DEFAULT_GROUPSIZE: u8 = 8;
/// Default frames per second.
const DEFAULT_FPS: u8 = 15;

// ─── Public API ──────────────────────────────────────────────────────────────

/// Parameters for VQA encoding.
pub struct VqaEncodeParams {
    /// Block width in pixels (default: 4).
    pub block_w: u8,
    /// Block height in pixels (default: 2).
    pub block_h: u8,
    /// Maximum codebook entries (default: 256).
    pub cb_entries: u16,
    /// Groupsize — frames per codebook cycle (default: 8).
    pub groupsize: u8,
    /// Frames per second (default: 15).
    pub fps: u8,
}

impl Default for VqaEncodeParams {
    fn default() -> Self {
        Self {
            block_w: DEFAULT_BLOCK_W,
            block_h: DEFAULT_BLOCK_H,
            cb_entries: DEFAULT_CB_ENTRIES,
            groupsize: DEFAULT_GROUPSIZE,
            fps: DEFAULT_FPS,
        }
    }
}

/// Optional audio data for VQA encoding.
pub struct VqaAudioInput<'a> {
    /// PCM audio samples (signed 16-bit, interleaved for stereo).
    pub samples: &'a [i16],
    /// Audio sample rate in Hz.
    pub sample_rate: u16,
    /// 1 (mono) or 2 (stereo).
    pub channels: u8,
}

/// Encodes frames and audio into a VQA version 2 file.
///
/// # Arguments
///
/// - `indexed_frames`: palette-indexed pixel data per frame (row-major,
///   `width × height` bytes).
/// - `palette_rgb8`: 768-byte RGB palette (8-bit values, 0–255).
/// - `width`, `height`: frame dimensions in pixels.
/// - `audio`: optional audio data (PCM samples + rate + channels).
/// - `params`: encoding parameters (block size, codebook size, fps, etc.).
///
/// # Errors
///
/// - [`Error::InvalidSize`] if dimensions or frame count are invalid.
/// - [`Error::DecompressionError`] if LCW compression fails.
pub fn encode_vqa(
    indexed_frames: &[Vec<u8>],
    palette_rgb8: &[u8; 768],
    width: u16,
    height: u16,
    audio: Option<&VqaAudioInput<'_>>,
    params: &VqaEncodeParams,
) -> Result<Vec<u8>, Error> {
    if indexed_frames.is_empty() {
        return Err(Error::DecompressionError {
            reason: "no frames provided for VQA encoding",
        });
    }
    if width == 0 || height == 0 {
        return Err(Error::InvalidSize {
            value: 0,
            limit: 1,
            context: "VQA encode dimensions",
        });
    }
    let block_w = params.block_w.max(1) as usize;
    let block_h = params.block_h.max(1) as usize;
    let block_size = block_w.saturating_mul(block_h);
    let blocks_x = (width as usize) / block_w;
    let blocks_y = (height as usize) / block_h;
    let total_blocks = blocks_x.saturating_mul(blocks_y);
    let fill_marker: u8 = if block_h == 4 { 0xFF } else { 0x0F };
    let geo = BlockGeometry {
        block_w,
        block_h,
        block_size,
        blocks_x,
        blocks_y,
        total_blocks,
        fill_marker,
    };
    let num_frames = indexed_frames.len() as u16;
    let (sample_rate, channels, stereo) = audio
        .map(|a| (a.sample_rate, a.channels, a.channels >= 2))
        .unwrap_or((0, 0, false));

    // Convert 8-bit palette to 6-bit VGA for CPL chunk.
    let mut palette_6bit = [0u8; 768];
    for (dst, &src) in palette_6bit.iter_mut().zip(palette_rgb8.iter()) {
        // 8-bit (0–255) → 6-bit (0–63): divide by 4 (shift right 2).
        *dst = src >> 2;
    }

    // Encode audio chunks (one per frame, distributed evenly).
    let audio_chunks: Vec<Vec<u8>> = if let Some(a) = audio {
        if sample_rate > 0 && channels > 0 {
            encode_audio_chunks(a.samples, stereo, num_frames as usize)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    let has_audio = !audio_chunks.is_empty();

    // Build per-frame VQFR chunks.
    let groupsize = params.groupsize.max(1) as usize;
    let cb_entries = params.cb_entries.max(1) as usize;
    let mut frame_data_list: Vec<Vec<u8>> = Vec::with_capacity(indexed_frames.len());
    let mut max_frame_size: usize = 0;

    for (frame_idx, pixels) in indexed_frames.iter().enumerate() {
        let mut vqfr = Vec::new();

        // Emit a full codebook at the start of each groupsize cycle.
        if frame_idx % groupsize == 0 {
            let codebook = build_codebook(
                pixels,
                width as usize,
                height as usize,
                block_w,
                block_h,
                cb_entries,
            );

            // CPL0 (palette) — only on first frame or if palette changes.
            if frame_idx == 0 {
                write_sub_chunk(&mut vqfr, b"CPL0", &palette_6bit);
            }

            // CBF0 (full codebook, uncompressed).
            // Try LCW compression; use compressed if smaller.
            let compressed_cb = lcw::compress(&codebook);
            if compressed_cb.len() < codebook.len() {
                write_sub_chunk(&mut vqfr, b"CBFZ", &compressed_cb);
            } else {
                write_sub_chunk(&mut vqfr, b"CBF0", &codebook);
            }

            // VPT — map each block to the codebook.
            let vpt = build_vpt(pixels, &codebook, width as usize, &geo);

            let compressed_vpt = lcw::compress(&vpt);
            if compressed_vpt.len() < vpt.len() {
                write_sub_chunk(&mut vqfr, b"VPTZ", &compressed_vpt);
            } else {
                write_sub_chunk(&mut vqfr, b"VPT0", &vpt);
            }
        } else {
            // Non-keyframe: re-use the last codebook, just emit a new VPT.
            // Rebuild the codebook for this cycle to find nearest entries.
            let cycle_start = (frame_idx / groupsize) * groupsize;
            let keyframe = indexed_frames.get(cycle_start).unwrap_or(pixels);
            let codebook = build_codebook(
                keyframe,
                width as usize,
                height as usize,
                block_w,
                block_h,
                cb_entries,
            );

            let vpt = build_vpt(pixels, &codebook, width as usize, &geo);

            let compressed_vpt = lcw::compress(&vpt);
            if compressed_vpt.len() < vpt.len() {
                write_sub_chunk(&mut vqfr, b"VPTZ", &compressed_vpt);
            } else {
                write_sub_chunk(&mut vqfr, b"VPT0", &vpt);
            }
        }

        if vqfr.len() > max_frame_size {
            max_frame_size = vqfr.len();
        }
        frame_data_list.push(vqfr);
    }

    // ── Assemble IFF container ──────────────────────────────────────────

    // Estimate total size.
    let estimated = 12
        + 50
        + 8
        + (num_frames as usize * 4)
        + 8
        + frame_data_list.iter().map(|f| f.len() + 8).sum::<usize>()
        + audio_chunks.iter().map(|a| a.len() + 8).sum::<usize>()
        + 256;
    let mut out = Vec::with_capacity(estimated);

    // FORM header (patched later).
    out.extend_from_slice(b"FORM");
    let form_size_pos = out.len();
    write_u32_be(&mut out, 0); // placeholder
    out.extend_from_slice(b"WVQA");

    // ── VQHD (42 bytes) ─────────────────────────────────────────────
    out.extend_from_slice(b"VQHD");
    write_u32_be(&mut out, 42);

    write_u16_le_buf(&mut out, 2); // version
    let flags: u16 = if has_audio { 1 } else { 0 };
    write_u16_le_buf(&mut out, flags); // flags
    write_u16_le_buf(&mut out, num_frames); // num_frames
    write_u16_le_buf(&mut out, width); // width
    write_u16_le_buf(&mut out, height); // height
    out.push(params.block_w.max(1)); // block_w
    out.push(params.block_h.max(1)); // block_h
    out.push(params.fps.max(1)); // fps
    out.push(params.groupsize.max(1)); // groupsize
    write_u16_le_buf(&mut out, 0); // num_1_colors
    write_u16_le_buf(&mut out, params.cb_entries); // cb_entries
    write_u16_le_buf(&mut out, 0xFFFF); // x_pos (center)
    write_u16_le_buf(&mut out, 0xFFFF); // y_pos (center)
    write_u16_le_buf(&mut out, max_frame_size.min(u16::MAX as usize) as u16); // max_frame_size
                                                                              // Audio fields.
    write_u16_le_buf(&mut out, if has_audio { sample_rate } else { 0 }); // freq
    out.push(if has_audio { channels.max(1) } else { 0 }); // channels
    out.push(if has_audio { 16 } else { 0 }); // bits
                                              // Reserved (14 bytes): alt audio + future use.
    out.extend_from_slice(&[0u8; 14]);

    // ── FINF (frame index placeholder) ───────────────────────────────
    // FINF has 4 bytes per frame, storing raw u32 offsets.  We'll write
    // placeholder zeros and patch after writing frame chunks.
    out.extend_from_slice(b"FINF");
    let finf_data_size = (num_frames as usize).saturating_mul(4);
    write_u32_be(&mut out, finf_data_size as u32);
    let finf_data_start = out.len();
    out.resize(out.len().saturating_add(finf_data_size), 0);

    // ── Frame and audio chunks ───────────────────────────────────────
    for (frame_idx, vqfr_data) in frame_data_list.iter().enumerate() {
        // Record frame offset for FINF (offset from start of FORM data, i.e.
        // from byte 8 of the file; divided by 2 per VQA convention, but we
        // store raw offset for version 2).
        let frame_offset = out.len() as u32;
        let finf_entry_pos = finf_data_start.saturating_add(frame_idx.saturating_mul(4));
        if let Some(slot) = out.get_mut(finf_entry_pos..finf_entry_pos.saturating_add(4)) {
            slot.copy_from_slice(&frame_offset.to_le_bytes());
        }

        // SND2 audio chunk (before VQFR for A/V sync).
        if let Some(audio_chunk) = audio_chunks.get(frame_idx) {
            if !audio_chunk.is_empty() {
                out.extend_from_slice(b"SND2");
                write_u32_be(&mut out, audio_chunk.len() as u32);
                out.extend_from_slice(audio_chunk);
                // Pad to even.
                if audio_chunk.len() & 1 != 0 {
                    out.push(0);
                }
            }
        }

        // VQFR chunk.
        out.extend_from_slice(b"VQFR");
        write_u32_be(&mut out, vqfr_data.len() as u32);
        out.extend_from_slice(vqfr_data);
        // Pad to even.
        if vqfr_data.len() & 1 != 0 {
            out.push(0);
        }
    }

    // Patch FORM size (everything after "FORM" + size field = total - 8).
    let form_size = (out.len() - 8) as u32;
    if let Some(slot) = out.get_mut(form_size_pos..form_size_pos.saturating_add(4)) {
        slot.copy_from_slice(&form_size.to_be_bytes());
    }

    Ok(out)
}

// ─── Internal Helpers ────────────────────────────────────────────────────────

/// Writes a big-endian u32 to the output buffer — IFF convention.
#[inline]
fn write_u32_be(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// Writes a little-endian u16 to the output buffer — VQHD fields are LE.
#[inline]
fn write_u16_le_buf(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Writes a sub-chunk inside a VQFR payload (FourCC + BE size + data).
fn write_sub_chunk(out: &mut Vec<u8>, fourcc: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(fourcc);
    // V38: saturate to u32::MAX rather than silently truncating on 64-bit.
    let len_u32 = u32::try_from(data.len()).unwrap_or(u32::MAX);
    write_u32_be(out, len_u32);
    out.extend_from_slice(data);
    // Pad to even boundary.
    if data.len() & 1 != 0 {
        out.push(0);
    }
}

/// Builds a VQ codebook from a single frame's pixel blocks.
///
/// Extracts all `block_w × block_h` pixel blocks from the frame, deduplicates
/// them, and selects up to `max_entries` representative blocks.  This is a
/// simple first-come strategy — a production encoder would use k-means or
/// similar, but for C&C content (low-res, 256-color) this produces acceptable
/// results.
fn build_codebook(
    pixels: &[u8],
    width: usize,
    height: usize,
    block_w: usize,
    block_h: usize,
    max_entries: usize,
) -> Vec<u8> {
    let block_size = block_w.saturating_mul(block_h);
    let blocks_x = width / block_w.max(1);
    let blocks_y = height / block_h.max(1);
    let mut entries: Vec<Vec<u8>> = Vec::with_capacity(max_entries);
    let mut seen = std::collections::HashSet::new();

    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            if entries.len() >= max_entries {
                break;
            }
            let mut block = Vec::with_capacity(block_size);
            for row in 0..block_h {
                let y = by.saturating_mul(block_h).saturating_add(row);
                for col in 0..block_w {
                    let x = bx.saturating_mul(block_w).saturating_add(col);
                    let idx = y.saturating_mul(width).saturating_add(x);
                    block.push(pixels.get(idx).copied().unwrap_or(0));
                }
            }
            if seen.insert(block.clone()) {
                entries.push(block);
            }
        }
    }

    // Flatten into a contiguous byte array.
    let mut codebook = Vec::with_capacity(entries.len().saturating_mul(block_size));
    for entry in &entries {
        codebook.extend_from_slice(entry);
    }
    codebook
}

/// Pre-computed block geometry for VQ encoding.
struct BlockGeometry {
    block_w: usize,
    block_h: usize,
    block_size: usize,
    blocks_x: usize,
    blocks_y: usize,
    total_blocks: usize,
    fill_marker: u8,
}

/// Builds the Version 2 VPT (Vector Pointer Table) for a frame.
///
/// The VPT is split into lo/hi halves.  For each screen block:
/// - If all pixels are the same color → solid fill: hi = fill_marker, lo = color.
/// - Otherwise → codebook lookup: find nearest entry, hi = index / 256,
///   lo = index % 256.
fn build_vpt(pixels: &[u8], codebook: &[u8], width: usize, geo: &BlockGeometry) -> Vec<u8> {
    // VPT is 2 × total_blocks: first half = lo, second half = hi.
    let mut vpt = vec![0u8; geo.total_blocks.saturating_mul(2)];

    let num_entries = if geo.block_size > 0 {
        codebook.len() / geo.block_size
    } else {
        0
    };

    for by in 0..geo.blocks_y {
        for bx in 0..geo.blocks_x {
            let idx = by.saturating_mul(geo.blocks_x).saturating_add(bx);

            // Extract current block.
            let mut block = Vec::with_capacity(geo.block_size);
            let mut all_same = true;
            let mut first_pixel: u8 = 0;
            for row in 0..geo.block_h {
                let y = by.saturating_mul(geo.block_h).saturating_add(row);
                for col in 0..geo.block_w {
                    let x = bx.saturating_mul(geo.block_w).saturating_add(col);
                    let px = pixels
                        .get(y.saturating_mul(width).saturating_add(x))
                        .copied()
                        .unwrap_or(0);
                    if block.is_empty() {
                        first_pixel = px;
                    } else if px != first_pixel {
                        all_same = false;
                    }
                    block.push(px);
                }
            }

            if all_same {
                // Solid fill.
                if let Some(lo) = vpt.get_mut(idx) {
                    *lo = first_pixel;
                }
                if let Some(hi) = vpt.get_mut(geo.total_blocks.saturating_add(idx)) {
                    *hi = geo.fill_marker;
                }
            } else {
                // Find nearest codebook entry by sum of absolute differences.
                let mut best_entry: usize = 0;
                let mut best_dist: u64 = u64::MAX;

                for entry_idx in 0..num_entries {
                    let cb_off = entry_idx.saturating_mul(geo.block_size);
                    let mut dist: u64 = 0;
                    for j in 0..geo.block_size {
                        let a = block.get(j).copied().unwrap_or(0) as i32;
                        let b = codebook.get(cb_off.saturating_add(j)).copied().unwrap_or(0) as i32;
                        dist = dist.saturating_add((a - b).unsigned_abs() as u64);
                    }
                    if dist < best_dist {
                        best_dist = dist;
                        best_entry = entry_idx;
                        if dist == 0 {
                            break;
                        }
                    }
                }

                if let Some(lo) = vpt.get_mut(idx) {
                    *lo = (best_entry & 0xFF) as u8;
                }
                if let Some(hi) = vpt.get_mut(geo.total_blocks.saturating_add(idx)) {
                    *hi = ((best_entry >> 8) & 0xFF) as u8;
                }
            }
        }
    }

    vpt
}

/// Distributes PCM audio samples across frames as IMA ADPCM chunks.
///
/// Evenly divides samples among `num_frames` chunks, then IMA-encodes each.
fn encode_audio_chunks(samples: &[i16], stereo: bool, num_frames: usize) -> Vec<Vec<u8>> {
    if num_frames == 0 || samples.is_empty() {
        return Vec::new();
    }

    let samples_per_frame = samples.len() / num_frames;
    // Round to channel count boundary.
    let ch = if stereo { 2 } else { 1 };
    let spf_aligned = (samples_per_frame / ch) * ch;

    let mut chunks = Vec::with_capacity(num_frames);
    let mut pos: usize = 0;

    for frame_idx in 0..num_frames {
        let chunk_len = if frame_idx == num_frames - 1 {
            // Last frame gets all remaining samples.
            samples.len().saturating_sub(pos)
        } else {
            spf_aligned.min(samples.len().saturating_sub(pos))
        };
        let chunk_samples = samples
            .get(pos..pos.saturating_add(chunk_len))
            .unwrap_or(&[]);
        let encoded = aud::encode_adpcm(chunk_samples, stereo);
        chunks.push(encoded);
        pos = pos.saturating_add(chunk_len);
    }

    chunks
}
