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
//! - **Archives (MIX, MEG):** detect overlapping entry ranges.

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
        Format::Shp => {
            cnc_formats::shp::ShpFile::parse(data)?;
        }
        Format::Pal => {
            cnc_formats::pal::Palette::parse(data)?;
        }
        Format::Aud => {
            cnc_formats::aud::AudFile::parse(data)?;
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
        Format::Wsa => {
            cnc_formats::wsa::WsaFile::parse(data)?;
        }
        Format::Fnt => {
            cnc_formats::fnt::FntFile::parse(data)?;
        }
        Format::Ini => {
            cnc_formats::ini::IniFile::parse(data)?;
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

        for i in 1..sorted.len() {
            let (prev_off, prev_size, prev_crc) = sorted[i - 1];
            let (cur_off, _, cur_crc) = sorted[i];
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
    let unsigned_sorted = entries.windows(2).all(|w| w[0].crc <= w[1].crc);
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

        for i in 1..sorted.len() {
            let (prev_off, prev_size, prev_name) = sorted[i - 1];
            let (cur_off, _, cur_name) = sorted[i];
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
