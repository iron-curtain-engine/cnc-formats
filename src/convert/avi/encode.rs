// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! AVI encoder — converts raw video frames + PCM audio to RIFF AVI format.

use super::{Error, BYTES_PER_PIXEL, MAX_DIMENSION, MAX_FRAME_COUNT};

// ─── AVI Writer ──────────────────────────────────────────────────────────────

/// Encodes an AVI file from raw video frames and optional PCM audio.
///
/// Video frames are stored as uncompressed bottom-up BGR24 (no codec needed).
/// Audio is stored as raw signed 16-bit PCM.  The output plays in any video
/// player (VLC, mpv, Windows Media Player, etc.).
///
/// # Arguments
///
/// - `frames`: RGBA pixel data per frame (4 bytes/pixel, top-down, `w × h`).
/// - `width`, `height`: frame dimensions.
/// - `fps`: frames per second (e.g. 15).
/// - `audio`: optional PCM audio (signed 16-bit samples, interleaved for
///   stereo).
/// - `sample_rate`: audio sample rate in Hz.
/// - `channels`: number of audio channels (1 or 2).
///
/// # Errors
///
/// - [`Error::InvalidSize`] if dimensions or frame count exceed V38 caps.
pub fn encode_avi<T: AsRef<[u8]>>(
    frames: &[T],
    width: u32,
    height: u32,
    fps: u32,
    audio: Option<&[i16]>,
    sample_rate: u32,
    channels: u16,
) -> Result<Vec<u8>, Error> {
    if frames.is_empty() {
        return Err(Error::DecompressionError {
            reason: "no video frames provided for AVI encoding",
        });
    }
    if frames.len() > MAX_FRAME_COUNT {
        return Err(Error::InvalidSize {
            value: frames.len(),
            limit: MAX_FRAME_COUNT,
            context: "AVI frame count",
        });
    }
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(Error::InvalidSize {
            value: width.max(height) as usize,
            limit: MAX_DIMENSION as usize,
            context: "AVI video dimensions",
        });
    }

    let has_audio = audio.is_some() && sample_rate > 0 && channels > 0;
    let num_frames = frames.len() as u32;

    // Row stride: BGR24, padded to 4-byte boundary per BMP convention.
    let row_bytes = (width as usize).saturating_mul(BYTES_PER_PIXEL);
    let row_stride = (row_bytes.saturating_add(3)) & !3;
    let frame_size = row_stride.saturating_mul(height as usize);

    // Audio parameters.
    let block_align = (channels as u32).saturating_mul(2); // 16-bit = 2 bytes/sample
    let avg_bytes_per_sec = sample_rate.saturating_mul(block_align);
    let audio_bytes: &[u8] = &[]; // placeholder
    let mut audio_raw: Vec<u8> = Vec::new();
    if has_audio {
        if let Some(samples) = audio {
            audio_raw = Vec::with_capacity(samples.len().saturating_mul(2));
            for &s in samples {
                audio_raw.extend_from_slice(&s.to_le_bytes());
            }
        }
    }
    let audio_data: &[u8] = if has_audio { &audio_raw } else { audio_bytes };

    // Distribute PCM by whole sample frames so AVI chunk boundaries track
    // video frame time without dropping the `sample_rate / fps` remainder.
    let suggested_audio_chunk_size = suggested_audio_chunk_size(sample_rate, fps, block_align);

    // ── Build the file ───────────────────────────────────────────────
    // Estimate total size and allocate.
    let num_streams: u32 = if has_audio { 2 } else { 1 };
    let hdrl_size = 4 + 64 + (12 + 64 + 48) + if has_audio { 12 + 64 + 26 } else { 0 };
    let movi_estimated = frames.len().saturating_mul(frame_size + 8)
        + if has_audio {
            audio_data.len() + frames.len() * 8
        } else {
            0
        };
    let idx1_estimated = frames.len().saturating_mul(if has_audio { 32 } else { 16 });
    let total_estimated = 12 + hdrl_size + 8 + movi_estimated + 8 + idx1_estimated + 64;
    let mut out = Vec::with_capacity(total_estimated);

    // RIFF header (patched later).
    out.extend_from_slice(b"RIFF");
    let riff_size_pos = out.len();
    write_u32_le(&mut out, 0); // placeholder
    out.extend_from_slice(b"AVI ");

    // ── LIST 'hdrl' ──────────────────────────────────────────────────
    let hdrl_start = out.len();
    out.extend_from_slice(b"LIST");
    let hdrl_size_pos = out.len();
    write_u32_le(&mut out, 0); // placeholder
    out.extend_from_slice(b"hdrl");

    // ── 'avih' (main AVI header, 56 bytes) ───────────────────────────
    out.extend_from_slice(b"avih");
    write_u32_le(&mut out, 56); // chunk size
    let us_per_frame = if fps > 0 { 1_000_000 / fps } else { 66667 };
    write_u32_le(&mut out, us_per_frame); // dwMicroSecPerFrame
    write_u32_le(&mut out, 0); // dwMaxBytesPerSec
    write_u32_le(&mut out, 0); // dwPaddingGranularity
    write_u32_le(&mut out, 0x10); // dwFlags (AVIF_HASINDEX)
    write_u32_le(&mut out, num_frames); // dwTotalFrames
    write_u32_le(&mut out, 0); // dwInitialFrames
    write_u32_le(&mut out, num_streams); // dwStreams
    write_u32_le(&mut out, frame_size as u32); // dwSuggestedBufferSize
    write_u32_le(&mut out, width); // dwWidth
    write_u32_le(&mut out, height); // dwHeight
    write_u32_le(&mut out, 0); // dwReserved[0]
    write_u32_le(&mut out, 0); // dwReserved[1]
    write_u32_le(&mut out, 0); // dwReserved[2]
    write_u32_le(&mut out, 0); // dwReserved[3]

    // ── LIST 'strl' (video stream) ───────────────────────────────────
    out.extend_from_slice(b"LIST");
    let strl_v_size_pos = out.len();
    write_u32_le(&mut out, 0); // placeholder
    out.extend_from_slice(b"strl");

    // 'strh' (stream header, 56 bytes)
    out.extend_from_slice(b"strh");
    write_u32_le(&mut out, 56);
    out.extend_from_slice(b"vids"); // fccType
    out.extend_from_slice(b"\x00\x00\x00\x00"); // fccHandler (uncompressed DIB)
    write_u32_le(&mut out, 0); // dwFlags
    write_u16_le_buf(&mut out, 0); // wPriority
    write_u16_le_buf(&mut out, 0); // wLanguage
    write_u32_le(&mut out, 0); // dwInitialFrames
    write_u32_le(&mut out, 1); // dwScale
    write_u32_le(&mut out, fps); // dwRate (fps)
    write_u32_le(&mut out, 0); // dwStart
    write_u32_le(&mut out, num_frames); // dwLength
    write_u32_le(&mut out, frame_size as u32); // dwSuggestedBufferSize
    write_u32_le(&mut out, 0xFFFFFFFF); // dwQuality (-1)
    write_u32_le(&mut out, 0); // dwSampleSize
    write_u16_le_buf(&mut out, 0); // rcFrame.left
    write_u16_le_buf(&mut out, 0); // rcFrame.top
    write_u16_le_buf(&mut out, width as u16); // rcFrame.right
    write_u16_le_buf(&mut out, height as u16); // rcFrame.bottom

    // 'strf' (BITMAPINFOHEADER, 40 bytes)
    out.extend_from_slice(b"strf");
    write_u32_le(&mut out, 40);
    write_u32_le(&mut out, 40); // biSize
    write_i32_le(&mut out, width as i32); // biWidth
    write_i32_le(&mut out, height as i32); // biHeight (positive = bottom-up)
    write_u16_le_buf(&mut out, 1); // biPlanes
    write_u16_le_buf(&mut out, 24); // biBitCount (BGR24)
    write_u32_le(&mut out, 0); // biCompression (BI_RGB)
    write_u32_le(&mut out, frame_size as u32); // biSizeImage
    write_u32_le(&mut out, 0); // biXPelsPerMeter
    write_u32_le(&mut out, 0); // biYPelsPerMeter
    write_u32_le(&mut out, 0); // biClrUsed
    write_u32_le(&mut out, 0); // biClrImportant

    // Patch video strl size.
    let strl_v_end = out.len();
    patch_u32_le(
        &mut out,
        strl_v_size_pos,
        (strl_v_end - strl_v_size_pos - 4) as u32,
    );

    // ── LIST 'strl' (audio stream, optional) ─────────────────────────
    if has_audio {
        out.extend_from_slice(b"LIST");
        let strl_a_size_pos = out.len();
        write_u32_le(&mut out, 0); // placeholder
        out.extend_from_slice(b"strl");

        // 'strh' (audio stream header, 56 bytes)
        out.extend_from_slice(b"strh");
        write_u32_le(&mut out, 56);
        out.extend_from_slice(b"auds"); // fccType
        write_u32_le(&mut out, 1); // fccHandler (PCM)
        write_u32_le(&mut out, 0); // dwFlags
        write_u16_le_buf(&mut out, 0); // wPriority
        write_u16_le_buf(&mut out, 0); // wLanguage
        write_u32_le(&mut out, 0); // dwInitialFrames
        write_u32_le(&mut out, block_align); // dwScale
        write_u32_le(&mut out, avg_bytes_per_sec); // dwRate
        write_u32_le(&mut out, 0); // dwStart
        write_u32_le(&mut out, (audio_data.len() / block_align as usize) as u32); // dwLength
        write_u32_le(&mut out, suggested_audio_chunk_size.max(4096)); // dwSuggestedBufferSize
        write_u32_le(&mut out, 0xFFFFFFFF); // dwQuality
        write_u32_le(&mut out, block_align); // dwSampleSize
        write_u16_le_buf(&mut out, 0); // rcFrame
        write_u16_le_buf(&mut out, 0);
        write_u16_le_buf(&mut out, 0);
        write_u16_le_buf(&mut out, 0);

        // 'strf' (WAVEFORMATEX, 18 bytes)
        out.extend_from_slice(b"strf");
        write_u32_le(&mut out, 18);
        write_u16_le_buf(&mut out, 1); // wFormatTag (PCM)
        write_u16_le_buf(&mut out, channels); // nChannels
        write_u32_le(&mut out, sample_rate); // nSamplesPerSec
        write_u32_le(&mut out, avg_bytes_per_sec); // nAvgBytesPerSec
        write_u16_le_buf(&mut out, block_align as u16); // nBlockAlign
        write_u16_le_buf(&mut out, 16); // wBitsPerSample
        write_u16_le_buf(&mut out, 0); // cbSize (extra data)

        let strl_a_end = out.len();
        patch_u32_le(
            &mut out,
            strl_a_size_pos,
            (strl_a_end - strl_a_size_pos - 4) as u32,
        );
    }

    // Patch hdrl size.
    let hdrl_end = out.len();
    patch_u32_le(&mut out, hdrl_size_pos, (hdrl_end - hdrl_start - 8) as u32);

    // ── LIST 'movi' ──────────────────────────────────────────────────
    out.extend_from_slice(b"LIST");
    let movi_size_pos = out.len();
    write_u32_le(&mut out, 0); // placeholder
    out.extend_from_slice(b"movi");
    let movi_start = out.len() - 4; // offset for idx1

    // Index entries (built while writing movi).
    let mut idx1_entries: Vec<[u8; 16]> =
        Vec::with_capacity(frames.len().saturating_mul(if has_audio { 2 } else { 1 }));

    // Audio position tracker for per-frame interleaving.
    let mut audio_pos: usize = 0;
    let mut audio_frame_remainder: u64 = 0;

    for rgba in frames {
        let rgba = rgba.as_ref();
        // ── Audio chunk (before video, for better A/V sync) ──────
        if has_audio && !audio_data.is_empty() {
            let remaining = audio_data.len().saturating_sub(audio_pos);
            let aligned = next_audio_chunk_size(
                sample_rate,
                fps,
                block_align as usize,
                &mut audio_frame_remainder,
                remaining,
            );

            if aligned > 0 {
                let chunk_offset = out.len() - movi_start;
                let audio_slice = audio_data
                    .get(audio_pos..audio_pos.saturating_add(aligned))
                    .unwrap_or(&[]);

                out.extend_from_slice(b"01wb");
                write_u32_le(&mut out, audio_slice.len() as u32);
                out.extend_from_slice(audio_slice);
                // Pad to even.
                if audio_slice.len() & 1 != 0 {
                    out.push(0);
                }

                idx1_entries.push(make_idx1_entry(
                    b"01wb",
                    0x10, // AVIIF_KEYFRAME
                    chunk_offset as u32,
                    audio_slice.len() as u32,
                ));
                audio_pos = audio_pos.saturating_add(aligned);
            }
        }

        // ── Video chunk ──────────────────────────────────────────
        let chunk_offset = out.len() - movi_start;
        out.extend_from_slice(b"00dc");
        write_u32_le(&mut out, frame_size as u32);

        // Convert RGBA (top-down) → BGR24 (bottom-up, padded rows).
        let pad_bytes = row_stride - row_bytes;
        for row in (0..height as usize).rev() {
            // Bottom-up: last row first.
            for col in 0..width as usize {
                let src = row
                    .saturating_mul(width as usize)
                    .saturating_add(col)
                    .saturating_mul(4);
                let r = rgba.get(src).copied().unwrap_or(0);
                let g = rgba.get(src.saturating_add(1)).copied().unwrap_or(0);
                let b = rgba.get(src.saturating_add(2)).copied().unwrap_or(0);
                // BGR order for BMP/AVI.
                out.push(b);
                out.push(g);
                out.push(r);
            }
            // Row padding to 4-byte boundary.
            out.resize(out.len().saturating_add(pad_bytes), 0);
        }

        idx1_entries.push(make_idx1_entry(
            b"00dc",
            0x10, // AVIIF_KEYFRAME (all frames are keyframes for uncompressed)
            chunk_offset as u32,
            frame_size as u32,
        ));
    }

    // Write remaining audio as a final chunk (if any left over).
    if has_audio && audio_pos < audio_data.len() {
        let remaining = audio_data.get(audio_pos..).unwrap_or(&[]);
        if !remaining.is_empty() {
            let chunk_offset = out.len() - movi_start;
            out.extend_from_slice(b"01wb");
            write_u32_le(&mut out, remaining.len() as u32);
            out.extend_from_slice(remaining);
            if remaining.len() & 1 != 0 {
                out.push(0);
            }
            idx1_entries.push(make_idx1_entry(
                b"01wb",
                0x10,
                chunk_offset as u32,
                remaining.len() as u32,
            ));
        }
    }

    // Patch movi size.
    let movi_end = out.len();
    patch_u32_le(
        &mut out,
        movi_size_pos,
        (movi_end - movi_size_pos - 4) as u32,
    );

    // ── 'idx1' (index) ──────────────────────────────────────────────
    out.extend_from_slice(b"idx1");
    let idx1_size = idx1_entries.len().saturating_mul(16);
    write_u32_le(&mut out, idx1_size as u32);
    for entry in &idx1_entries {
        out.extend_from_slice(entry);
    }

    // Patch RIFF size.
    let total_size = out.len();
    patch_u32_le(&mut out, riff_size_pos, (total_size - 8) as u32);

    Ok(out)
}

