// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Shared helpers used by all `cncf` subcommands.
//!
//! Provides:
//! - **I/O**: [`read_file`], [`open_file`] — read bytes or open a file handle,
//!   exiting with a diagnostic on failure.
//! - **Format detection**: [`resolve_format`] — resolves an explicit `--format`
//!   flag or falls back to extension-based auto-detection.
//! - **Error reporting**: [`print_format_hint`], [`report_parse_error`],
//!   [`report_convert_error`] — consistent diagnostic messages.
//! - **MIX name maps**: [`load_name_map`], [`build_mix_name_map_reader`],
//!   [`build_mix_name_map_archive`] — CRC→filename resolution for `list`
//!   and `extract`.
//! - **Shared subcommands**: [`cmd_identify`], [`cmd_validate`] — the
//!   `identify` and `validate` implementations used from `main.rs`.

use super::Format;
use std::collections::HashMap;
use std::io::{Read, Seek};
use std::process;

/// Read the file at `path` into a byte vector, or exit with an error.
pub(crate) fn read_file(path: &str) -> Vec<u8> {
    match std::fs::read(path) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error reading {path}: {e}");
            process::exit(1);
        }
    }
}

/// Open the file at `path`, or exit with an error.
pub(crate) fn open_file(path: &str) -> std::fs::File {
    match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Error opening {path}: {e}");
            process::exit(1);
        }
    }
}

/// Resolve the final format: explicit `--format` wins, then auto-detect.
pub(crate) fn resolve_format(path: &str, explicit: Option<Format>) -> Format {
    if let Some(f) = explicit {
        return f;
    }
    match detect_format(path) {
        Some(f) => f,
        None => {
            eprintln!(
                "Cannot detect format from extension. \
                 Use --format to specify explicitly \
                 (e.g. --format tmp or --format tmp-ra for .tmp files)."
            );
            process::exit(1);
        }
    }
}

/// Print a hint about `--format` override when a parse error might be caused
/// by format misdetection.
pub(crate) fn print_format_hint(path: &str) {
    let ext = path.rsplit('.').next().unwrap_or("");
    if ext.eq_ignore_ascii_case("tmp") {
        eprintln!("  Hint: .tmp files are ambiguous. Try --format tmp or --format tmp-ra.");
    } else if !ext.is_empty() {
        eprintln!(
            "  Hint: if the file was misdetected, use --format to override \
             (e.g. --format shp)."
        );
    }
}

/// Report a parse error with the source file path and diagnostic context.
#[cfg(any(feature = "convert", feature = "miniyaml"))]
pub(crate) fn report_parse_error(path: &str, e: &cnc_formats::Error) {
    eprintln!("Error: failed to parse {path}");
    eprintln!("  {e}");
    print_format_hint(path);
}

/// Report a conversion error with the source file path.
#[cfg(feature = "convert")]
pub(crate) fn report_convert_error(path: &str, e: &cnc_formats::Error) {
    eprintln!("Error: conversion failed for {path}");
    eprintln!("  {e}");
}

/// Build a human-readable list of supported archive format names.
///
/// This is used in error messages so they stay accurate regardless of
/// which features are compiled in.
pub(crate) fn supported_archive_list() -> String {
    #[cfg(feature = "meg")]
    {
        ["mix", "big", "pak", "bag-idx", "meg", "pgm"].join(", ")
    }
    #[cfg(not(feature = "meg"))]
    {
        "mix, big, pak, bag-idx".to_string()
    }
}

/// Load a name-to-CRC mapping from a text file (one filename per line).
///
/// Used by `list` and `extract` to resolve MIX CRC hashes back to
/// human-readable filenames.  Lines starting with '#' are comments;
/// empty lines are skipped.  Each filename is hashed with the Westwood
/// MIX CRC algorithm and stored in a reverse lookup map.
pub(crate) fn load_name_map(
    path: &str,
) -> Result<HashMap<cnc_formats::mix::MixCrc, String>, std::io::Error> {
    let content = std::fs::read_to_string(path)?;
    let mut map = HashMap::new();
    for line in content.lines() {
        let name = line.trim();
        if name.is_empty() || name.starts_with('#') {
            continue;
        }
        let crc = cnc_formats::mix::crc(name);
        map.insert(crc, name.to_string());
    }
    Ok(map)
}

