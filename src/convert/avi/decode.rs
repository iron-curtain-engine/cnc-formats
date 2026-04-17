// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! AVI decoder — extracts uncompressed video frames + PCM audio from RIFF AVI.

use super::{Error, MAX_AVI_SIZE, MAX_DIMENSION};

// ─── AVI Reader ──────────────────────────────────────────────────────────────

/// Decoded AVI content: video frames + optional audio.
#[derive(Debug)]
pub struct AviContent {
    /// RGBA pixel data per frame (4 bytes/pixel, top-down, `w × h`).
    pub frames: Vec<Vec<u8>>,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Frames per second.
    pub fps: u32,
    /// PCM audio samples (signed 16-bit, interleaved for stereo).  Empty if
    /// no audio.
    pub audio: Vec<i16>,
    /// Audio sample rate (0 if no audio).
    pub sample_rate: u32,
    /// Audio channels (0 if no audio).
    pub channels: u16,
}

/// Decodes an AVI file, extracting uncompressed video frames and PCM audio.
///
/// Only supports uncompressed RGB/BGR video (BI_RGB) and PCM audio.
/// Compressed AVI files (DivX, MJPEG, etc.) are rejected.
///
/// # Errors
///
/// - [`Error::InvalidMagic`] if not a valid RIFF AVI file.
/// - [`Error::DecompressionError`] if video is compressed (not BI_RGB).
/// - [`Error::InvalidSize`] if dimensions or frame count exceed V38 caps.
pub fn decode_avi(data: &[u8]) -> Result<AviContent, Error> {
    if data.len() > MAX_AVI_SIZE {
        return Err(Error::InvalidSize {
            value: data.len(),
            limit: MAX_AVI_SIZE,
            context: "AVI file size",
        });
    }
    if data.len() < 12 {
        return Err(Error::UnexpectedEof {
            needed: 12,
            available: data.len(),
        });
    }

    // Validate RIFF 'AVI ' header.
    let riff_tag = data.get(0..4).ok_or(Error::UnexpectedEof {
        needed: 4,
        available: data.len(),
    })?;
    if riff_tag != b"RIFF" {
        return Err(Error::InvalidMagic {
            context: "AVI RIFF header",
        });
    }
    let avi_tag = data.get(8..12).ok_or(Error::UnexpectedEof {
        needed: 12,
        available: data.len(),
    })?;
    if avi_tag != b"AVI " {
        return Err(Error::InvalidMagic {
            context: "AVI form type",
        });
    }

    // Parse RIFF chunks to find hdrl and movi.
    let mut width: u32 = 0;
    let mut height: u32 = 0;
    let mut fps: u32 = 15;
    let mut bit_count: u16 = 24;
    let mut compression: u32 = 0;
    let mut sample_rate: u32 = 0;
    let mut channels: u16 = 0;
    let mut audio_bits: u16 = 16;

    let mut video_frames: Vec<Vec<u8>> = Vec::new();
    let mut audio_samples: Vec<i16> = Vec::new();

    // Walk RIFF sub-chunks.
    let mut pos: usize = 12;
    while pos.saturating_add(8) <= data.len() {
        let fourcc = data.get(pos..pos.saturating_add(4)).unwrap_or(b"\0\0\0\0");
        let chunk_size = read_riff_u32(data, pos.saturating_add(4))? as usize;
        let payload_start = pos.saturating_add(8);
        let payload_end = payload_start.saturating_add(chunk_size);

        match fourcc {
            b"LIST" => {
                let list_type = data
                    .get(payload_start..payload_start.saturating_add(4))
                    .unwrap_or(b"\0\0\0\0");
                match list_type {
                    b"hdrl" => {
                        // Parse header list for stream info.
                        let hdrl = parse_hdrl(
                            data.get(payload_start.saturating_add(4)..payload_end)
                                .unwrap_or(&[]),
                        );
                        width = hdrl.width;
                        height = hdrl.height;
                        fps = hdrl.fps;
                        bit_count = hdrl.bit_count;
                        compression = hdrl.compression;
                        sample_rate = hdrl.sample_rate;
                        channels = hdrl.channels;
                        audio_bits = hdrl.audio_bits;
                    }
                    b"movi" => {
                        // Extract video and audio chunks.
                        parse_movi(
                            data.get(payload_start.saturating_add(4)..payload_end)
                                .unwrap_or(&[]),
                            width,
                            height,
                            bit_count,
                            &mut video_frames,
                            audio_bits,
                            &mut audio_samples,
                        )?;
                    }
                    _ => {}
                }
            }
            _ => {
                // idx1, JUNK, etc. — skip.
            }
        }

        let padded = chunk_size.saturating_add(chunk_size & 1);
        pos = payload_start.saturating_add(padded);
    }

    if compression != 0 {
        return Err(Error::DecompressionError {
            reason: "AVI video uses compressed codec (only uncompressed BI_RGB supported)",
        });
    }

    // V38: reject dimensions that could cause excessive allocation.
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(Error::InvalidSize {
            value: width.max(height) as usize,
            limit: MAX_DIMENSION as usize,
            context: "AVI video dimension",
        });
    }

    Ok(AviContent {
        frames: video_frames,
        width,
        height,
        fps,
        audio: audio_samples,
        sample_rate,
        channels,
    })
}

