// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Bidirectional format conversion for C&C game assets.
//!
//! ## Export — C&C → common formats
//!
//! | Source | Target | Function |
//! |--------|--------|----------|
//! | SHP + PAL | PNG (per frame) | [`shp_frames_to_png`] |
//! | SHP + PAL | GIF (animated) | [`shp_frames_to_gif`] |
//! | PAL | PNG (16×16 swatch) | [`pal_to_png`] |
//! | AUD | WAV (PCM) | [`aud_to_wav`] |
//! | TMP TD + PAL | PNG (per tile) | [`td_tmp_tiles_to_png`] |
//! | TMP RA + PAL | PNG (per tile) | [`ra_tmp_tiles_to_png`] |
//! | WSA + PAL | PNG (per frame) | [`wsa_frames_to_png`] |
//! | WSA + PAL | GIF (animated) | [`wsa_frames_to_gif`] |
//! | FNT | PNG (font atlas) | [`fnt_to_png`] |
//! | VQA | MKV (BGR24 + PCM) | [`vqa_to_mkv`] |
//!
//! ## Import — common formats → C&C
//!
//! | Source | Target | Function |
//! |--------|--------|----------|
//! | PNG(s) + PAL | SHP | [`png_to_shp`] |
//! | GIF + PAL | SHP | [`gif_to_shp`] |
//! | PNG(s) + PAL | WSA | [`png_to_wsa`] |
//! | GIF + PAL | WSA | [`gif_to_wsa`] |
//! | PNG | PAL (768 bytes) | [`png_to_pal`] |
//! | WAV | AUD (ADPCM) | [`wav_to_aud`] |
//! | PNG(s) + PAL | TMP (TD) | [`png_to_td_tmp`] |
//!
//! All functions return `Vec<u8>` containing the encoded file bytes — no
//! filesystem I/O is performed.  Callers write the output to files, network,
//! or wherever they need.
//!
//! Requires the `convert` feature flag.

mod avi;
mod export;
mod import;
pub(crate) mod mkv;

// Re-export all public conversion functions.
pub use avi::{decode_avi, encode_avi, AviContent};
pub use export::*;
pub use import::*;
pub use mkv::{encode_mkv, MkvAudio, MkvVideoCodec};

use crate::error::Error;
use crate::pal::Palette;

/// V38: maximum image dimension (width or height) for decode operations.
///
/// Prevents untrusted PNG/GIF headers from triggering OOM.  4096 × 4096 × 4
/// bytes ≈ 64 MB per frame — generous for any C&C asset while preventing a
/// crafted image from allocating gigabytes.  Matches `avi::MAX_DIMENSION`.
const MAX_IMAGE_DIMENSION: u32 = 4096;

/// V38: maximum total frame count for GIF decode.
///
/// GIF animations with thousands of frames could consume excessive memory.
/// 65536 frames × 64 MB worst case = absurd, but the per-frame cap above
/// keeps each frame reasonable.  This mainly guards against degenerate GIFs.
const MAX_GIF_FRAMES: usize = 65536;

// ─── Shared helpers ──────────────────────────────────────────────────────────

/// Converts palette-indexed pixels to RGBA bytes.
///
/// Index 0 is treated as transparent (alpha = 0) for sprite formats where
/// color 0 is conventionally transparent in C&C games.  Set
/// `transparent_zero` to `false` for formats where all indices are opaque
/// (e.g. terrain tiles, palette swatches).
fn indexed_to_rgba(pixels: &[u8], palette: &Palette, transparent_zero: bool) -> Vec<u8> {
    let lut = palette.to_rgb8_array();
    let mut rgba = Vec::with_capacity(pixels.len().saturating_mul(4));
    for &idx in pixels {
        let [r, g, b] = lut.get(idx as usize).copied().unwrap_or([0, 0, 0]);
        rgba.push(r);
        rgba.push(g);
        rgba.push(b);
        // Index 0 = transparent for sprites; fully opaque otherwise.
        let alpha = if transparent_zero && idx == 0 { 0 } else { 255 };
        rgba.push(alpha);
    }
    rgba
}

/// Encodes RGBA pixels as a PNG file in memory.
///
/// Returns the complete PNG file as `Vec<u8>`.
fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, Error> {
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut buf, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|_| Error::DecompressionError {
                reason: "PNG encoder failed to write header",
            })?;
        writer
            .write_image_data(rgba)
            .map_err(|_| Error::DecompressionError {
                reason: "PNG encoder failed to write image data",
            })?;
    }
    Ok(buf)
}

