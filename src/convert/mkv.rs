// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Matroska (MKV) encoder — muxes raw video frames + PCM audio into an MKV
//! container with `A_PCM/INT/LIT` (signed 16-bit little-endian PCM) audio.
//!
//! Two video codecs are supported (selectable via [`MkvVideoCodec`]):
//!
//! - **`V_UNCOMPRESSED`** (default) — native Matroska uncompressed video per
//!   RFC 9559.  BGR24 top-down.  Requires a modern player (ffplay, mpv ≥ 0.37).
//! - **`V_MS/VFW/FOURCC`** (compat) — legacy Video for Windows mapping with a
//!   40-byte BITMAPINFOHEADER as CodecPrivate.  BGR24 bottom-up.  Plays in
//!   VLC 3.x, Windows Media Player, and every other legacy player.
//!
//! No external dependencies — EBML encoding is implemented inline.
//!
//! ## MKV Structure
//!
//! ```text
//! EBML Header (DocType "matroska")
//! Segment
//!   Info (TimestampScale, Duration, MuxingApp, WritingApp)
//!   Tracks
//!     TrackEntry 1 — Video (V_UNCOMPRESSED or V_MS/VFW/FOURCC, BGR24)
//!     TrackEntry 2 — Audio (A_PCM/INT/LIT, 16-bit) [optional]
//!   Cluster (absolute timestamp)
//!     SimpleBlock — video frame (keyframe, BGR24)
//!     SimpleBlock — audio chunk (PCM)
//!   Cluster …
//! ```
//!
//! ## References
//!
//! - IETF RFC 8794 (EBML), RFC 9559 (Matroska container)
//! - Matroska Codec Specifications: `V_UNCOMPRESSED`, `V_MS/VFW/FOURCC`,
//!   `A_PCM/INT/LIT`

use crate::error::Error;

// ─── Limits ─────────────────────────────────────────────────────────────────

/// V38: maximum video frame count.
const MAX_FRAME_COUNT: usize = 65536;

/// V38: maximum single video dimension.
const MAX_DIMENSION: u32 = 4096;

/// Bytes per pixel for BGR24 output.
const BYTES_PER_PIXEL: usize = 3;

// ─── EBML Element IDs (Matroska spec) ───────────────────────────────────────

const EBML_ID: u32 = 0x1A45_DFA3;
const EBML_VERSION: u32 = 0x4286;
const EBML_READ_VERSION: u32 = 0x42F7;
const EBML_MAX_ID_LENGTH: u32 = 0x42F2;
const EBML_MAX_SIZE_LENGTH: u32 = 0x42F3;
const DOC_TYPE: u32 = 0x4282;
const DOC_TYPE_VERSION: u32 = 0x4287;
const DOC_TYPE_READ_VERSION: u32 = 0x4285;

const SEGMENT: u32 = 0x1853_8067;
const INFO: u32 = 0x1549_A966;
const TIMESTAMP_SCALE: u32 = 0x2A_D7B1;
const DURATION_ID: u32 = 0x4489;
const MUXING_APP: u32 = 0x4D80;
const WRITING_APP: u32 = 0x5741;

const TRACKS: u32 = 0x1654_AE6B;
const TRACK_ENTRY: u32 = 0xAE;
const TRACK_NUMBER: u32 = 0xD7;
const TRACK_UID: u32 = 0x73C5;
const TRACK_TYPE: u32 = 0x83;
const FLAG_LACING: u32 = 0x9C;
const DEFAULT_DURATION: u32 = 0x23_E383;
const CODEC_ID: u32 = 0x86;
const CODEC_PRIVATE: u32 = 0x63A2;

const VIDEO_ID: u32 = 0xE0;
const PIXEL_WIDTH: u32 = 0xB0;
const PIXEL_HEIGHT: u32 = 0xBA;
const UNCOMPRESSED_FOURCC: u32 = 0x2E_B524;

const AUDIO_ID: u32 = 0xE1;
const SAMPLING_FREQUENCY: u32 = 0xB5;
const CHANNELS: u32 = 0x9F;
const BIT_DEPTH: u32 = 0x6264;

const CLUSTER: u32 = 0x1F43_B675;
const TIMESTAMP_ID: u32 = 0xE7;
const SIMPLE_BLOCK: u32 = 0xA3;

// ─── Video Codec Selection ──────────────────────────────────────────────────