/// Build a CRC→filename map for a streaming MIX archive reader.
pub(crate) fn build_mix_name_map_reader<R: Read + Seek>(
    archive: &mut cnc_formats::mix::MixArchiveReader<R>,
) -> Result<HashMap<cnc_formats::mix::MixCrc, String>, cnc_formats::Error> {
    let embedded = archive.embedded_names()?;
    if !embedded.is_empty() {
        eprintln!(
            "Using embedded XCC filename database ({} names)",
            embedded.len()
        );
        return Ok(embedded);
    }

    let builtin = cnc_formats::mix::builtin_name_map();
    let stats = cnc_formats::mix::builtin_name_stats();
    eprintln!(
        "Using built-in MIX name resolver ({} unique CRCs, {} ambiguous CRCs omitted)",
        stats.resolved_crc_count, stats.ambiguous_crc_count
    );
    Ok(builtin)
}

/// Build a CRC→filename map for an eagerly parsed MIX archive.
pub(crate) fn build_mix_name_map_archive(
    archive: &cnc_formats::mix::MixArchive<'_>,
) -> HashMap<cnc_formats::mix::MixCrc, String> {
    let embedded = archive.embedded_names();
    if !embedded.is_empty() {
        eprintln!(
            "Using embedded XCC filename database ({} names)",
            embedded.len()
        );
        return embedded;
    }

    let builtin = cnc_formats::mix::builtin_name_map();
    let stats = cnc_formats::mix::builtin_name_stats();
    eprintln!(
        "Using built-in MIX name resolver ({} unique CRCs, {} ambiguous CRCs omitted)",
        stats.resolved_crc_count, stats.ambiguous_crc_count
    );
    builtin
}

/// Sniff a likely format from file contents.
pub(crate) fn cmd_identify(path: &str) -> i32 {
    let data = read_file(path);
    if let Some(fmt) = cnc_formats::sniff::sniff_format(&data) {
        println!("{fmt}");
        return 0;
    }
    if let Some(fmt) = detect_format(path) {
        println!("{}", format_name(&fmt));
        return 0;
    }

    println!("unknown");
    1
}

/// Parse the file and report success or failure.  Exit code 0 = valid,
/// 1 = parse error.
pub(crate) fn cmd_validate(path: &str, explicit: Option<Format>) -> i32 {
    let fmt = resolve_format(path, explicit);
    let data = read_file(path);
    let result = validate_data(&data, &fmt);
    match result {
        Ok(()) => {
            println!("OK: {path}");
            0
        }
        Err(e) => {
            eprintln!("INVALID: {path}: {e}");
            print_format_hint(path);
            1
        }
    }
}

