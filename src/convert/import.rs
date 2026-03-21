// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Common format → C&C format import conversions (PNG/GIF/WAV → C&C).
//!
//! These functions accept standard file formats and produce C&C format
//! binary data suitable for writing to `.shp`, `.aud`, `.pal`, `.wsa`,
//! or `.tmp` files.

use crate::error::Error;
use crate::pal::Palette;

use super::{decode_gif_frames_rgba, decode_png, rgba_to_indexed};

// ─── PNG → SHP ───────────────────────────────────────────────────────────────

/// Converts one or more PNG files to an SHP sprite file.
///
/// Each PNG is decoded to RGBA, quantized to the given palette via
/// nearest-color matching, and encoded as an LCW keyframe.  All PNGs
/// must share the same dimensions.
///
/// Returns the complete SHP file as `Vec<u8>`.
pub fn png_to_shp(png_files: &[&[u8]], palette: &Palette) -> Result<Vec<u8>, Error> {
    if png_files.is_empty() {
        return Err(Error::DecompressionError {
            reason: "no PNG files provided for SHP encoding",
        });
    }

    let mut indexed_frames: Vec<Vec<u8>> = Vec::with_capacity(png_files.len());
    let mut frame_w: u32 = 0;
    let mut frame_h: u32 = 0;

    for (i, png_data) in png_files.iter().enumerate() {
        let (rgba, w, h) = decode_png(png_data)?;
        if i == 0 {
            frame_w = w;
            frame_h = h;
        } else if w != frame_w || h != frame_h {
            return Err(Error::DecompressionError {
                reason: "PNG dimensions must be identical for all SHP frames",
            });
        }
        indexed_frames.push(rgba_to_indexed(&rgba, palette));
    }

    let frame_refs: Vec<&[u8]> = indexed_frames.iter().map(|f| f.as_slice()).collect();
    crate::shp::encode_frames(&frame_refs, frame_w as u16, frame_h as u16)
}

// ─── GIF → SHP ───────────────────────────────────────────────────────────────

/// Converts an animated GIF to an SHP sprite file.
///
/// Each GIF frame is extracted as palette-indexed pixels.  If the GIF's
/// palette differs from the target palette, pixels are re-quantized via
/// nearest-color matching.  All frames must share the GIF's logical
/// screen dimensions.
///
/// Returns the complete SHP file as `Vec<u8>`.
pub fn gif_to_shp(gif_data: &[u8], palette: &Palette) -> Result<Vec<u8>, Error> {
    let (rgba_frames, width, height) = decode_gif_frames_rgba(gif_data)?;
    if rgba_frames.is_empty() {
        return Err(Error::DecompressionError {
            reason: "GIF contains no frames",
        });
    }
    // Quantize each RGBA frame to the target C&C palette.
    let indexed: Vec<Vec<u8>> = rgba_frames
        .iter()
        .map(|rgba| rgba_to_indexed(rgba, palette))
        .collect();
    let frame_refs: Vec<&[u8]> = indexed.iter().map(|f| f.as_slice()).collect();
    crate::shp::encode_frames(&frame_refs, width, height)
}

// ─── GIF → WSA ───────────────────────────────────────────────────────────────

/// Converts an animated GIF to a WSA animation file.
///
/// Each GIF frame is extracted as palette-indexed pixels.  WSA uses
/// XOR-delta + LCW compression between consecutive frames, making it
/// efficient for animations where successive frames differ slightly.
///
/// Returns the complete WSA file as `Vec<u8>`.
pub fn gif_to_wsa(gif_data: &[u8], palette: &Palette) -> Result<Vec<u8>, Error> {
    let (rgba_frames, width, height) = decode_gif_frames_rgba(gif_data)?;
    if rgba_frames.is_empty() {
        return Err(Error::DecompressionError {
            reason: "GIF contains no frames",
        });
    }
    // Quantize each RGBA frame to the target C&C palette.
    let indexed: Vec<Vec<u8>> = rgba_frames
        .iter()
        .map(|rgba| rgba_to_indexed(rgba, palette))
        .collect();
    let frame_refs: Vec<&[u8]> = indexed.iter().map(|f| f.as_slice()).collect();
    crate::wsa::encode_frames(&frame_refs, width, height)
}

// ─── PNG → PAL ───────────────────────────────────────────────────────────────