/// Video codec for MKV output.
///
/// Controls which Matroska video codec mapping is used when muxing raw frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MkvVideoCodec {
    /// `V_UNCOMPRESSED` — native Matroska uncompressed video (RFC 9559).
    ///
    /// BGR24 top-down.  The modern, spec-correct codec mapping.  Requires a
    /// player that implements RFC 9559 `V_UNCOMPRESSED` (ffplay, mpv ≥ 0.37).
    /// VLC 3.x does **not** support this codec.
    #[default]
    Uncompressed,

    /// `V_MS/VFW/FOURCC` with BITMAPINFOHEADER — maximum player compatibility.
    ///
    /// BGR24 bottom-up per BMP convention.  Uses the legacy Video for Windows
    /// codec mapping understood by VLC, ffplay, mpv, Windows Media Player,
    /// and all major players.
    Vfw,
}

// ─── Audio Configuration ────────────────────────────────────────────────────

/// Audio stream configuration for MKV encoding.
///
/// Groups the PCM audio samples with their format metadata.  Passed as
/// `Option<&MkvAudio>` to [`encode_mkv`] — `None` produces a video-only file.
#[derive(Debug, Clone)]
pub struct MkvAudio<'audio> {
    /// Signed 16-bit PCM samples (interleaved for stereo: `[L₀, R₀, L₁, …]`).
    pub samples: &'audio [i16],
    /// Sample rate in Hz (e.g. 22050).
    pub sample_rate: u32,
    /// Number of audio channels (1 = mono, 2 = stereo).
    pub channels: u16,
}

// ─── Public API ─────────────────────────────────────────────────────────────