/// Decodes a PNG file from bytes to RGBA pixels + dimensions.
///
/// Returns `(rgba_pixels, width, height)`.
fn decode_png(data: &[u8]) -> Result<(Vec<u8>, u32, u32), Error> {
    let decoder = png::Decoder::new(std::io::Cursor::new(data));
    let mut reader = decoder.read_info().map_err(|_| Error::DecompressionError {
        reason: "PNG decode failed",
    })?;
    let buf_size = reader.output_buffer_size().unwrap_or(0);
    let mut buf = vec![0u8; buf_size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|_| Error::DecompressionError {
            reason: "PNG frame decode failed",
        })?;
    let width = info.width;
    let height = info.height;

    // V38: reject images whose dimensions could cause excessive allocation.
    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err(Error::InvalidSize {
            value: width.max(height) as usize,
            limit: MAX_IMAGE_DIMENSION as usize,
            context: "PNG image dimension",
        });
    }

    // Convert to RGBA regardless of input color type.
    let rgba = match info.color_type {
        png::ColorType::Rgba => {
            buf.truncate(info.buffer_size());
            buf
        }
        png::ColorType::Rgb => {
            // Expand RGB → RGBA (add alpha = 255).
            let pixel_count = (width as usize).saturating_mul(height as usize);
            let mut rgba = Vec::with_capacity(pixel_count.saturating_mul(4));
            let rgb_data = buf.get(..info.buffer_size()).unwrap_or(&buf);
            let mut i: usize = 0;
            while i.saturating_add(2) < rgb_data.len() {
                rgba.push(rgb_data.get(i).copied().unwrap_or(0));
                rgba.push(rgb_data.get(i.saturating_add(1)).copied().unwrap_or(0));
                rgba.push(rgb_data.get(i.saturating_add(2)).copied().unwrap_or(0));
                rgba.push(255);
                i = i.saturating_add(3);
            }
            rgba
        }
        png::ColorType::GrayscaleAlpha => {
            let pixel_count = (width as usize).saturating_mul(height as usize);
            let mut rgba = Vec::with_capacity(pixel_count.saturating_mul(4));
            let ga_data = buf.get(..info.buffer_size()).unwrap_or(&buf);
            let mut i: usize = 0;
            while i.saturating_add(1) < ga_data.len() {
                let grey = ga_data.get(i).copied().unwrap_or(0);
                let alpha = ga_data.get(i.saturating_add(1)).copied().unwrap_or(255);
                rgba.push(grey);
                rgba.push(grey);
                rgba.push(grey);
                rgba.push(alpha);
                i = i.saturating_add(2);
            }
            rgba
        }
        png::ColorType::Grayscale => {
            let pixel_count = (width as usize).saturating_mul(height as usize);
            let mut rgba = Vec::with_capacity(pixel_count.saturating_mul(4));
            let g_data = buf.get(..info.buffer_size()).unwrap_or(&buf);
            for &grey in g_data {
                rgba.push(grey);
                rgba.push(grey);
                rgba.push(grey);
                rgba.push(255);
            }
            rgba
        }
        png::ColorType::Indexed => {
            // Indexed PNG: look up palette to produce RGBA.
            let reader2 = png::Decoder::new(std::io::Cursor::new(data))
                .read_info()
                .map_err(|_| Error::DecompressionError {
                    reason: "PNG re-decode failed for palette extraction",
                })?;
            let plte = reader2
                .info()
                .palette
                .as_ref()
                .ok_or(Error::DecompressionError {
                    reason: "indexed PNG missing palette",
                })?;
            let trns = reader2.info().trns.as_ref();
            let indices = buf.get(..info.buffer_size()).unwrap_or(&buf);
            let pixel_count = (width as usize).saturating_mul(height as usize);
            let mut rgba = Vec::with_capacity(pixel_count.saturating_mul(4));
            for &idx in indices {
                let base = (idx as usize).saturating_mul(3);
                let r = plte.get(base).copied().unwrap_or(0);
                let g = plte.get(base.saturating_add(1)).copied().unwrap_or(0);
                let b = plte.get(base.saturating_add(2)).copied().unwrap_or(0);
                let a = trns
                    .and_then(|t| t.get(idx as usize).copied())
                    .unwrap_or(255);
                rgba.push(r);
                rgba.push(g);
                rgba.push(b);
                rgba.push(a);
            }
            rgba
        }
    };
    Ok((rgba, width, height))
}

/// Maps RGBA pixels to palette indices using nearest-color matching.
///
/// For each pixel, finds the palette entry with the smallest Euclidean
/// distance in RGB space.  Alpha < 128 maps to index 0 (transparent).
fn rgba_to_indexed(rgba: &[u8], palette: &Palette) -> Vec<u8> {
    let lut = palette.to_rgb8_array();
    let pixel_count = rgba.len() / 4;
    let mut indices = Vec::with_capacity(pixel_count);

    let mut i: usize = 0;
    while i.saturating_add(3) < rgba.len() {
        let r = rgba.get(i).copied().unwrap_or(0);
        let g = rgba.get(i.saturating_add(1)).copied().unwrap_or(0);
        let b = rgba.get(i.saturating_add(2)).copied().unwrap_or(0);
        let a = rgba.get(i.saturating_add(3)).copied().unwrap_or(255);

        if a < 128 {
            // Transparent pixel → index 0.
            indices.push(0);
        } else {
            // Find nearest palette entry by squared Euclidean distance.
            let mut best_idx: u8 = 0;
            let mut best_dist: u32 = u32::MAX;
            for (idx, &[pr, pg, pb]) in lut.iter().enumerate() {
                let dr = (r as i32).saturating_sub(pr as i32);
                let dg = (g as i32).saturating_sub(pg as i32);
                let db = (b as i32).saturating_sub(pb as i32);
                let dist = (dr.saturating_mul(dr) as u32)
                    .saturating_add(dg.saturating_mul(dg) as u32)
                    .saturating_add(db.saturating_mul(db) as u32);
                if dist < best_dist {
                    best_dist = dist;
                    best_idx = idx as u8;
                    if dist == 0 {
                        break; // Exact match.
                    }
                }
            }
            indices.push(best_idx);
        }
        i = i.saturating_add(4);
    }
    indices
}

