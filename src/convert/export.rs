// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! C&C format → common format export conversions (PNG, WAV, GIF).

use crate::aud::{AudFile, AudStream};
use crate::error::Error;
use crate::fnt::FntFile;
use crate::pal::Palette;
use crate::shp::ShpFile;
use crate::tmp::{RaTmpFile, TdTmpFile};
use crate::wsa::WsaFile;

use super::{encode_png, indexed_to_rgba};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

// ─── SHP → PNG ───────────────────────────────────────────────────────────────

/// Converts all SHP frames to PNG images.
///
/// Decodes all frames (resolving XOR-delta references), applies the palette,
/// and encodes each frame as a separate RGBA PNG.  Index 0 is transparent.
///
/// Returns one `Vec<u8>` (PNG file bytes) per frame.
///
/// # Errors
///
/// - Frame decode errors (LCW decompression failure, missing keyframe).
/// - PNG encoding errors (shouldn't happen with valid pixel data).
pub fn shp_frames_to_png(shp: &ShpFile<'_>, palette: &Palette) -> Result<Vec<Vec<u8>>, Error> {
    let w = shp.header.width as u32;
    let h = shp.header.height as u32;
    let frames = shp.decode_frames()?;
    let mut pngs = Vec::with_capacity(frames.len());
    for pixels in &frames {
        let rgba = indexed_to_rgba(pixels, palette, true);
        pngs.push(encode_png(&rgba, w, h)?);
    }
    Ok(pngs)
}

// ─── PAL → PNG ───────────────────────────────────────────────────────────────

/// Converts a 256-color palette to a 16×16 swatch PNG.
///
/// Each color occupies one pixel in a 16×16 grid.  All pixels are fully
/// opaque (no transparency).  This is useful for palette visualization
/// and debugging.
///
/// Returns the PNG file as `Vec<u8>`.
pub fn pal_to_png(palette: &Palette) -> Result<Vec<u8>, Error> {
    let lut = palette.to_rgb8_array();
    // 16×16 swatch, 4 bytes (RGBA) per pixel.
    let mut rgba = Vec::with_capacity(256 * 4);
    for &[r, g, b] in &lut {
        rgba.push(r);
        rgba.push(g);
        rgba.push(b);
        rgba.push(255);
    }
    encode_png(&rgba, 16, 16)
}

// ─── AUD → WAV ───────────────────────────────────────────────────────────────

/// Converts an AUD file to WAV format bytes.
///
/// Decodes the Westwood IMA ADPCM audio to 16-bit PCM and wraps it in a
/// standard RIFF WAV container.  The output is a complete `.wav` file
/// suitable for writing to disk or streaming.
///
/// # Errors
///
/// - Returns an error if WAV encoding fails (e.g. sample rate of 0).
pub fn aud_to_wav(aud_file: &AudFile<'_>) -> Result<Vec<u8>, Error> {
    let payload = Cursor::new(aud_file.compressed_data);
    let mut stream = AudStream::from_payload(aud_file.header.clone(), payload);
    let mut buf = Cursor::new(Vec::new());
    aud_stream_to_wav(&mut stream, &mut buf)?;
    Ok(buf.into_inner())
}

/// Converts an AUD reader to WAV bytes without buffering the whole AUD file.
pub fn aud_reader_to_wav<R: Read, W: Write + Seek>(reader: R, writer: W) -> Result<(), Error> {
    let mut stream = AudStream::open(reader)?;
    aud_stream_to_wav(&mut stream, writer)
}