/// Encodes an MKV file from raw video frames and optional PCM audio.
///
/// Video frames are stored using the codec specified by `video_codec`:
/// - [`MkvVideoCodec::Uncompressed`]: `V_UNCOMPRESSED` BGR24 top-down
/// - [`MkvVideoCodec::Vfw`]: `V_MS/VFW/FOURCC` BGR24 bottom-up
///
/// Audio is stored as `A_PCM/INT/LIT` (signed 16-bit LE).
///
/// # Arguments
///
/// - `frames`: RGBA pixel data per frame (4 bytes/pixel, top-down, `w × h`).
/// - `width`, `height`: frame dimensions.
/// - `fps`: frames per second (e.g. 15).
/// - `audio`: optional PCM audio configuration.
/// - `video_codec`: codec selection ([`MkvVideoCodec::Uncompressed`] or
///   [`MkvVideoCodec::Vfw`]).
///
/// # Errors
///
/// - [`Error::InvalidSize`] if dimensions or frame count exceed V38 caps.
pub fn encode_mkv<T: AsRef<[u8]>>(
    frames: &[T],
    width: u32,
    height: u32,
    fps: u32,
    audio: Option<&MkvAudio<'_>>,
    video_codec: MkvVideoCodec,
) -> Result<Vec<u8>, Error> {
    if frames.is_empty() {
        return Err(Error::DecompressionError {
            reason: "no video frames provided for MKV encoding",
        });
    }
    if frames.len() > MAX_FRAME_COUNT {
        return Err(Error::InvalidSize {
            value: frames.len(),
            limit: MAX_FRAME_COUNT,
            context: "MKV frame count",
        });
    }
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(Error::InvalidSize {
            value: width.max(height) as usize,
            limit: MAX_DIMENSION as usize,
            context: "MKV video dimensions",
        });
    }

    // Extract audio parameters (if present and valid).
    let (sample_rate, channels) = audio
        .filter(|a| a.sample_rate > 0 && a.channels > 0)
        .map(|a| (a.sample_rate, a.channels))
        .unwrap_or((0, 0));
    let has_audio = audio.is_some() && sample_rate > 0 && channels > 0;
    let fps = fps.max(1);

    // Convert audio samples to raw bytes.
    let audio_bytes: Vec<u8> = if has_audio {
        audio
            .map(|a| a.samples)
            .unwrap_or(&[])
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect()
    } else {
        Vec::new()
    };
    let block_align = (channels as usize).saturating_mul(2);

    // Estimate output size to reduce re-allocations.
    let frame_bgr_size = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(BYTES_PER_PIXEL);
    let estimated = frames
        .len()
        .saturating_mul(frame_bgr_size.saturating_add(16))
        .saturating_add(audio_bytes.len())
        .saturating_add(1024);
    let mut out = Vec::with_capacity(estimated);

    // ── EBML Header ────────────────────────────────────────────────────
    let mut ebml_children = Vec::new();
    write_uint_element(&mut ebml_children, EBML_VERSION, 1);
    write_uint_element(&mut ebml_children, EBML_READ_VERSION, 1);
    write_uint_element(&mut ebml_children, EBML_MAX_ID_LENGTH, 4);
    write_uint_element(&mut ebml_children, EBML_MAX_SIZE_LENGTH, 8);
    write_string_element(&mut ebml_children, DOC_TYPE, "matroska");
    write_uint_element(&mut ebml_children, DOC_TYPE_VERSION, 4);
    write_uint_element(&mut ebml_children, DOC_TYPE_READ_VERSION, 2);
    write_master_element(&mut out, EBML_ID, &ebml_children);

    // ── Segment (size patched at the end) ──────────────────────────────
    write_element_id(&mut out, SEGMENT);
    let segment_size_pos = out.len();
    write_unknown_size_placeholder(&mut out);
    let segment_data_start = out.len();

    // ── Segment Info ───────────────────────────────────────────────────
    let duration_ms = (frames.len() as f64) * 1000.0 / (fps as f64);
    let mut info_buf = Vec::new();
    write_uint_element(&mut info_buf, TIMESTAMP_SCALE, 1_000_000); // 1 ms per tick
    write_float_element(&mut info_buf, DURATION_ID, duration_ms);
    write_string_element(&mut info_buf, MUXING_APP, "cnc-formats");
    write_string_element(&mut info_buf, WRITING_APP, "cnc-formats");
    write_master_element(&mut out, INFO, &info_buf);

    // ── Tracks ─────────────────────────────────────────────────────────
    let frame_duration_ns = 1_000_000_000u64 / (fps as u64);

    let mut tracks_buf = Vec::new();

    // Video track (track 1).
    let mut video_sub = Vec::new();
    write_uint_element(&mut video_sub, PIXEL_WIDTH, width as u64);
    write_uint_element(&mut video_sub, PIXEL_HEIGHT, height as u64);

    let mut track1 = Vec::new();
    write_uint_element(&mut track1, TRACK_NUMBER, 1);
    write_uint_element(&mut track1, TRACK_UID, 1);
    write_uint_element(&mut track1, TRACK_TYPE, 1); // video
    write_uint_element(&mut track1, FLAG_LACING, 0);
    write_uint_element(&mut track1, DEFAULT_DURATION, frame_duration_ns);

    match video_codec {
        MkvVideoCodec::Uncompressed => {
            // V_UNCOMPRESSED — native Matroska uncompressed video (RFC 9559).
            // BI_RGB FourCC (all zeros) declares BGR24 top-down.
            write_binary_element(&mut video_sub, UNCOMPRESSED_FOURCC, &[0, 0, 0, 0]);
            write_string_element(&mut track1, CODEC_ID, "V_UNCOMPRESSED");
        }
        MkvVideoCodec::Vfw => {
            // V_MS/VFW/FOURCC — legacy VFW mapping for broad player compat.
            // BITMAPINFOHEADER (40 bytes) as CodecPrivate: BI_RGB BGR24
            // bottom-up (positive biHeight = bottom-to-top row order).
            let bih = build_bitmapinfoheader(width, height, frame_bgr_size as u32);
            write_string_element(&mut track1, CODEC_ID, "V_MS/VFW/FOURCC");
            write_binary_element(&mut track1, CODEC_PRIVATE, &bih);
        }
    }

    write_master_element(&mut track1, VIDEO_ID, &video_sub);
    write_master_element(&mut tracks_buf, TRACK_ENTRY, &track1);

    // Audio track (track 2, optional).
    if has_audio {
        let mut audio_sub = Vec::new();
        write_float_element(&mut audio_sub, SAMPLING_FREQUENCY, sample_rate as f64);
        write_uint_element(&mut audio_sub, CHANNELS, channels as u64);
        write_uint_element(&mut audio_sub, BIT_DEPTH, 16);

        let mut track2 = Vec::new();
        write_uint_element(&mut track2, TRACK_NUMBER, 2);
        write_uint_element(&mut track2, TRACK_UID, 2);
        write_uint_element(&mut track2, TRACK_TYPE, 2); // audio
        write_string_element(&mut track2, CODEC_ID, "A_PCM/INT/LIT");
        write_master_element(&mut track2, AUDIO_ID, &audio_sub);
        write_master_element(&mut tracks_buf, TRACK_ENTRY, &track2);
    }

    write_master_element(&mut out, TRACKS, &tracks_buf);

    // ── Clusters ───────────────────────────────────────────────────────
    let ms_per_frame = 1000.0 / (fps as f64);
    let max_cluster_offset: i64 = 30_000; // start a new cluster every ~30 s

    let mut audio_pos: usize = 0;
    let mut audio_remainder: u64 = 0;
    let mut cluster_start_ms: u64 = 0;
    let mut cluster_buf = Vec::new();
    let mut in_cluster = false;

    for (i, rgba) in frames.iter().enumerate() {
        let frame_ms = ((i as f64) * ms_per_frame) as u64;
        let offset_from_cluster = frame_ms.saturating_sub(cluster_start_ms) as i64;

        // Start a new cluster when the offset would exceed the limit.
        if !in_cluster || offset_from_cluster > max_cluster_offset {
            if in_cluster {
                write_master_element(&mut out, CLUSTER, &cluster_buf);
                cluster_buf.clear();
            }
            cluster_start_ms = frame_ms;
            in_cluster = true;
            write_uint_element(&mut cluster_buf, TIMESTAMP_ID, cluster_start_ms);
        }

        let block_offset = frame_ms.saturating_sub(cluster_start_ms) as i16;

        // Video SimpleBlock — RGBA → BGR24.  VFW uses bottom-up row order
        // per BMP convention; V_UNCOMPRESSED uses Matroska top-down order.
        let bottom_up = video_codec == MkvVideoCodec::Vfw;
        let bgr = rgba_to_bgr24(rgba.as_ref(), width, height, bottom_up);
        write_simple_block(&mut cluster_buf, 1, block_offset, &bgr);

        // Audio SimpleBlock (interleaved with video for A/V sync).
        if has_audio && audio_pos < audio_bytes.len() {
            let remaining = audio_bytes.len().saturating_sub(audio_pos);
            let chunk_size = next_audio_chunk_size(
                sample_rate,
                fps,
                block_align,
                &mut audio_remainder,
                remaining,
            );
            if chunk_size > 0 {
                let audio_chunk = audio_bytes
                    .get(audio_pos..audio_pos.saturating_add(chunk_size))
                    .unwrap_or(&[]);
                write_simple_block(&mut cluster_buf, 2, block_offset, audio_chunk);
                audio_pos = audio_pos.saturating_add(chunk_size);
            }
        }
    }

    // Flush the last open cluster.
    if in_cluster && !cluster_buf.is_empty() {
        write_master_element(&mut out, CLUSTER, &cluster_buf);
    }

    // Write any remaining audio as a trailing cluster.
    if has_audio && audio_pos < audio_bytes.len() {
        let remaining = audio_bytes.get(audio_pos..).unwrap_or(&[]);
        if !remaining.is_empty() {
            let mut tail = Vec::new();
            let tail_ts = ((frames.len() as f64) * ms_per_frame) as u64;
            write_uint_element(&mut tail, TIMESTAMP_ID, tail_ts);
            write_simple_block(&mut tail, 2, 0, remaining);
            write_master_element(&mut out, CLUSTER, &tail);
        }
    }

    // Patch the Segment size now that we know the total.
    let segment_data_size = out.len().saturating_sub(segment_data_start);
    patch_8byte_vint(&mut out, segment_size_pos, segment_data_size);

    Ok(out)
}