/// Extracts a 256-color palette from a PNG image.
///
/// If the PNG is an indexed-color image with ≤256 entries, the palette is
/// extracted directly (converted from 8-bit to 6-bit VGA).  If the PNG is
/// RGBA, the unique colors are collected (up to 256); an error is returned
/// if the image contains more than 256 unique colors.
///
/// Returns the complete PAL file as `Vec<u8>` (768 bytes).
pub fn png_to_pal(png_data: &[u8]) -> Result<Vec<u8>, Error> {
    let decoder = png::Decoder::new(std::io::Cursor::new(png_data));
    let reader = decoder.read_info().map_err(|_| Error::DecompressionError {
        reason: "PNG decode failed",
    })?;
    let info = reader.info();

    // Try to extract palette from indexed PNG directly.
    if info.color_type == png::ColorType::Indexed {
        if let Some(plte) = &info.palette {
            // Each palette entry is 3 bytes (R, G, B).  Pad to 256 entries.
            let mut rgb8 = [0u8; 768];
            let copy_len = plte.len().min(768);
            if let Some(dest) = rgb8.get_mut(..copy_len) {
                if let Some(src) = plte.get(..copy_len) {
                    dest.copy_from_slice(src);
                }
            }
            return Palette::from_rgb8(&rgb8).map(|pal| pal.encode());
        }
    }

    // For RGBA images: decode fully and collect unique colors.
    let (rgba, _, _) = decode_png(png_data)?;
    let mut unique_colors: Vec<[u8; 3]> = Vec::with_capacity(256);

    // Iterate RGBA pixels in 4-byte chunks.
    let mut i: usize = 0;
    while i.saturating_add(3) < rgba.len() {
        let r = rgba.get(i).copied().unwrap_or(0);
        let g = rgba.get(i.saturating_add(1)).copied().unwrap_or(0);
        let b = rgba.get(i.saturating_add(2)).copied().unwrap_or(0);
        let color = [r, g, b];
        if !unique_colors.contains(&color) {
            if unique_colors.len() >= 256 {
                return Err(Error::DecompressionError {
                    reason: "PNG has more than 256 unique colors; cannot create PAL",
                });
            }
            unique_colors.push(color);
        }
        i = i.saturating_add(4);
    }

    // Build 768-byte RGB8 buffer, pad remaining entries with black.
    let mut rgb8 = [0u8; 768];
    for (idx, &[r, g, b]) in unique_colors.iter().enumerate() {
        let base = idx.saturating_mul(3);
        if let Some(slot) = rgb8.get_mut(base..base.saturating_add(3)) {
            slot.copy_from_slice(&[r, g, b]);
        }
    }
    Palette::from_rgb8(&rgb8).map(|pal| pal.encode())
}

// ─── WAV → AUD ───────────────────────────────────────────────────────────────

/// Converts a WAV file to Westwood AUD format (IMA ADPCM).
///
/// Decodes the WAV to 16-bit PCM samples, then encodes using the Westwood
/// IMA ADPCM codec.  The resulting AUD file uses `SCOMP_WESTWOOD` (99)
/// compression.
///
/// Supports mono and stereo WAV input at any sample rate.
///
/// Returns the complete AUD file as `Vec<u8>`.
pub fn wav_to_aud(wav_data: &[u8]) -> Result<Vec<u8>, Error> {
    let cursor = std::io::Cursor::new(wav_data);
    let mut reader = hound::WavReader::new(cursor).map_err(|_| Error::DecompressionError {
        reason: "WAV decode failed",
    })?;

    let spec = reader.spec();
    let sample_rate = spec.sample_rate as u16;
    let stereo = spec.channels > 1;

    // Read all samples as i16 (hound converts from other bit depths).
    let samples: Vec<i16> = if spec.sample_format == hound::SampleFormat::Float {
        // Float WAV: convert to i16.
        reader
            .samples::<f32>()
            .map(|s| {
                let f = s.unwrap_or(0.0);
                // Clamp to [-1.0, 1.0] and scale to i16 range.
                (f.clamp(-1.0, 1.0) * 32767.0) as i16
            })
            .collect()
    } else {
        reader.samples::<i16>().map(|s| s.unwrap_or(0)).collect()
    };

    Ok(crate::aud::build_aud(&samples, sample_rate, stereo))
}

// ─── PNG → TMP (TD) ─────────────────────────────────────────────────────────

