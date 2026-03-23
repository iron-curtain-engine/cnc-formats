// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! `check` subcommand — deep structural integrity verification beyond
//! what `validate` does.
//!
//! `validate` answers "does this file parse?"
//! `check` answers "is this file internally consistent?"
//!
//! ## Additional checks
//!
//! - **Archives (MIX, BIG, MEG):** detect overlapping entry ranges.

use super::{print_format_hint, read_file, resolve_format, Format};

// ── check ────────────────────────────────────────────────────────────────

/// Parse the file, run deeper integrity checks, and report results.
pub(crate) fn cmd_check(path: &str, explicit: Option<Format>) -> i32 {
    let fmt = resolve_format(path, explicit);
    let data = read_file(path);

    // Step 1: parse (same as validate).
    if let Err(e) = parse_for_check(&data, &fmt) {
        eprintln!("FAIL: {path}: parse error: {e}");
        print_format_hint(path);
        return 1;
    }

    // Step 2: format-specific deep checks.
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    match fmt {
        Format::Mix => check_mix(&data, &mut warnings, &mut errors),
        Format::Big => check_big(&data, &mut warnings, &mut errors),
        Format::Shp => check_shp(&data, &mut errors),
        Format::Wsa => check_wsa(&data, &mut errors),
        #[cfg(feature = "meg")]
        Format::Meg => check_meg(&data, &mut warnings, &mut errors),
        _ => {
            // Non-archive formats: parse success is the only check.
        }
    }

    // Report.
    for w in &warnings {
        eprintln!("  WARN: {w}");
    }
    for e in &errors {
        eprintln!("  FAIL: {e}");
    }

    if errors.is_empty() {
        let extra = if warnings.is_empty() {
            String::new()
        } else {
            format!(
                " ({} warning{})",
                warnings.len(),
                if warnings.len() == 1 { "" } else { "s" }
            )
        };
        println!("OK: {path}{extra}");
        0
    } else {
        eprintln!(
            "FAIL: {path} ({} error{}, {} warning{})",
            errors.len(),
            if errors.len() == 1 { "" } else { "s" },
            warnings.len(),
            if warnings.len() == 1 { "" } else { "s" },
        );
        1
    }
}