/// Returns the largest per-frame PCM chunk size the encoder will emit.
///
/// Why: AVI stream headers use `dwSuggestedBufferSize` as a decoder hint.
/// It should reflect one frame's worth of audio, not an arbitrary half-second
/// burst that can overstate buffering needs.
fn suggested_audio_chunk_size(sample_rate: u32, fps: u32, block_align: u32) -> u32 {
    if sample_rate == 0 || fps == 0 || block_align == 0 {
        return 0;
    }

    let sample_frames = (sample_rate as u64)
        .saturating_add(fps as u64)
        .saturating_sub(1)
        / fps as u64;
    sample_frames.saturating_mul(block_align as u64) as u32
}

/// Returns the next audio chunk size in bytes for one video frame.
///
/// Why: `sample_rate / fps` is often fractional.  Carrying the remainder
/// forward preserves exact timing across the stream instead of dropping the
/// fractional sample frame every time.
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

// ─── Write Helpers ───────────────────────────────────────────────────────────

/// Writes a little-endian u32 to the output buffer.
#[inline]
fn write_u32_le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Writes a little-endian i32 to the output buffer.
#[inline]
fn write_i32_le(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Writes a little-endian u16 to the output buffer.
#[inline]
fn write_u16_le_buf(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Patches a u32 LE value at a given position in the buffer.
#[inline]
fn patch_u32_le(buf: &mut [u8], pos: usize, value: u32) {
    let bytes = value.to_le_bytes();
    if let Some(slice) = buf.get_mut(pos..pos.saturating_add(4)) {
        slice.copy_from_slice(&bytes);
    }
}

/// Creates an idx1 index entry (16 bytes).
fn make_idx1_entry(fourcc: &[u8; 4], flags: u32, offset: u32, size: u32) -> [u8; 16] {
    let mut entry = [0u8; 16];
    if let Some(s) = entry.get_mut(0..4) {
        s.copy_from_slice(fourcc);
    }
    if let Some(s) = entry.get_mut(4..8) {
        s.copy_from_slice(&flags.to_le_bytes());
    }
    if let Some(s) = entry.get_mut(8..12) {
        s.copy_from_slice(&offset.to_le_bytes());
    }
    if let Some(s) = entry.get_mut(12..16) {
        s.copy_from_slice(&size.to_le_bytes());
    }
    entry
}