// ─── Read Helpers ────────────────────────────────────────────────────────────

/// Reads a little-endian u32 from RIFF data.
#[inline]
fn read_riff_u32(data: &[u8], offset: usize) -> Result<u32, Error> {
    let end = offset.checked_add(4).ok_or(Error::UnexpectedEof {
        needed: usize::MAX,
        available: data.len(),
    })?;
    let slice = data.get(offset..end).ok_or(Error::UnexpectedEof {
        needed: end,
        available: data.len(),
    })?;
    let mut buf = [0u8; 4];
    buf.copy_from_slice(slice);
    Ok(u32::from_le_bytes(buf))
}

/// Reads a little-endian u16 from RIFF data.
#[inline]
fn read_riff_u16(data: &[u8], offset: usize) -> u16 {
    let end = offset.saturating_add(2);
    let slice = data.get(offset..end).unwrap_or(&[0, 0]);
    let mut buf = [0u8; 2];
    let copy_len = slice.len().min(2);
    if let Some(dst) = buf.get_mut(..copy_len) {
        if let Some(src) = slice.get(..copy_len) {
            dst.copy_from_slice(src);
        }
    }
    u16::from_le_bytes(buf)
}

/// Reads a little-endian i32 from RIFF data.
#[inline]
fn read_riff_i32(data: &[u8], offset: usize) -> i32 {
    let end = offset.saturating_add(4);
    let slice = data.get(offset..end).unwrap_or(&[0, 0, 0, 0]);
    let mut buf = [0u8; 4];
    let copy_len = slice.len().min(4);
    if let Some(dst) = buf.get_mut(..copy_len) {
        if let Some(src) = slice.get(..copy_len) {
            dst.copy_from_slice(src);
        }
    }
    i32::from_le_bytes(buf)
}

// ─── Internal Parsers ────────────────────────────────────────────────────────

/// Parsed stream parameters from the AVI hdrl LIST.
#[derive(Default)]
struct HdrlParams {
    width: u32,
    height: u32,
    fps: u32,
    bit_count: u16,
    compression: u32,
    sample_rate: u32,
    channels: u16,
    audio_bits: u16,
}