/// Detect the format from the file extension.  Returns `None` for unknown
/// extensions.
fn detect_format(path: &str) -> Option<Format> {
    let ext = path.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "mix" => Some(Format::Mix),
        "big" => Some(Format::Big),
        "shp" => Some(Format::Shp),
        "pal" => Some(Format::Pal),
        "aud" => Some(Format::Aud),
        "lut" => Some(Format::Lut),
        "dip" => Some(Format::Dip),
        "vqa" => Some(Format::Vqa),
        "vqp" => Some(Format::Vqp),
        "wsa" => Some(Format::Wsa),
        "fnt" => Some(Format::Fnt),
        "eng" | "ger" | "fre" => Some(Format::Eng),
        "ini" => Some(Format::Ini),
        "vxl" => Some(Format::Vxl),
        "hva" => Some(Format::Hva),
        "csf" => Some(Format::Csf),
        "cps" => Some(Format::Cps),
        "w3d" => Some(Format::W3d),
        #[cfg(feature = "convert")]
        "avi" => Some(Format::Avi),
        #[cfg(feature = "miniyaml")]
        "miniyaml" => Some(Format::Miniyaml),
        #[cfg(feature = "midi")]
        "mid" | "midi" => Some(Format::Mid),
        #[cfg(feature = "adl")]
        "adl" => Some(Format::Adl),
        #[cfg(feature = "xmi")]
        "xmi" => Some(Format::Xmi),
        "voc" => Some(Format::Voc),
        "pak" => Some(Format::Pak),
        "icn" => Some(Format::Icn),
        "bin" => Some(Format::BinTd),
        "mpr" => Some(Format::Mpr),
        "bag" | "idx" => Some(Format::BagIdx),
        "wnd" => Some(Format::Wnd),
        "str" => Some(Format::SageStr),
        "apt" => Some(Format::Apt),
        "dds" => Some(Format::Dds),
        "tga" => Some(Format::Tga),
        // JPG/JPEG: detected by sniff (identify command) but no parser module — not mapped here.
        #[cfg(feature = "meg")]
        "meg" | "pgm" => Some(Format::Meg),
        _ => None,
    }
}

fn format_name(fmt: &Format) -> &'static str {
    match fmt {
        Format::Mix => "mix",
        Format::Big => "big",
        Format::Shp => "shp",
        Format::Pal => "pal",
        Format::Aud => "aud",
        Format::Lut => "lut",
        Format::Dip => "dip",
        Format::Tmp => "tmp",
        Format::TmpRa => "tmp-ra",
        Format::Vqa => "vqa",
        Format::Vqp => "vqp",
        Format::Wsa => "wsa",
        Format::Fnt => "fnt",
        Format::Eng => "eng",
        Format::Ini => "ini",
        Format::Vxl => "vxl",
        Format::Hva => "hva",
        Format::ShpTs => "shp-ts",
        Format::Csf => "csf",
        Format::Cps => "cps",
        Format::W3d => "w3d",
        Format::TmpTs => "tmp-ts",
        Format::Voc => "voc",
        Format::Pak => "pak",
        Format::ShpD2 => "shp-d2",
        Format::Icn => "icn",
        Format::D2Map => "d2-map",
        Format::BinTd => "bin-td",
        Format::Mpr => "mpr",
        Format::BagIdx => "bag-idx",
        Format::MapRa2 => "map-ra2",
        Format::Wnd => "wnd",
        Format::SageStr => "sage-str",
        Format::MapSage => "map-sage",
        Format::Apt => "apt",
        Format::Dds => "dds",
        Format::Tga => "tga",
        #[cfg(feature = "convert")]
        Format::Avi => "avi",
        #[cfg(feature = "miniyaml")]
        Format::Miniyaml => "miniyaml",
        #[cfg(feature = "midi")]
        Format::Mid => "mid",
        #[cfg(feature = "adl")]
        Format::Adl => "adl",
        #[cfg(feature = "xmi")]
        Format::Xmi => "xmi",
        #[cfg(feature = "meg")]
        Format::Meg => "meg",
    }
}