/// Converts a streaming AUD decoder into WAV written to `writer`.
///
/// Writes the RIFF WAV header, streams decoded PCM in bulk, then patches
/// the RIFF and data chunk sizes.  All sample data is written via
/// `write_all` on 8 KB byte slices instead of per-sample calls, which is
/// orders of magnitude faster for long files.
pub fn aud_stream_to_wav<R: Read, W: Write + Seek>(
    stream: &mut AudStream<R>,
    mut writer: W,
) -> Result<(), Error> {
    let header = stream.header();
    let channels = if header.is_stereo() { 2u16 } else { 1u16 };
    let sample_rate = header.sample_rate as u32;
    let block_align = channels.saturating_mul(2); // 16-bit = 2 bytes/sample
    let byte_rate = sample_rate.saturating_mul(block_align as u32);

    // Write RIFF WAV header (44 bytes) with placeholder sizes.
    writer
        .write_all(b"RIFF")
        .map_err(|_| wav_io_error("RIFF tag"))?;
    let riff_size_pos = writer
        .stream_position()
        .map_err(|_| wav_io_error("RIFF size position"))?;
    writer
        .write_all(&0u32.to_le_bytes())
        .map_err(|_| wav_io_error("RIFF size placeholder"))?;
    writer
        .write_all(b"WAVE")
        .map_err(|_| wav_io_error("WAVE tag"))?;

    // fmt chunk (16 bytes payload).
    writer
        .write_all(b"fmt ")
        .map_err(|_| wav_io_error("fmt tag"))?;
    writer
        .write_all(&16u32.to_le_bytes())
        .map_err(|_| wav_io_error("fmt size"))?;
    writer
        .write_all(&1u16.to_le_bytes())
        .map_err(|_| wav_io_error("format tag"))?; // PCM
    writer
        .write_all(&channels.to_le_bytes())
        .map_err(|_| wav_io_error("channels"))?;
    writer
        .write_all(&sample_rate.to_le_bytes())
        .map_err(|_| wav_io_error("sample rate"))?;
    writer
        .write_all(&byte_rate.to_le_bytes())
        .map_err(|_| wav_io_error("byte rate"))?;
    writer
        .write_all(&block_align.to_le_bytes())
        .map_err(|_| wav_io_error("block align"))?;
    writer
        .write_all(&16u16.to_le_bytes())
        .map_err(|_| wav_io_error("bits per sample"))?;

    // data chunk header with placeholder size.
    writer
        .write_all(b"data")
        .map_err(|_| wav_io_error("data tag"))?;
    let data_size_pos = writer
        .stream_position()
        .map_err(|_| wav_io_error("data size position"))?;
    writer
        .write_all(&0u32.to_le_bytes())
        .map_err(|_| wav_io_error("data size placeholder"))?;

    // Decode and write PCM in bulk chunks.
    let mut samples = [0i16; 4096];
    let mut data_bytes_written = 0u64;

    loop {
        let read = stream.read_samples(&mut samples)?;
        if read == 0 {
            break;
        }
        // Convert i16 samples to LE bytes in a stack buffer, then write once.
        let sample_slice = samples.get(..read).unwrap_or(&[]);
        let mut byte_buf = [0u8; 4096 * 2];
        for (i, &s) in sample_slice.iter().enumerate() {
            let le = s.to_le_bytes();
            let base = i.saturating_mul(2);
            if let Some(dst) = byte_buf.get_mut(base..base.saturating_add(2)) {
                dst.copy_from_slice(&le);
            }
        }
        let chunk = byte_buf
            .get(..read.saturating_mul(2))
            .unwrap_or(&byte_buf);
        writer
            .write_all(chunk)
            .map_err(|_| wav_io_error("PCM data"))?;
        data_bytes_written = data_bytes_written.saturating_add(chunk.len() as u64);
    }

    // Patch RIFF size and data chunk size.
    let data_size = data_bytes_written as u32;
    let riff_size = data_size.saturating_add(36); // 44 - 8 = 36 bytes of header after RIFF size

    writer
        .seek(SeekFrom::Start(data_size_pos))
        .map_err(|_| wav_io_error("seek to data size"))?;
    writer
        .write_all(&data_size.to_le_bytes())
        .map_err(|_| wav_io_error("patch data size"))?;

    writer
        .seek(SeekFrom::Start(riff_size_pos))
        .map_err(|_| wav_io_error("seek to RIFF size"))?;
    writer
        .write_all(&riff_size.to_le_bytes())
        .map_err(|_| wav_io_error("patch RIFF size"))?;

    Ok(())
}

/// Shorthand for a WAV I/O write error.
fn wav_io_error(context: &'static str) -> Error {
    Error::DecompressionError { reason: context }
}

// ─── TMP → PNG ───────────────────────────────────────────────────────────────

/// Converts all tiles from a TD TMP file to individual PNG images.
///
/// Each tile is rendered as a separate PNG with `icon_width × icon_height`
/// dimensions.  All pixels are fully opaque (terrain has no transparency).
///
/// Returns one `Vec<u8>` (PNG bytes) per tile, in tile order.
pub fn td_tmp_tiles_to_png(tmp: &TdTmpFile<'_>, palette: &Palette) -> Result<Vec<Vec<u8>>, Error> {
    let w = tmp.header.icon_width as u32;
    let h = tmp.header.icon_height as u32;
    let mut pngs = Vec::with_capacity(tmp.tiles.len());
    for tile in &tmp.tiles {
        let rgba = indexed_to_rgba(tile.pixels, palette, false);
        pngs.push(encode_png(&rgba, w, h)?);
    }
    Ok(pngs)
}