/// Parse the data as the given format (same as validate).
fn parse_for_check(data: &[u8], fmt: &Format) -> Result<(), cnc_formats::Error> {
    match fmt {
        Format::Mix => {
            cnc_formats::mix::MixArchive::parse(data)?;
        }
        Format::Big => {
            cnc_formats::big::BigArchive::parse(data)?;
        }
        Format::Shp => {
            cnc_formats::shp::ShpFile::parse(data)?;
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

/// Deep integrity checks for BIG archives.
#[allow(clippy::ptr_arg)]
fn check_big(data: &[u8], _warnings: &mut Vec<String>, errors: &mut Vec<String>) {
    let archive = match cnc_formats::big::BigArchive::parse(data) {
        Ok(a) => a,
        Err(_) => return,
    };

    let entries = archive.entries();

    if entries.len() >= 2 {
        let mut sorted: Vec<(u64, u64, &str)> = entries
            .iter()
            .map(|e| (e.offset, e.size, e.name.as_str()))
            .collect();
        sorted.sort_by_key(|&(off, _, _)| off);

        for pair in sorted.windows(2) {
            let Some((prev_off, prev_size, prev_name)) = pair.first().copied() else {
                continue;
            };
            let Some((cur_off, _, cur_name)) = pair.get(1).copied() else {
                continue;
            };
            let prev_end = prev_off.saturating_add(prev_size);
            if prev_end > cur_off && prev_size > 0 {
                errors.push(format!(
                    "overlapping entries: \"{prev_name}\" (offset {prev_off}, \
                     size {prev_size}) overlaps \"{cur_name}\" (offset {cur_off})"
                ));
            }
        }
    }
}

/// Deep integrity checks for SHP sprites.
fn check_shp(data: &[u8], errors: &mut Vec<String>) {
    let shp = match cnc_formats::shp::ShpFile::parse(data) {
        Ok(shp) => shp,
        Err(_) => return,
    };

    if let Err(e) = shp.decode_frames() {
        errors.push(format!("frame decode failed: {e}"));
    }
}

/// Deep integrity checks for WSA animations.
fn check_wsa(data: &[u8], errors: &mut Vec<String>) {
    let wsa = match cnc_formats::wsa::WsaFile::parse(data) {
        Ok(wsa) => wsa,
        Err(_) => return,
    };

    if let Err(e) = wsa.decode_frames() {
        errors.push(format!("frame decode failed: {e}"));
    }
}

// ── MIX checks ───────────────────────────────────────────────────────────

/// Deep integrity checks for MIX archives.
fn check_mix(data: &[u8], warnings: &mut Vec<String>, errors: &mut Vec<String>) {
    let archive = match cnc_formats::mix::MixArchive::parse(data) {
        Ok(a) => a,
        Err(_) => return, // Already reported by parse_for_check.
    };

    let entries = archive.entries();

    // Check for overlapping entry ranges.
    if entries.len() >= 2 {
        let mut sorted: Vec<(u32, u32, u32)> = entries
            .iter()
            .map(|e| (e.offset, e.size, e.crc.to_raw()))
            .collect();
        sorted.sort_by_key(|&(off, _, _)| off);

        for pair in sorted.windows(2) {
            let Some((prev_off, prev_size, prev_crc)) = pair.first().copied() else {
                continue;
            };
            let Some((cur_off, _, cur_crc)) = pair.get(1).copied() else {
                continue;
            };
            let prev_end = prev_off.saturating_add(prev_size);
            if prev_end > cur_off && prev_size > 0 {
                errors.push(format!(
                    "overlapping entries: CRC 0x{prev_crc:08X} (offset {prev_off}, \
                     size {prev_size}) overlaps CRC 0x{cur_crc:08X} (offset {cur_off})"
                ));
            }
        }
    }

    // Check entry order (Westwood convention: signed i32 CRC sort on disk).
    // After parsing, entries are re-sorted by unsigned CRC.  This is informational.
    let unsigned_sorted = entries.windows(2).all(|w| {
        let first = w.first().map(|entry| entry.crc);
        let second = w.get(1).map(|entry| entry.crc);
        match (first, second) {
            (Some(first), Some(second)) => first <= second,
            _ => true,
        }
    });
    if !unsigned_sorted {
        warnings.push("entries not sorted by unsigned CRC (unexpected after parse)".to_string());
    }
}

/// Deep integrity checks for MEG archives.
#[cfg(feature = "meg")]
#[allow(clippy::ptr_arg)]
fn check_meg(data: &[u8], _warnings: &mut Vec<String>, errors: &mut Vec<String>) {
    let archive = match cnc_formats::meg::MegArchive::parse(data) {
        Ok(a) => a,
        Err(_) => return,
    };

    let entries = archive.entries();

    // Check for overlapping entry ranges.
    if entries.len() >= 2 {
        let mut sorted: Vec<(u64, u64, &str)> = entries
            .iter()
            .map(|e| (e.offset, e.size, e.name.as_str()))
            .collect();
        sorted.sort_by_key(|&(off, _, _)| off);

        for pair in sorted.windows(2) {
            let Some((prev_off, prev_size, prev_name)) = pair.first().copied() else {
                continue;
            };
            let Some((cur_off, _, cur_name)) = pair.get(1).copied() else {
                continue;
            };
            let prev_end = prev_off.saturating_add(prev_size);
            if prev_end > cur_off && prev_size > 0 {
                errors.push(format!(
                    "overlapping entries: \"{prev_name}\" (offset {prev_off}, \
                     size {prev_size}) overlaps \"{cur_name}\" (offset {cur_off})"
                ));
            }
        }
    }
}