/// Decodes an animated GIF into RGBA frames.
///
/// Each frame is composited onto a canvas (handling sub-frame positioning
/// and disposal methods).  The GIF's palette (global or per-frame local)
/// is used to expand each indexed pixel to RGBA.  Transparent pixels get
/// alpha = 0.
///
/// Returns `(rgba_frames, width, height)` where each frame is a flat
/// `Vec<u8>` of RGBA pixels (4 bytes per pixel, w × h pixels).
fn decode_gif_frames_rgba(data: &[u8]) -> Result<(Vec<Vec<u8>>, u16, u16), Error> {
    let mut decoder_opts = gif::DecodeOptions::new();
    // RGBA output handles global/local palettes and transparency for us.
    decoder_opts.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = decoder_opts
        .read_info(std::io::Cursor::new(data))
        .map_err(|_| Error::DecompressionError {
            reason: "GIF decode failed",
        })?;

    let width = decoder.width();
    let height = decoder.height();

    // V38: reject GIFs whose canvas dimensions could cause excessive allocation.
    if width as u32 > MAX_IMAGE_DIMENSION || height as u32 > MAX_IMAGE_DIMENSION {
        return Err(Error::InvalidSize {
            value: (width as u32).max(height as u32) as usize,
            limit: MAX_IMAGE_DIMENSION as usize,
            context: "GIF image dimension",
        });
    }

    let screen_size = (width as usize).saturating_mul(height as usize);
    let mut frames: Vec<Vec<u8>> = Vec::new();

    // Composite canvas — RGBA, 4 bytes per pixel.
    let mut canvas = vec![0u8; screen_size.saturating_mul(4)];

    while let Some(frame) = decoder
        .read_next_frame()
        .map_err(|_| Error::DecompressionError {
            reason: "GIF frame decode failed",
        })?
    {
        let fw = frame.width as usize;
        let fh = frame.height as usize;
        let fx = frame.left as usize;
        let fy = frame.top as usize;

        // Composite the RGBA frame onto the canvas.
        for row in 0..fh {
            for col in 0..fw {
                let src_base = row.saturating_mul(fw).saturating_add(col).saturating_mul(4);
                let a = frame
                    .buffer
                    .get(src_base.saturating_add(3))
                    .copied()
                    .unwrap_or(0);
                // Skip fully transparent pixels.
                if a == 0 {
                    continue;
                }
                let dst_x = fx.saturating_add(col);
                let dst_y = fy.saturating_add(row);
                if dst_x < width as usize && dst_y < height as usize {
                    let dst_base = dst_y
                        .saturating_mul(width as usize)
                        .saturating_add(dst_x)
                        .saturating_mul(4);
                    if let Some(dst) = canvas.get_mut(dst_base..dst_base.saturating_add(4)) {
                        let sr = frame.buffer.get(src_base).copied().unwrap_or(0);
                        let sg = frame
                            .buffer
                            .get(src_base.saturating_add(1))
                            .copied()
                            .unwrap_or(0);
                        let sb = frame
                            .buffer
                            .get(src_base.saturating_add(2))
                            .copied()
                            .unwrap_or(0);
                        dst.copy_from_slice(&[sr, sg, sb, a]);
                    }
                }
            }
        }

        frames.push(canvas.clone());

        // V38: cap frame count to prevent excessive memory usage.
        if frames.len() >= MAX_GIF_FRAMES {
            break;
        }

        // Handle disposal method for next frame.
        match frame.dispose {
            gif::DisposalMethod::Background => {
                // Clear the frame area to transparent.
                for row in 0..fh {
                    for col in 0..fw {
                        let dst_x = fx.saturating_add(col);
                        let dst_y = fy.saturating_add(row);
                        if dst_x < width as usize && dst_y < height as usize {
                            let idx = dst_y
                                .saturating_mul(width as usize)
                                .saturating_add(dst_x)
                                .saturating_mul(4);
                            if let Some(dst) = canvas.get_mut(idx..idx.saturating_add(4)) {
                                dst.copy_from_slice(&[0, 0, 0, 0]);
                            }
                        }
                    }
                }
            }
            gif::DisposalMethod::Previous => {
                // Treat as keep for simplicity.
            }
            _ => {
                // Keep: leave canvas as-is for next frame.
            }
        }
    }

    Ok((frames, width, height))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_validation;

#[cfg(test)]
mod tests_avi;

#[cfg(test)]
mod tests_roundtrip;

#[cfg(test)]
mod tests_mkv;