/// Converts all tiles from an RA TMP file to individual PNG images.
///
/// Only present tiles (`Some`) are included.  Each tile is rendered as a
/// separate PNG with `tile_width × tile_height` dimensions.  Missing tiles
/// are represented as `None` in the output vector.
///
/// Returns a `Vec` matching the input tile grid, with `None` for absent tiles.
pub fn ra_tmp_tiles_to_png(
    tmp: &RaTmpFile<'_>,
    palette: &Palette,
) -> Result<Vec<Option<Vec<u8>>>, Error> {
    let w = tmp.header.tile_width;
    let h = tmp.header.tile_height;
    let mut pngs = Vec::with_capacity(tmp.tiles.len());
    for tile_opt in &tmp.tiles {
        match tile_opt {
            Some(tile) => {
                let rgba = indexed_to_rgba(tile.pixels, palette, false);
                pngs.push(Some(encode_png(&rgba, w, h)?));
            }
            None => pngs.push(None),
        }
    }
    Ok(pngs)
}

// ─── WSA → PNG ───────────────────────────────────────────────────────────────

/// Converts all WSA animation frames to PNG images.
///
/// Decodes the LCW-compressed XOR-delta chain, applies the palette, and
/// encodes each frame as a separate RGBA PNG.  Index 0 is transparent.
///
/// Returns one `Vec<u8>` (PNG bytes) per frame.
pub fn wsa_frames_to_png(wsa: &WsaFile<'_>, palette: &Palette) -> Result<Vec<Vec<u8>>, Error> {
    let w = wsa.header.width as u32;
    let h = wsa.header.height as u32;
    let frames = wsa.decode_frames()?;
    let mut pngs = Vec::with_capacity(frames.len());
    for pixels in &frames {
        let rgba = indexed_to_rgba(pixels, palette, true);
        pngs.push(encode_png(&rgba, w, h)?);
    }
    Ok(pngs)
}

// ─── FNT → PNG ───────────────────────────────────────────────────────────────

/// Converts a bitmap font to a grayscale PNG atlas.
///
/// Renders all 256 glyphs in a 16×16 grid.  Each cell is `max_width ×
/// max_height` pixels.  The 4-bit color indices (0–15) are mapped to
/// grayscale: 0 = black (transparent background), 1–15 = proportional
/// white intensity (17×index, so index 15 = 255).
///
/// Returns the PNG file as `Vec<u8>`.
pub fn fnt_to_png(fnt: &FntFile<'_>) -> Result<Vec<u8>, Error> {
    let cell_w = fnt.header.max_width as u32;
    let cell_h = fnt.header.max_height as u32;
    // 16×16 grid of glyphs.
    let img_w = cell_w.saturating_mul(16);
    let img_h = cell_h.saturating_mul(16);

    if img_w == 0 || img_h == 0 {
        // Degenerate font with zero dimensions — return a 1x1 transparent PNG.
        return encode_png(&[0, 0, 0, 0], 1, 1);
    }

    let pixel_count = (img_w as usize).saturating_mul(img_h as usize);
    let mut rgba = vec![0u8; pixel_count.saturating_mul(4)];

    for glyph in &fnt.glyphs {
        let code = glyph.code as u32;
        // Grid position: row = code / 16, col = code % 16.
        let grid_col = code % 16;
        let grid_row = code / 16;
        let base_x = grid_col.saturating_mul(cell_w);
        let base_y = grid_row
            .saturating_mul(cell_h)
            .saturating_add(glyph.y_offset as u32);

        for y in 0..glyph.data_rows as u32 {
            for x in 0..glyph.width as u32 {
                let color_idx = glyph.pixel(x as u8, y as u8);
                if color_idx == 0 {
                    continue; // transparent background — already zeroed.
                }
                // Map 4-bit color (1–15) to grayscale.  17 * 15 = 255.
                let grey = (color_idx as u32).saturating_mul(17).min(255) as u8;

                let px = base_x.saturating_add(x);
                let py = base_y.saturating_add(y);
                if px < img_w && py < img_h {
                    let offset =
                        (py.saturating_mul(img_w).saturating_add(px) as usize).saturating_mul(4);
                    if let Some(pixel) = rgba.get_mut(offset..offset.saturating_add(4)) {
                        pixel.copy_from_slice(&[grey, grey, grey, 255]);
                    }
                }
            }
        }
    }
    encode_png(&rgba, img_w, img_h)
}

