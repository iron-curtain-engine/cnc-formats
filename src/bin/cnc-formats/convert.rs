// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! `convert` subcommand — convert between C&C formats and common formats.

#[cfg(feature = "miniyaml")]
mod text;

#[cfg(feature = "convert")]
use super::{read_file, resolve_format};
use super::{ConvertTarget, Format};
#[cfg(feature = "convert")]
use std::process;

// ── convert ──────────────────────────────────────────────────────────────────

/// Unified convert dispatcher.  Routes to the appropriate conversion based
/// on the target format and auto-detected (or overridden) source format.
pub(crate) fn cmd_convert(
    path: &str,
    to: ConvertTarget,
    explicit_format: Option<Format>,
    #[cfg(feature = "convert")] palette_path: Option<&str>,
    #[cfg(feature = "convert")] output_path: Option<&str>,
) -> i32 {
    match to {
        #[cfg(feature = "miniyaml")]
        ConvertTarget::Yaml => text::convert_miniyaml_to_yaml(path, explicit_format),
        #[cfg(feature = "convert")]
        ConvertTarget::Png => convert_to_png(path, explicit_format, palette_path, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Wav => convert_to_wav(path, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Gif => convert_to_gif(path, explicit_format, palette_path, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Shp => convert_to_shp(path, palette_path, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Aud => convert_to_aud(path, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Wsa => convert_to_wsa(path, palette_path, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Pal => convert_to_pal(path, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Tmp => convert_to_tmp(path, palette_path, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Avi => convert_to_avi(path, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Vqa => convert_to_vqa(path, palette_path, output_path),
    }
}

// ── Palette helpers ──────────────────────────────────────────────────────────

/// Read and parse a palette file, or exit with an error message.
#[cfg(feature = "convert")]
fn load_palette(palette_path: Option<&str>) -> Option<cnc_formats::pal::Palette> {
    let path = palette_path?;
    let data = read_file(path);
    match cnc_formats::pal::Palette::parse(&data) {
        Ok(pal) => Some(pal),
        Err(e) => {
            eprintln!("Error parsing palette {path}: {e}");
            process::exit(1);
        }
    }
}

/// Require a palette, printing an error if absent.
#[cfg(feature = "convert")]
fn require_palette(palette_path: Option<&str>, format_name: &str) -> cnc_formats::pal::Palette {
    match load_palette(palette_path) {
        Some(p) => p,
        None => {
            eprintln!("{format_name} requires a palette file.");
            eprintln!("  Use: --palette <file.pal>");
            eprintln!(
                "  Palettes can be extracted from C&C game data \
                 (e.g. temperat.pal, snow.pal)."
            );
            process::exit(1);
        }
    }
}

// ── Output helpers ───────────────────────────────────────────────────────────

/// Write multiple PNG files to a directory with zero-padded numeric names.
#[cfg(feature = "convert")]
fn write_png_sequence(pngs: &[Vec<u8>], output_dir: &str, prefix: &str) -> i32 {
    if let Err(e) = std::fs::create_dir_all(output_dir) {
        eprintln!("Error creating directory {output_dir}: {e}");
        return 1;
    }
    let digits = if pngs.is_empty() {
        1
    } else {
        (pngs.len() as f64).log10() as usize + 1
    };
    for (i, png_data) in pngs.iter().enumerate() {
        let name = format!("{output_dir}/{prefix}_{:0>width$}.png", i, width = digits);
        if let Err(e) = std::fs::write(&name, png_data) {
            eprintln!("Error writing {name}: {e}");
            return 1;
        }
    }
    println!("Wrote {} PNG files to {output_dir}/", pngs.len());
    0
}

/// Derive an output path from the input path by replacing its extension.
#[cfg(feature = "convert")]
fn derive_output(input: &str, ext: &str) -> String {
    match input.rsplit_once('.') {
        Some((stem, _)) => format!("{stem}.{ext}"),
        None => format!("{input}.{ext}"),
    }
}

/// Derive an output directory from the input path.
#[cfg(feature = "convert")]
fn derive_output_dir(input: &str) -> String {
    match input.rsplit_once('.') {
        Some((stem, _)) => stem.to_string(),
        None => format!("{input}_out"),
    }
}

/// Write a single output file and print confirmation.
#[cfg(feature = "convert")]
fn write_single_file(path: &str, data: &[u8]) -> i32 {
    match std::fs::write(path, data) {
        Ok(()) => {
            println!("Wrote {} bytes to {path}", data.len());
            0
        }
        Err(e) => {
            eprintln!("Error writing {path}: {e}");
            1
        }
    }
}

// ── Export conversions (C&C → common) ────────────────────────────────────────

/// Convert a file to PNG based on its detected source format.
#[cfg(feature = "convert")]
fn convert_to_png(
    path: &str,
    explicit: Option<Format>,
    palette_path: Option<&str>,
    output: Option<&str>,
) -> i32 {
    let fmt = resolve_format(path, explicit);
    let data = read_file(path);

    match fmt {
        Format::Shp => {
            let pal = require_palette(palette_path, "SHP");
            match cnc_formats::shp::ShpFile::parse(&data) {
                Ok(shp) => {
                    eprintln!(
                        "Converting SHP → PNG ({} frames, {}×{})...",
                        shp.header.frame_count, shp.header.width, shp.header.height
                    );
                    match cnc_formats::convert::shp_frames_to_png(&shp, &pal) {
                        Ok(pngs) => {
                            let dir_default = derive_output_dir(path);
                            let dir = output.unwrap_or(&dir_default);
                            write_png_sequence(&pngs, dir, "frame")
                        }
                        Err(e) => {
                            super::report_convert_error(path, &e);
                            1
                        }
                    }
                }
                Err(e) => {
                    super::report_parse_error(path, &e);
                    1
                }
            }
        }
        Format::Pal => match cnc_formats::pal::Palette::parse(&data) {
            Ok(pal) => {
                eprintln!("Converting PAL → PNG (256-color swatch)...");
                match cnc_formats::convert::pal_to_png(&pal) {
                    Ok(png_data) => {
                        let out = output
                            .map(String::from)
                            .unwrap_or_else(|| derive_output(path, "png"));
                        write_single_file(&out, &png_data)
                    }
                    Err(e) => {
                        super::report_convert_error(path, &e);
                        1
                    }
                }
            }
            Err(e) => {
                super::report_parse_error(path, &e);
                1
            }
        },
        Format::Tmp => {
            let pal = require_palette(palette_path, "TMP (TD)");
            match cnc_formats::tmp::TdTmpFile::parse(&data) {
                Ok(tmp) => {
                    eprintln!("Converting TMP (TD) → PNG ({} tiles)...", tmp.tiles.len());
                    match cnc_formats::convert::td_tmp_tiles_to_png(&tmp, &pal) {
                        Ok(pngs) => {
                            let dir_default = derive_output_dir(path);
                            let dir = output.unwrap_or(&dir_default);
                            write_png_sequence(&pngs, dir, "tile")
                        }
                        Err(e) => {
                            super::report_convert_error(path, &e);
                            1
                        }
                    }
                }
                Err(e) => {
                    super::report_parse_error(path, &e);
                    1
                }
            }
        }
        Format::TmpRa => {
            let pal = require_palette(palette_path, "TMP (RA)");
            match cnc_formats::tmp::RaTmpFile::parse(&data) {
                Ok(tmp) => {
                    let present_count = tmp.tiles.iter().filter(|t| t.is_some()).count();
                    eprintln!("Converting TMP (RA) → PNG ({present_count} tiles)...");
                    match cnc_formats::convert::ra_tmp_tiles_to_png(&tmp, &pal) {
                        Ok(tiles) => {
                            let dir_default = derive_output_dir(path);
                            let dir = output.unwrap_or(&dir_default);
                            let present: Vec<_> = tiles.into_iter().flatten().collect();
                            write_png_sequence(&present, dir, "tile")
                        }
                        Err(e) => {
                            super::report_convert_error(path, &e);
                            1
                        }
                    }
                }
                Err(e) => {
                    super::report_parse_error(path, &e);
                    1
                }
            }
        }
        Format::Wsa => {
            let pal = require_palette(palette_path, "WSA");
            match cnc_formats::wsa::WsaFile::parse(&data) {
                Ok(wsa) => {
                    eprintln!(
                        "Converting WSA → PNG ({} frames, {}×{})...",
                        wsa.header.num_frames, wsa.header.width, wsa.header.height
                    );
                    match cnc_formats::convert::wsa_frames_to_png(&wsa, &pal) {
                        Ok(pngs) => {
                            let dir_default = derive_output_dir(path);
                            let dir = output.unwrap_or(&dir_default);
                            write_png_sequence(&pngs, dir, "frame")
                        }
                        Err(e) => {
                            super::report_convert_error(path, &e);
                            1
                        }
                    }
                }
                Err(e) => {
                    super::report_parse_error(path, &e);
                    1
                }
            }
        }
        Format::Fnt => match cnc_formats::fnt::FntFile::parse(&data) {
            Ok(fnt) => {
                eprintln!("Converting FNT → PNG (font atlas)...");
                match cnc_formats::convert::fnt_to_png(&fnt) {
                    Ok(png_data) => {
                        let out = output
                            .map(String::from)
                            .unwrap_or_else(|| derive_output(path, "png"));
                        write_single_file(&out, &png_data)
                    }
                    Err(e) => {
                        super::report_convert_error(path, &e);
                        1
                    }
                }
            }
            Err(e) => {
                super::report_parse_error(path, &e);
                1
            }
        },
        _ => {
            eprintln!("PNG conversion not supported for this format.");
            eprintln!("Supported: shp, pal, tmp, tmp-ra, wsa, fnt");
            1
        }
    }
}

/// Convert AUD to WAV.
#[cfg(feature = "convert")]
fn convert_to_wav(path: &str, output: Option<&str>) -> i32 {
    let data = read_file(path);
    match cnc_formats::aud::AudFile::parse(&data) {
        Ok(aud) => {
            eprintln!(
                "Converting AUD → WAV ({}Hz, {})...",
                aud.header.sample_rate,
                if aud.header.is_stereo() {
                    "stereo"
                } else {
                    "mono"
                }
            );
            match cnc_formats::convert::aud_to_wav(&aud) {
                Ok(wav_data) => {
                    let out = output
                        .map(String::from)
                        .unwrap_or_else(|| derive_output(path, "wav"));
                    write_single_file(&out, &wav_data)
                }
                Err(e) => {
                    super::report_convert_error(path, &e);
                    1
                }
            }
        }
        Err(e) => {
            super::report_parse_error(path, &e);
            1
        }
    }
}

/// Convert SHP or WSA to animated GIF.
#[cfg(feature = "convert")]
fn convert_to_gif(
    path: &str,
    explicit: Option<Format>,
    palette_path: Option<&str>,
    output: Option<&str>,
) -> i32 {
    let fmt = resolve_format(path, explicit);
    let data = read_file(path);

    // Default delay: 10 centiseconds = 100ms per frame ≈ 10 fps.
    let delay_cs: u16 = 10;

    match fmt {
        Format::Shp => {
            let pal = require_palette(palette_path, "SHP → GIF");
            match cnc_formats::shp::ShpFile::parse(&data) {
                Ok(shp) => {
                    eprintln!(
                        "Converting SHP → GIF ({} frames, {}×{})...",
                        shp.header.frame_count, shp.header.width, shp.header.height
                    );
                    match cnc_formats::convert::shp_frames_to_gif(&shp, &pal, delay_cs) {
                        Ok(gif_data) => {
                            let out = output
                                .map(String::from)
                                .unwrap_or_else(|| derive_output(path, "gif"));
                            write_single_file(&out, &gif_data)
                        }
                        Err(e) => {
                            super::report_convert_error(path, &e);
                            1
                        }
                    }
                }
                Err(e) => {
                    super::report_parse_error(path, &e);
                    1
                }
            }
        }
        Format::Wsa => {
            let pal = require_palette(palette_path, "WSA → GIF");
            match cnc_formats::wsa::WsaFile::parse(&data) {
                Ok(wsa) => {
                    eprintln!(
                        "Converting WSA → GIF ({} frames, {}×{})...",
                        wsa.header.num_frames, wsa.header.width, wsa.header.height
                    );
                    match cnc_formats::convert::wsa_frames_to_gif(&wsa, &pal, delay_cs) {
                        Ok(gif_data) => {
                            let out = output
                                .map(String::from)
                                .unwrap_or_else(|| derive_output(path, "gif"));
                            write_single_file(&out, &gif_data)
                        }
                        Err(e) => {
                            super::report_convert_error(path, &e);
                            1
                        }
                    }
                }
                Err(e) => {
                    super::report_parse_error(path, &e);
                    1
                }
            }
        }
        _ => {
            eprintln!("GIF conversion not supported for this format.");
            eprintln!("Supported: shp, wsa");
            1
        }
    }
}

// ── Import conversions (common → C&C) ───────────────────────────────────────

/// Convert PNG or GIF to SHP sprite.
#[cfg(feature = "convert")]
fn convert_to_shp(path: &str, palette_path: Option<&str>, output: Option<&str>) -> i32 {
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
            super::report_convert_error(path, &e);
            1
        }
    }
}

/// Convert WAV to AUD.
#[cfg(feature = "convert")]
fn convert_to_aud(path: &str, output: Option<&str>) -> i32 {
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
            super::report_convert_error(path, &e);
            1
        }
    }
}

/// Convert PNG or GIF to WSA animation.
#[cfg(feature = "convert")]
fn convert_to_wsa(path: &str, palette_path: Option<&str>, output: Option<&str>) -> i32 {
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
            super::report_convert_error(path, &e);
            1
        }
    }
}

/// Extract palette from PNG to PAL file.
#[cfg(feature = "convert")]
fn convert_to_pal(path: &str, output: Option<&str>) -> i32 {
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
            super::report_convert_error(path, &e);
            1
        }
    }
}

/// Convert PNG(s) to TMP terrain tiles (TD format).
#[cfg(feature = "convert")]
fn convert_to_tmp(path: &str, palette_path: Option<&str>, output: Option<&str>) -> i32 {
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
            super::report_convert_error(path, &e);
            1
        }
    }
}

/// Convert VQA to AVI video.
#[cfg(feature = "convert")]
fn convert_to_avi(path: &str, output: Option<&str>) -> i32 {
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
                    super::report_convert_error(path, &e);
                    1
                }
            }
        }
        Err(e) => {
            super::report_parse_error(path, &e);
            1
        }
    }
}

/// Convert AVI to VQA video.
#[cfg(feature = "convert")]
fn convert_to_vqa(path: &str, palette_path: Option<&str>, output: Option<&str>) -> i32 {
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
            super::report_convert_error(path, &e);
            1
        }
    }
}