/// Attempt to parse `data` as the given format.  Returns `Ok(())` if
/// parsing succeeds.
fn validate_data(data: &[u8], fmt: &Format) -> Result<(), cnc_formats::Error> {
    match fmt {
        Format::Mix => {
            cnc_formats::mix::MixArchive::parse(data)?;
        }
        Format::Big => {
            cnc_formats::big::BigArchive::parse(data)?;
        }
        Format::Shp => {
            let shp = cnc_formats::shp::ShpFile::parse(data)?;
            let pixel_count = shp.frame_pixel_count();
            for frame in &shp.frames {
                let _ = cnc_formats::lcw::decompress(frame.data, pixel_count)?;
            }
        }
        Format::Pal => {
            cnc_formats::pal::Palette::parse(data)?;
        }
        Format::Aud => {
            cnc_formats::aud::AudFile::parse(data)?;
        }
        Format::Lut => {
            cnc_formats::lut::LutFile::parse(data)?;
        }
        Format::Dip => {
            cnc_formats::dip::DipFile::parse(data)?;
        }
        Format::Tmp => {
            cnc_formats::tmp::TdTmpFile::parse(data)?;
        }
        Format::TmpRa => {
            cnc_formats::tmp::RaTmpFile::parse(data)?;
        }
        Format::Vqa => {
            cnc_formats::vqa::VqaFile::parse(data)?;
        }
        Format::Vqp => {
            cnc_formats::vqp::VqpFile::parse(data)?;
        }
        Format::Wsa => {
            cnc_formats::wsa::WsaFile::parse(data)?;
        }
        Format::Fnt => {
            cnc_formats::fnt::FntFile::parse(data)?;
        }
        Format::Eng => {
            cnc_formats::eng::EngFile::parse(data)?;
        }
        Format::Ini => {
            cnc_formats::ini::IniFile::parse(data)?;
        }
        Format::Vxl => {
            cnc_formats::vxl::VxlFile::parse(data)?;
        }
        Format::Hva => {
            cnc_formats::hva::HvaFile::parse(data)?;
        }
        Format::ShpTs => {
            cnc_formats::shp_ts::ShpTsFile::parse(data)?;
        }
        Format::Csf => {
            cnc_formats::csf::CsfFile::parse(data)?;
        }
        Format::Cps => {
            cnc_formats::cps::CpsFile::parse(data)?;
        }
        Format::W3d => {
            cnc_formats::w3d::W3dFile::parse(data)?;
        }
        Format::TmpTs => {
            cnc_formats::tmp::TsTmpFile::parse(data)?;
        }
        Format::Voc => {
            cnc_formats::voc::VocFile::parse(data)?;
        }
        Format::Pak => {
            cnc_formats::pak::PakArchive::parse(data)?;
        }
        Format::ShpD2 => {
            cnc_formats::shp_d2::ShpD2File::parse(data)?;
        }
        Format::Icn => {
            cnc_formats::icn::IcnFile::parse(data, 16, 16)?;
        }
        Format::D2Map => {
            cnc_formats::d2_map::D2Scenario::parse(data)?;
        }
        Format::BinTd => {
            cnc_formats::bin_td::BinMap::parse(data, 64, 64)?;
        }
        Format::Mpr => {
            cnc_formats::mpr::MprFile::parse(data)?;
        }
        Format::BagIdx => {
            cnc_formats::bag_idx::IdxFile::parse(data)?;
        }
        Format::MapRa2 => {
            cnc_formats::map_ra2::MapRa2File::parse(data)?;
        }
        Format::Wnd => {
            cnc_formats::wnd::WndFile::parse(data)?;
        }
        Format::SageStr => {
            cnc_formats::sage_str::StrFile::parse(data)?;
        }
        Format::MapSage => {
            cnc_formats::map_sage::MapSageFile::parse(data)?;
        }
        Format::Apt => {
            cnc_formats::apt::AptFile::parse(data)?;
        }
        Format::Dds => {
            cnc_formats::dds::DdsFile::parse(data)?;
        }
        Format::Tga => {
            cnc_formats::tga::TgaFile::parse(data)?;
        }
        #[cfg(feature = "miniyaml")]
        Format::Miniyaml => {
            cnc_formats::miniyaml::MiniYamlDoc::parse(data)?;
        }
        #[cfg(feature = "convert")]
        Format::Avi => {
            cnc_formats::convert::decode_avi(data)?;
        }
        #[cfg(feature = "midi")]
        Format::Mid => {
            cnc_formats::mid::MidFile::parse(data)?;
        }
        #[cfg(feature = "adl")]
        Format::Adl => {
            cnc_formats::adl::AdlFile::parse(data)?;
        }
        #[cfg(feature = "xmi")]
        Format::Xmi => {
            cnc_formats::xmi::XmiFile::parse(data)?;
        }
        #[cfg(feature = "meg")]
        Format::Meg => {
            cnc_formats::meg::MegArchive::parse(data)?;
        }
    }
    Ok(())
}