// ─── SHP → GIF ───────────────────────────────────────────────────────────────

/// Converts all SHP frames to an animated GIF.
///
/// GIF is a natural fit for SHP sprites: both are palette-indexed with at
/// most 256 colors, and GIF natively supports multi-frame animation.
/// Each frame is rendered using the provided palette.  Index 0 is treated
/// as transparent.
///
/// The `delay_cs` parameter sets the delay between frames in centiseconds
/// (hundredths of a second).  A value of 10 ≈ 100ms per frame ≈ 10 fps.
///
/// Returns the complete GIF file as `Vec<u8>`.
pub fn shp_frames_to_gif(
    shp: &ShpFile<'_>,
    palette: &Palette,
    delay_cs: u16,
) -> Result<Vec<u8>, Error> {
    let frames = shp.decode_frames()?;
    let rgb8 = palette.to_rgb8_array();
    encode_animated_gif(
        &frames,
        shp.header.width,
        shp.header.height,
        &rgb8,
        delay_cs,
    )
}

// ─── WSA → GIF ───────────────────────────────────────────────────────────────

/// Converts all WSA animation frames to an animated GIF.
///
/// WSA is an animation format (LCW + XOR-delta).  GIF is the natural
/// container for previewing these animations since both are palette-indexed.
/// Index 0 is treated as transparent.
///
/// The `delay_cs` parameter works the same as for [`shp_frames_to_gif`].
///
/// Returns the complete GIF file as `Vec<u8>`.
pub fn wsa_frames_to_gif(
    wsa: &WsaFile<'_>,
    palette: &Palette,
    delay_cs: u16,
) -> Result<Vec<u8>, Error> {
    let frames = wsa.decode_frames()?;
    let rgb8 = palette.to_rgb8_array();
    encode_animated_gif(
        &frames,
        wsa.header.width,
        wsa.header.height,
        &rgb8,
        delay_cs,
    )
}

// ─── VQA → AVI ───────────────────────────────────────────────────────────────

/// Converts a VQA video file to an uncompressed AVI.
///
/// Decodes all VQA frames (Version 2 VQ codebook + LCW decompression) and
/// extracts audio (SND0/SND1/SND2), then muxes them into an AVI container
/// with uncompressed BGR24 video and PCM audio.  The result plays in any
/// standard video player (VLC, mpv, WMP, etc.).
///
/// Returns the complete AVI file as `Vec<u8>`.
///
/// # Errors
///
/// - [`Error::DecompressionError`] if VQA frame decoding fails.
/// - [`Error::InvalidSize`] if no frames are produced.
pub fn vqa_to_avi(vqa: &crate::vqa::VqaFile<'_>) -> Result<Vec<u8>, Error> {
    let decoded_frames = vqa.decode_frames()?;
    if decoded_frames.is_empty() {
        return Err(Error::DecompressionError {
            reason: "VQA produced no decoded frames",
        });
    }

    let width = vqa.header.width as u32;
    let height = vqa.header.height as u32;
    let fps = (vqa.header.fps as u32).max(1);

    // Convert palette-indexed frames to RGBA for AVI encoding.
    let mut rgba_frames: Vec<Vec<u8>> = Vec::with_capacity(decoded_frames.len());
    for frame in &decoded_frames {
        let pixel_count = frame.pixels.len();
        let mut rgba = Vec::with_capacity(pixel_count.saturating_mul(4));
        for &idx in &frame.pixels {
            let base = (idx as usize).saturating_mul(3);
            let r = frame.palette.get(base).copied().unwrap_or(0);
            let g = frame
                .palette
                .get(base.saturating_add(1))
                .copied()
                .unwrap_or(0);
            let b = frame
                .palette
                .get(base.saturating_add(2))
                .copied()
                .unwrap_or(0);
            rgba.push(r);
            rgba.push(g);
            rgba.push(b);
            rgba.push(255);
        }
        rgba_frames.push(rgba);
    }

    // Extract audio (if present).
    let audio = vqa.extract_audio()?;
    let (audio_ref, sample_rate, channels) = match &audio {
        Some(a) => (
            Some(a.samples.as_slice()),
            a.sample_rate as u32,
            a.channels as u16,
        ),
        None => (None, 0, 0),
    };

    super::avi::encode_avi(
        &rgba_frames,
        width,
        height,
        fps,
        audio_ref,
        sample_rate,
        channels,
    )
}

// ─── VQA → MKV ──────────────────────────────────────────────────────────────