// ─── BITMAPINFOHEADER Builder ────────────────────────────────────────────────

/// Builds a 40-byte BITMAPINFOHEADER for BI_RGB BGR24 (bottom-up).
///
/// Fields are written sequentially via `extend_from_slice` — no direct
/// array indexing.  The resulting header is used as `CodecPrivate` for
/// the `V_MS/VFW/FOURCC` video codec.
fn build_bitmapinfoheader(width: u32, height: u32, size_image: u32) -> Vec<u8> {
    let mut bih = Vec::with_capacity(40);
    bih.extend_from_slice(&40u32.to_le_bytes()); // biSize
    bih.extend_from_slice(&width.to_le_bytes()); // biWidth
    bih.extend_from_slice(&height.to_le_bytes()); // biHeight (positive = bottom-up)
    bih.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    bih.extend_from_slice(&24u16.to_le_bytes()); // biBitCount
    bih.extend_from_slice(&0u32.to_le_bytes()); // biCompression (BI_RGB = 0)
    bih.extend_from_slice(&size_image.to_le_bytes()); // biSizeImage
    bih.extend_from_slice(&0u32.to_le_bytes()); // biXPelsPerMeter
    bih.extend_from_slice(&0u32.to_le_bytes()); // biYPelsPerMeter
    bih.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    bih.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant
    bih
}