/// Parses the hdrl LIST to extract video/audio stream parameters.
fn parse_hdrl(data: &[u8]) -> HdrlParams {
    let mut p = HdrlParams::default();
    let mut pos: usize = 0;
    let mut in_video_strl = false;
    let mut in_audio_strl = false;

    while pos.saturating_add(8) <= data.len() {
        let fourcc = data.get(pos..pos.saturating_add(4)).unwrap_or(b"\0\0\0\0");
        let chunk_size = data
            .get(pos.saturating_add(4)..pos.saturating_add(8))
            .map(|s| {
                let mut buf = [0u8; 4];
                buf.copy_from_slice(s);
                u32::from_le_bytes(buf) as usize
            })
            .unwrap_or(0);
        let payload_start = pos.saturating_add(8);

        match fourcc {
            b"LIST" => {
                let list_type = data
                    .get(payload_start..payload_start.saturating_add(4))
                    .unwrap_or(b"\0\0\0\0");
                if list_type == b"strl" {
                    // Peek at strh to determine stream type.
                    let inner_start = payload_start.saturating_add(4);
                    let strh_type = data
                        .get(inner_start.saturating_add(8)..inner_start.saturating_add(12))
                        .unwrap_or(b"\0\0\0\0");
                    in_video_strl = strh_type == b"vids";
                    in_audio_strl = strh_type == b"auds";
                }
                pos = payload_start.saturating_add(4);
                continue;
            }
            b"avih" => {
                // Main AVI header: extract fps from dwMicroSecPerFrame.
                let us_per_frame = data
                    .get(payload_start..payload_start.saturating_add(4))
                    .map(|s| {
                        let mut buf = [0u8; 4];
                        buf.copy_from_slice(s);
                        u32::from_le_bytes(buf)
                    })
                    .unwrap_or(66667);
                if let Some(fps) = 1_000_000u32.checked_div(us_per_frame) {
                    p.fps = fps;
                }
            }
            b"strh" if in_video_strl => {
                // dwScale at offset 20, dwRate at offset 24.
                let scale_off = payload_start.saturating_add(20);
                let rate_off = payload_start.saturating_add(24);
                let scale = data
                    .get(scale_off..scale_off.saturating_add(4))
                    .map(|s| {
                        let mut buf = [0u8; 4];
                        buf.copy_from_slice(s);
                        u32::from_le_bytes(buf)
                    })
                    .unwrap_or(1);
                let rate = data
                    .get(rate_off..rate_off.saturating_add(4))
                    .map(|s| {
                        let mut buf = [0u8; 4];
                        buf.copy_from_slice(s);
                        u32::from_le_bytes(buf)
                    })
                    .unwrap_or(15);
                if let Some(fps) = rate.checked_div(scale) {
                    p.fps = fps;
                }
            }
            b"strf" => {
                if in_video_strl {
                    // BITMAPINFOHEADER: biWidth(4), biHeight(4), biPlanes(2), biBitCount(2), biCompression(4)
                    // Starts at offset 4 within strf payload (after biSize).
                    p.width = read_riff_i32(data, payload_start.saturating_add(4)).unsigned_abs();
                    p.height = read_riff_i32(data, payload_start.saturating_add(8)).unsigned_abs();
                    p.bit_count = read_riff_u16(data, payload_start.saturating_add(14));
                    p.compression = data
                        .get(payload_start.saturating_add(16)..payload_start.saturating_add(20))
                        .map(|s| {
                            let mut buf = [0u8; 4];
                            buf.copy_from_slice(s);
                            u32::from_le_bytes(buf)
                        })
                        .unwrap_or(0);
                    in_video_strl = false;
                } else if in_audio_strl {
                    // WAVEFORMATEX.
                    p.channels = read_riff_u16(data, payload_start.saturating_add(2));
                    p.sample_rate = data
                        .get(payload_start.saturating_add(4)..payload_start.saturating_add(8))
                        .map(|s| {
                            let mut buf = [0u8; 4];
                            buf.copy_from_slice(s);
                            u32::from_le_bytes(buf)
                        })
                        .unwrap_or(0);
                    p.audio_bits = read_riff_u16(data, payload_start.saturating_add(14));
                    in_audio_strl = false;
                }
            }
            _ => {}
        }

        let padded = chunk_size.saturating_add(chunk_size & 1);
        pos = payload_start.saturating_add(padded);
    }

    p
}