/// Converts a VQA video file to a Matroska (MKV) container.
///
/// Decodes all VQA frames and extracts audio, then muxes them into an MKV
/// container with BGR24 video and `A_PCM/INT/LIT` audio.  The `video_codec`
/// parameter selects between `V_UNCOMPRESSED` (modern, RFC 9559) and
/// `V_MS/VFW/FOURCC` (legacy, broad player compatibility).
///
/// Returns the complete MKV file as `Vec<u8>`.
///
/// # Errors
///
/// - [`Error::DecompressionError`] if VQA frame decoding fails.
/// - [`Error::InvalidSize`] if no frames are produced.
pub fn vqa_to_mkv(
    vqa: &crate::vqa::VqaFile<'_>,
    video_codec: super::mkv::MkvVideoCodec,
) -> Result<Vec<u8>, Error> {
    let decoded_frames = vqa.decode_frames()?;
    if decoded_frames.is_empty() {
        return Err(Error::DecompressionError {
            reason: "VQA produced no decoded frames",
        });
    }

    let width = vqa.header.width as u32;
    let height = vqa.header.height as u32;
    let fps = (vqa.header.fps as u32).max(1);

    // Convert palette-indexed frames to RGBA for MKV encoding.
    let mut rgba_frames: Vec<Vec<u8>> = Vec::with_capacity(decoded_frames.len());
    for frame in &decoded_frames {
        let pixel_count = frame.pixels.len();
        let mut rgba = Vec::with_capacity(pixel_count.saturating_mul(4));
        for &idx in &frame.pixels {
            let base = (idx as usize).saturating_mul(3);
            let r = frame.palette.get(base).copied().unwrap_or(0);
            let g = frame
                .palette
                .get(base.saturating_add(1))
                .copied()
                .unwrap_or(0);
            let b = frame
                .palette
                .get(base.saturating_add(2))
                .copied()
                .unwrap_or(0);
            rgba.push(r);
            rgba.push(g);
            rgba.push(b);
            rgba.push(255);
        }
        rgba_frames.push(rgba);
    }

    // Extract audio (if present).
    let extracted = vqa.extract_audio()?;
    let mkv_audio = extracted.as_ref().map(|a| super::mkv::MkvAudio {
        samples: a.samples.as_slice(),
        sample_rate: a.sample_rate as u32,
        channels: a.channels as u16,
    });

    super::mkv::encode_mkv(
        &rgba_frames,
        width,
        height,
        fps,
        mkv_audio.as_ref(),
        video_codec,
    )
}

// ─── GIF encoding helper ─────────────────────────────────────────────────────

/// Encodes palette-indexed animation frames as an animated GIF.
///
/// Each frame is a flat `Vec<u8>` of palette indices (w × h bytes).
/// The palette is an array of 256 RGB triplets in 8-bit range.
/// Index 0 is the transparent color.
fn encode_animated_gif<T: AsRef<[u8]>>(
    frames: &[T],
    width: u16,
    height: u16,
    rgb8_palette: &[[u8; 3]; 256],
    delay_cs: u16,
) -> Result<Vec<u8>, Error> {
    // Build the flat 768-byte GIF palette (R, G, B × 256 entries).
    let mut flat_palette = [0u8; 768];
    for (i, &[r, g, b]) in rgb8_palette.iter().enumerate() {
        let base = i.saturating_mul(3);
        if let Some(slot) = flat_palette.get_mut(base..base.saturating_add(3)) {
            slot.copy_from_slice(&[r, g, b]);
        }
    }

    let mut buf = Vec::new();
    {
        let mut encoder =
            gif::Encoder::new(&mut buf, width, height, &flat_palette).map_err(|_| {
                Error::DecompressionError {
                    reason: "GIF encoder initialization failed",
                }
            })?;
        // Set the animation to loop infinitely.
        encoder
            .set_repeat(gif::Repeat::Infinite)
            .map_err(|_| Error::DecompressionError {
                reason: "GIF encoder failed to set repeat",
            })?;

        for indexed_pixels in frames {
            let indexed_pixels = indexed_pixels.as_ref();
            let frame = gif::Frame {
                width,
                height,
                delay: delay_cs,
                transparent: Some(0),
                dispose: gif::DisposalMethod::Background,
                buffer: std::borrow::Cow::Borrowed(indexed_pixels),
                ..gif::Frame::default()
            };
            encoder
                .write_frame(&frame)
                .map_err(|_| Error::DecompressionError {
                    reason: "GIF encoder failed to write frame",
                })?;
        }
    }
    Ok(buf)
}