// ─── Pixel Conversion ───────────────────────────────────────────────────────

/// Converts RGBA (top-down) to BGR24.
///
/// When `bottom_up` is true, rows are flipped to bottom-to-top order (BMP / VFW
/// convention with positive `biHeight`).  When false, rows stay top-down
/// (Matroska `V_UNCOMPRESSED` convention).
fn rgba_to_bgr24(rgba: &[u8], width: u32, height: u32, bottom_up: bool) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let pixel_count = w.saturating_mul(h);
    let mut bgr = vec![0u8; pixel_count.saturating_mul(BYTES_PER_PIXEL)];
    let row_bytes = w.saturating_mul(BYTES_PER_PIXEL);
    for y in 0..h {
        let src_row_start = y.saturating_mul(w);
        // VFW bottom-up: flip row y → row (h-1-y).  Otherwise keep top-down.
        let dst_y = if bottom_up {
            h.saturating_sub(1).saturating_sub(y)
        } else {
            y
        };
        let dst_row_start = dst_y.saturating_mul(row_bytes);
        for x in 0..w {
            let src_base = src_row_start.saturating_add(x).saturating_mul(4);
            let dst_base = dst_row_start.saturating_add(x.saturating_mul(BYTES_PER_PIXEL));
            let r = rgba.get(src_base).copied().unwrap_or(0);
            let g = rgba.get(src_base.saturating_add(1)).copied().unwrap_or(0);
            let b = rgba.get(src_base.saturating_add(2)).copied().unwrap_or(0);
            if let Some(dst) = bgr.get_mut(dst_base..dst_base.saturating_add(3)) {
                dst.copy_from_slice(&[b, g, r]);
            }
        }
    }
    bgr
}

// ─── Audio Helpers ──────────────────────────────────────────────────────────

/// Returns the next audio chunk size in bytes for one video frame.
///
/// Carries the fractional remainder forward to preserve exact timing.
fn next_audio_chunk_size(
    sample_rate: u32,
    fps: u32,
    block_align: usize,
    remainder: &mut u64,
    remaining_bytes: usize,
) -> usize {
    if sample_rate == 0 || fps == 0 || block_align == 0 || remaining_bytes < block_align {
        return 0;
    }
    *remainder = (*remainder).saturating_add(sample_rate as u64);
    let sample_frames = (*remainder / fps as u64) as usize;
    *remainder %= fps as u64;
    let available_frames = remaining_bytes / block_align;
    sample_frames
        .min(available_frames)
        .saturating_mul(block_align)
}

// ─── SimpleBlock Writer ─────────────────────────────────────────────────────

/// Writes a SimpleBlock element for a single track.
///
/// Layout: `[0xA3] [vint_size] [track_vint] [ts_be_i16] [flags] [payload]`
fn write_simple_block(out: &mut Vec<u8>, track: u8, timestamp_offset: i16, data: &[u8]) {
    let block_data_size = 1usize
        .saturating_add(2)
        .saturating_add(1)
        .saturating_add(data.len());
    write_element_id(out, SIMPLE_BLOCK);
    write_vint_size(out, block_data_size);
    out.push(0x80 | track); // VINT-encoded track number (1-byte, tracks 1–127)
    out.extend_from_slice(&timestamp_offset.to_be_bytes());
    out.push(0x80); // flags: keyframe, no lacing, not invisible, not discardable
    out.extend_from_slice(data);
}

// ─── EBML Encoding Primitives ───────────────────────────────────────────────

/// Writes a variable-length EBML element ID.
///
/// IDs are big-endian with the VINT marker bit baked into the most significant
/// byte:
/// - `1xxxxxxx` → 1 byte
/// - `01xxxxxx …` → 2 bytes
/// - `001xxxxx …` → 3 bytes
/// - `0001xxxx …` → 4 bytes
#[inline]
fn write_element_id(out: &mut Vec<u8>, id: u32) {
    let bytes = id.to_be_bytes();
    if id >= 0x1000_0000 {
        out.extend_from_slice(&bytes);
    } else if id >= 0x0020_0000 {
        out.extend_from_slice(bytes.get(1..).unwrap_or(&[]));
    } else if id >= 0x0000_4000 {
        out.extend_from_slice(bytes.get(2..).unwrap_or(&[]));
    } else {
        out.push(bytes.get(3).copied().unwrap_or(0));
    }
}

