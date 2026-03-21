// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Import conversions: common formats → C&C formats (SHP, AUD, WSA, PAL, TMP, AVI, MKV, VQA).

#[cfg(feature = "convert")]
use super::super::{read_file, report_convert_error, report_parse_error};
#[cfg(feature = "convert")]
use super::{derive_output, require_palette, write_single_file};

// ── Import conversions (common → C&C) ───────────────────────────────────────

/// Convert PNG or GIF to SHP sprite.
#[cfg(feature = "convert")]
pub(super) fn convert_to_shp(path: &str, palette_path: Option<&str>, output: Option<&str>) -> i32 {
    let pal = require_palette(palette_path, "→ SHP");
    let data = read_file(path);
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();

    eprintln!("Converting {ext} → SHP...");
    let result = if ext == "gif" {
        cnc_formats::convert::gif_to_shp(&data, &pal)
    } else {
        // Treat as PNG (single frame).
        cnc_formats::convert::png_to_shp(&[&data], &pal)
    };

    match result {
        Ok(shp_data) => {
            let out = output
                .map(String::from)
                .unwrap_or_else(|| derive_output(path, "shp"));
            write_single_file(&out, &shp_data)
        }
        Err(e) => {
            report_convert_error(path, &e);
            1
        }
    }
}

/// Convert WAV to AUD.
#[cfg(feature = "convert")]
pub(super) fn convert_to_aud(path: &str, output: Option<&str>) -> i32 {
    let data = read_file(path);
    eprintln!("Converting WAV → AUD (IMA ADPCM)...");
    match cnc_formats::convert::wav_to_aud(&data) {
        Ok(aud_data) => {
            let out = output
                .map(String::from)
                .unwrap_or_else(|| derive_output(path, "aud"));
            write_single_file(&out, &aud_data)
        }
        Err(e) => {
            report_convert_error(path, &e);
            1
        }
    }
}

/// Convert PNG or GIF to WSA animation.
#[cfg(feature = "convert")]
pub(super) fn convert_to_wsa(path: &str, palette_path: Option<&str>, output: Option<&str>) -> i32 {
    let pal = require_palette(palette_path, "→ WSA");
    let data = read_file(path);
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();

    eprintln!("Converting {ext} → WSA...");
    let result = if ext == "gif" {
        cnc_formats::convert::gif_to_wsa(&data, &pal)
    } else {
        // Treat as PNG (single frame).
        cnc_formats::convert::png_to_wsa(&[&data], &pal)
    };

    match result {
        Ok(wsa_data) => {
            let out = output
                .map(String::from)
                .unwrap_or_else(|| derive_output(path, "wsa"));
            write_single_file(&out, &wsa_data)
        }
        Err(e) => {
            report_convert_error(path, &e);
            1
        }
    }
}

/// Extract palette from PNG to PAL file.
#[cfg(feature = "convert")]
pub(super) fn convert_to_pal(path: &str, output: Option<&str>) -> i32 {
    let data = read_file(path);
    eprintln!("Converting PNG → PAL...");
    match cnc_formats::convert::png_to_pal(&data) {
        Ok(pal_data) => {
            let out = output
                .map(String::from)
                .unwrap_or_else(|| derive_output(path, "pal"));
            write_single_file(&out, &pal_data)
        }
        Err(e) => {
            report_convert_error(path, &e);
            1
        }
    }
}

/// Convert PNG(s) to TMP terrain tiles (TD format).
#[cfg(feature = "convert")]
pub(super) fn convert_to_tmp(path: &str, palette_path: Option<&str>, output: Option<&str>) -> i32 {
    let pal = require_palette(palette_path, "→ TMP");
    let data = read_file(path);
    eprintln!("Converting PNG → TMP (TD)...");
    match cnc_formats::convert::png_to_td_tmp(&[&data], &pal) {
        Ok(tmp_data) => {
            let out = output
                .map(String::from)
                .unwrap_or_else(|| derive_output(path, "tmp"));
            write_single_file(&out, &tmp_data)
        }
        Err(e) => {
            report_convert_error(path, &e);
            1
        }
    }
}

/// Convert VQA to AVI video.
#[cfg(feature = "convert")]
pub(super) fn convert_to_avi(path: &str, output: Option<&str>) -> i32 {
    let data = read_file(path);
    match cnc_formats::vqa::VqaFile::parse(&data) {
        Ok(vqa) => {
            eprintln!(
                "Converting VQA → AVI ({} frames, {}×{}, {} fps)...",
                vqa.header.num_frames, vqa.header.width, vqa.header.height, vqa.header.fps
            );
            match cnc_formats::convert::vqa_to_avi(&vqa) {
                Ok(avi_data) => {
                    let out = output
                        .map(String::from)
                        .unwrap_or_else(|| derive_output(path, "avi"));
                    write_single_file(&out, &avi_data)
                }
                Err(e) => {
                    report_convert_error(path, &e);
                    1
                }
            }
        }
        Err(e) => {
            report_parse_error(path, &e);
            1
        }
    }
}

/// Convert VQA to MKV video (Matroska container).
#[cfg(feature = "convert")]
pub(super) fn convert_to_mkv(
    path: &str,
    output: Option<&str>,
    mkv_codec: crate::CliMkvCodec,
) -> i32 {
    let data = read_file(path);
    // Map CLI enum to library enum.
    let video_codec = match mkv_codec {
        crate::CliMkvCodec::Uncompressed => cnc_formats::convert::MkvVideoCodec::Uncompressed,
        crate::CliMkvCodec::Vfw => cnc_formats::convert::MkvVideoCodec::Vfw,
    };
    match cnc_formats::vqa::VqaFile::parse(&data) {
        Ok(vqa) => {
            eprintln!(
                "Converting VQA → MKV ({} frames, {}×{}, {} fps)...",
                vqa.header.num_frames, vqa.header.width, vqa.header.height, vqa.header.fps
            );
            match cnc_formats::convert::vqa_to_mkv(&vqa, video_codec) {
                Ok(mkv_data) => {
                    let out = output
                        .map(String::from)
                        .unwrap_or_else(|| derive_output(path, "mkv"));
                    write_single_file(&out, &mkv_data)
                }
                Err(e) => {
                    report_convert_error(path, &e);
                    1
                }
            }
        }
        Err(e) => {
            report_parse_error(path, &e);
            1
        }
    }
}

/// Convert AVI to VQA video.
#[cfg(feature = "convert")]
pub(super) fn convert_to_vqa(path: &str, palette_path: Option<&str>, output: Option<&str>) -> i32 {
    let pal = require_palette(palette_path, "AVI → VQA");
    let data = read_file(path);
    eprintln!("Decoding AVI and encoding VQA...");
    match cnc_formats::convert::avi_to_vqa(&data, &pal) {
        Ok(vqa_data) => {
            let out = output
                .map(String::from)
                .unwrap_or_else(|| derive_output(path, "vqa"));
            write_single_file(&out, &vqa_data)
        }
        Err(e) => {
            report_convert_error(path, &e);
            1
        }
    }
}