/// Parses the movi LIST to extract video frames and audio chunks.
fn parse_movi(
    data: &[u8],
    width: u32,
    height: u32,
    bit_count: u16,
    frames: &mut Vec<Vec<u8>>,
    audio_bits: u16,
    audio_samples: &mut Vec<i16>,
) -> Result<(), Error> {
    let mut pos: usize = 0;

    while pos.saturating_add(8) <= data.len() {
        let fourcc = data.get(pos..pos.saturating_add(4)).unwrap_or(b"\0\0\0\0");
        let chunk_size = data
            .get(pos.saturating_add(4)..pos.saturating_add(8))
            .map(|s| {
                let mut buf = [0u8; 4];
                buf.copy_from_slice(s);
                u32::from_le_bytes(buf) as usize
            })
            .unwrap_or(0);
        let payload_start = pos.saturating_add(8);
        let payload_end = payload_start.saturating_add(chunk_size);
        let payload = data
            .get(payload_start..payload_end)
            .ok_or(Error::InvalidOffset {
                offset: payload_end,
                bound: data.len(),
            })?;

        match fourcc {
            // Video: "00dc" or "00db" (compressed/uncompressed DIB).
            b"00dc" | b"00db" if width > 0 && height > 0 => {
                let rgba = bgr_to_rgba(payload, width, height, bit_count)?;
                frames.push(rgba);
            }
            // Audio: "01wb".
            b"01wb" => {
                decode_pcm_audio(payload, audio_bits, audio_samples);
            }
            // Nested LIST (rec chunks in some AVIs).
            b"LIST" => {
                let inner = data
                    .get(payload_start.saturating_add(4)..payload_end)
                    .ok_or(Error::InvalidOffset {
                        offset: payload_end,
                        bound: data.len(),
                    })?;
                parse_movi(
                    inner,
                    width,
                    height,
                    bit_count,
                    frames,
                    audio_bits,
                    audio_samples,
                )?;
                let padded = chunk_size.saturating_add(chunk_size & 1);
                pos = payload_start.saturating_add(padded);
                continue;
            }
            _ => {
                // Skip unknown chunks.
            }
        }

        let padded = chunk_size.saturating_add(chunk_size & 1);
        pos = payload_start.saturating_add(padded);
    }
    Ok(())
}

/// Converts bottom-up BGR24 (AVI/BMP) to top-down RGBA.
fn bgr_to_rgba(data: &[u8], width: u32, height: u32, bit_count: u16) -> Result<Vec<u8>, Error> {
    let w = width as usize;
    let h = height as usize;
    let bpp = (bit_count as usize) / 8;
    if bpp < 3 {
        return Err(Error::DecompressionError {
            reason: "AVI video bit depth unsupported (expected 24-bit or 32-bit BI_RGB)",
        });
    }
    let row_bytes = w.saturating_mul(bpp);
    let row_stride = (row_bytes.saturating_add(3)) & !3;
    let expected_len = row_stride.saturating_mul(h);
    if data.len() < expected_len {
        return Err(Error::UnexpectedEof {
            needed: expected_len,
            available: data.len(),
        });
    }
    if data.len() > expected_len {
        return Err(Error::InvalidSize {
            value: data.len(),
            limit: expected_len,
            context: "AVI video frame payload",
        });
    }
    let pixel_count = w.saturating_mul(h);
    let mut rgba = vec![0u8; pixel_count.saturating_mul(4)];

    for row in 0..h {
        // Bottom-up: row 0 in data = last row on screen.
        let src_row = (h.saturating_sub(1).saturating_sub(row)).saturating_mul(row_stride);
        for col in 0..w {
            let src = src_row.saturating_add(col.saturating_mul(bpp));
            let dst = row.saturating_mul(w).saturating_add(col).saturating_mul(4);

            // BGR24 or BGR32.
            let bb = data.get(src).copied().unwrap_or(0);
            let gg = data.get(src.saturating_add(1)).copied().unwrap_or(0);
            let rr = data.get(src.saturating_add(2)).copied().unwrap_or(0);

            if let Some(dst_slice) = rgba.get_mut(dst..dst.saturating_add(4)) {
                dst_slice.copy_from_slice(&[rr, gg, bb, 255]);
            }
        }
    }

    Ok(rgba)
}

/// Decodes raw PCM audio bytes into i16 samples.
fn decode_pcm_audio(data: &[u8], bits: u16, samples: &mut Vec<i16>) {
    if bits == 16 {
        let mut pos: usize = 0;
        while pos.saturating_add(1) < data.len() {
            let lo = data.get(pos).copied().unwrap_or(0) as u16;
            let hi = data.get(pos.saturating_add(1)).copied().unwrap_or(0) as u16;
            samples.push((lo | (hi << 8)) as i16);
            pos = pos.saturating_add(2);
        }
    } else {
        // 8-bit unsigned → signed 16-bit.
        for &byte in data {
            samples.push((byte as i16 - 128) * 256);
        }
    }
}