/// Writes a VINT-encoded size value (1–8 bytes).
///
/// The leading byte contains a VINT marker bit that indicates the total byte
/// width.  The all-bits-1 pattern at each width is reserved for "unknown size"
/// and is never used for a concrete value.
#[inline]
fn write_vint_size(out: &mut Vec<u8>, size: usize) {
    if size <= 126 {
        out.push(0x80 | size as u8);
    } else if size <= 16382 {
        out.push(0x40 | ((size >> 8) as u8));
        out.push(size as u8);
    } else if size <= 2_097_150 {
        out.push(0x20 | ((size >> 16) as u8));
        out.push((size >> 8) as u8);
        out.push(size as u8);
    } else if size <= 268_435_454 {
        out.push(0x10 | ((size >> 24) as u8));
        out.push((size >> 16) as u8);
        out.push((size >> 8) as u8);
        out.push(size as u8);
    } else {
        // 8-byte VINT for very large sizes (>256 MB).
        let s = size as u64;
        out.push(0x01);
        out.extend_from_slice(s.to_be_bytes().get(1..).unwrap_or(&[]));
    }
}

/// Writes an 8-byte "unknown size" placeholder (`0x01FF_FFFF_FFFF_FFFF`).
#[inline]
fn write_unknown_size_placeholder(out: &mut Vec<u8>) {
    out.extend_from_slice(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
}

/// Overwrites an 8-byte VINT placeholder at `pos` with the actual size.
///
/// Builds the 8-byte VINT (0x01 marker + 7 big-endian size bytes) as an array,
/// then writes it in one `copy_from_slice` — no direct indexing.
fn patch_8byte_vint(buf: &mut [u8], pos: usize, size: usize) {
    if let Some(dst) = buf.get_mut(pos..pos.saturating_add(8)) {
        let be = (size as u64).to_be_bytes();
        // VINT marker 0x01 followed by the lower 7 bytes of the big-endian size.
        let vint = [
            0x01,
            be.get(1).copied().unwrap_or(0),
            be.get(2).copied().unwrap_or(0),
            be.get(3).copied().unwrap_or(0),
            be.get(4).copied().unwrap_or(0),
            be.get(5).copied().unwrap_or(0),
            be.get(6).copied().unwrap_or(0),
            be.get(7).copied().unwrap_or(0),
        ];
        dst.copy_from_slice(&vint);
    }
}

/// Writes a UINT element (variable-length big-endian, minimal encoding).
fn write_uint_element(out: &mut Vec<u8>, id: u32, value: u64) {
    write_element_id(out, id);
    let bytes = value.to_be_bytes();
    let first_nonzero = bytes.iter().position(|&b| b != 0).unwrap_or(7);
    let len = (8usize.saturating_sub(first_nonzero)).max(1);
    write_vint_size(out, len);
    out.extend_from_slice(bytes.get(8usize.saturating_sub(len)..).unwrap_or(&[]));
}

/// Writes a FLOAT element (always 8 bytes, big-endian IEEE 754).
fn write_float_element(out: &mut Vec<u8>, id: u32, value: f64) {
    write_element_id(out, id);
    write_vint_size(out, 8);
    out.extend_from_slice(&value.to_be_bytes());
}

/// Writes a UTF-8 STRING element.
fn write_string_element(out: &mut Vec<u8>, id: u32, value: &str) {
    write_element_id(out, id);
    write_vint_size(out, value.len());
    out.extend_from_slice(value.as_bytes());
}

/// Writes a BINARY element.
fn write_binary_element(out: &mut Vec<u8>, id: u32, value: &[u8]) {
    write_element_id(out, id);
    write_vint_size(out, value.len());
    out.extend_from_slice(value);
}

/// Writes a MASTER element whose children are already serialized.
fn write_master_element(out: &mut Vec<u8>, id: u32, children: &[u8]) {
    write_element_id(out, id);
    write_vint_size(out, children.len());
    out.extend_from_slice(children);
}
