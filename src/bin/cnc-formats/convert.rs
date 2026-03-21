// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! `convert` subcommand — convert between C&C formats and common formats.

#[cfg(feature = "miniyaml")]
mod text;

#[cfg(feature = "convert")]
mod export;
#[cfg(feature = "convert")]
mod import;

#[cfg(feature = "convert")]
use super::read_file;
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
    output_path: Option<&str>,
) -> i32 {
    match to {
        #[cfg(feature = "miniyaml")]
        ConvertTarget::Yaml => text::convert_miniyaml_to_yaml(path, explicit_format, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Png => {
            export::convert_to_png(path, explicit_format, palette_path, output_path)
        }
        #[cfg(feature = "convert")]
        ConvertTarget::Wav => export::convert_to_wav(path, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Gif => {
            export::convert_to_gif(path, explicit_format, palette_path, output_path)
        }
        #[cfg(feature = "convert")]
        ConvertTarget::Shp => import::convert_to_shp(path, palette_path, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Aud => import::convert_to_aud(path, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Wsa => import::convert_to_wsa(path, palette_path, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Pal => import::convert_to_pal(path, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Tmp => import::convert_to_tmp(path, palette_path, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Avi => import::convert_to_avi(path, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Mkv => import::convert_to_mkv(path, output_path),
        #[cfg(feature = "convert")]
        ConvertTarget::Vqa => import::convert_to_vqa(path, palette_path, output_path),
    }
}

// ── Palette helpers ──────────────────────────────────────────────────────────

/// Read and parse a palette file, or exit with an error message.
#[cfg(feature = "convert")]
pub(super) fn load_palette(palette_path: Option<&str>) -> Option<cnc_formats::pal::Palette> {
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
pub(super) fn require_palette(
    palette_path: Option<&str>,
    format_name: &str,
) -> cnc_formats::pal::Palette {
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
pub(super) fn write_png_sequence<T: AsRef<[u8]>>(
    pngs: &[T],
    output_dir: &str,
    prefix: &str,
) -> i32 {
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
        if let Err(e) = std::fs::write(&name, png_data.as_ref()) {
            eprintln!("Error writing {name}: {e}");
            return 1;
        }
    }
    println!("Wrote {} PNG files to {output_dir}/", pngs.len());
    0
}

/// Derive an output path from the input path by replacing its extension.
#[cfg(feature = "convert")]
pub(super) fn derive_output(input: &str, ext: &str) -> String {
    match input.rsplit_once('.') {
        Some((stem, _)) => format!("{stem}.{ext}"),
        None => format!("{input}.{ext}"),
    }
}

/// Derive an output directory from the input path.
#[cfg(feature = "convert")]
pub(super) fn derive_output_dir(input: &str) -> String {
    match input.rsplit_once('.') {
        Some((stem, _)) => stem.to_string(),
        None => format!("{input}_out"),
    }
}

/// Write a single output file and print confirmation.
#[cfg(feature = "convert")]
pub(super) fn write_single_file(path: &str, data: &[u8]) -> i32 {
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