/// Converts one or more PNGs to a Tiberian Dawn TMP terrain tile file.
///
/// Each PNG represents one tile.  Pixels are quantized to the given palette
/// using nearest-color matching.  All PNGs must share the same dimensions
/// (typically 24×24 for TD tiles).
///
/// Returns the complete TMP file as `Vec<u8>`.
pub fn png_to_td_tmp(png_files: &[&[u8]], palette: &Palette) -> Result<Vec<u8>, Error> {
    if png_files.is_empty() {
        return Err(Error::DecompressionError {
            reason: "no PNG files provided for TMP encoding",
        });
    }

    let mut indexed_tiles: Vec<Vec<u8>> = Vec::with_capacity(png_files.len());
    let mut tile_w: u32 = 0;
    let mut tile_h: u32 = 0;

    for (i, png_data) in png_files.iter().enumerate() {
        let (rgba, w, h) = decode_png(png_data)?;
        if i == 0 {
            tile_w = w;
            tile_h = h;
        } else if w != tile_w || h != tile_h {
            return Err(Error::DecompressionError {
                reason: "PNG dimensions must be identical for all TMP tiles",
            });
        }
        indexed_tiles.push(rgba_to_indexed(&rgba, palette));
    }

    let tile_refs: Vec<&[u8]> = indexed_tiles.iter().map(|t| t.as_slice()).collect();
    crate::tmp::encode_td_tmp(&tile_refs, tile_w as u16, tile_h as u16)
}

// ─── PNG → WSA ───────────────────────────────────────────────────────────────

/// Converts one or more PNGs to a WSA animation file.
///
/// Each PNG represents one frame.  Pixels are quantized to the given palette
/// using nearest-color matching.  All PNGs must share the same dimensions.
///
/// Returns the complete WSA file as `Vec<u8>`.
pub fn png_to_wsa(png_files: &[&[u8]], palette: &Palette) -> Result<Vec<u8>, Error> {
    if png_files.is_empty() {
        return Err(Error::DecompressionError {
            reason: "no PNG files provided for WSA encoding",
        });
    }

    let mut indexed_frames: Vec<Vec<u8>> = Vec::with_capacity(png_files.len());
    let mut frame_w: u32 = 0;
    let mut frame_h: u32 = 0;

    for (i, png_data) in png_files.iter().enumerate() {
        let (rgba, w, h) = decode_png(png_data)?;
        if i == 0 {
            frame_w = w;
            frame_h = h;
        } else if w != frame_w || h != frame_h {
            return Err(Error::DecompressionError {
                reason: "PNG dimensions must be identical for all WSA frames",
            });
        }
        indexed_frames.push(rgba_to_indexed(&rgba, palette));
    }

    let frame_refs: Vec<&[u8]> = indexed_frames.iter().map(|f| f.as_slice()).collect();
    crate::wsa::encode_frames(&frame_refs, frame_w as u16, frame_h as u16)
}

// ─── AVI → VQA ───────────────────────────────────────────────────────────────

/// Converts an AVI video file to VQA format (version 2).
///
/// Reads uncompressed AVI frames, quantizes RGBA pixels to the provided
/// palette, VQ-encodes the blocks, and wraps audio as IMA ADPCM.  The
/// resulting VQA file is compatible with C&C engine playback.
///
/// Returns the complete VQA file as `Vec<u8>`.
///
/// # Errors
///
/// - [`Error::InvalidMagic`] if the AVI is not valid RIFF.
/// - [`Error::DecompressionError`] if the AVI uses compressed video.
/// - [`Error::InvalidSize`] if no frames are found.
pub fn avi_to_vqa(avi_data: &[u8], palette: &Palette) -> Result<Vec<u8>, Error> {
    let avi = super::avi::decode_avi(avi_data)?;
    if avi.frames.is_empty() {
        return Err(Error::DecompressionError {
            reason: "AVI contains no video frames",
        });
    }

    // Quantize each RGBA frame to palette indices.
    let indexed_frames: Vec<Vec<u8>> = avi
        .frames
        .iter()
        .map(|rgba| rgba_to_indexed(rgba, palette))
        .collect();

    // Build 8-bit RGB palette for VQA encoder.
    let rgb8_lut = palette.to_rgb8_array();
    let mut palette_rgb8 = [0u8; 768];
    for (i, &[r, g, b]) in rgb8_lut.iter().enumerate() {
        let base = i.saturating_mul(3);
        if let Some(slot) = palette_rgb8.get_mut(base..base.saturating_add(3)) {
            slot.copy_from_slice(&[r, g, b]);
        }
    }

    let channels: u8 = if avi.channels > 0 {
        avi.channels.min(255) as u8
    } else {
        1
    };
    let audio_input = if !avi.audio.is_empty() && avi.sample_rate > 0 {
        Some(crate::vqa::VqaAudioInput {
            samples: &avi.audio,
            sample_rate: avi.sample_rate as u16,
            channels,
        })
    } else {
        None
    };

    let params = crate::vqa::VqaEncodeParams {
        fps: avi.fps.clamp(1, 255) as u8,
        ..Default::default()
    };

    crate::vqa::encode_vqa(
        &indexed_frames,
        &palette_rgb8,
        avi.width as u16,
        avi.height as u16,
        audio_input.as_ref(),
        &params,
    )
}
