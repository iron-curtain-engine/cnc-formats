// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! `list` subcommand — quick archive inventory showing CRC, size, and
//! optionally resolved filenames for each entry.
//!
//! Lighter than `inspect` for answering "what's in this `.mix`?".
//! Output goes to stdout so it can be piped to other tools.

use super::{
    build_mix_name_map, load_name_map, read_file, resolve_format, supported_archive_list, Format,
};
use std::collections::HashMap;

use cnc_formats::mix::MixCrc;

// ── list ─────────────────────────────────────────────────────────────────────

/// Parse an archive and print a per-entry inventory.
pub(crate) fn cmd_list(path: &str, explicit: Option<Format>, names: Option<&str>) -> i32 {
    let fmt = resolve_format(path, explicit);

    if !is_archive_format(&fmt) {
        eprintln!(
            "Error: `list` only supports archive formats ({}).\n  \
             To view non-archive files, use `cncf inspect`.",
            supported_archive_list()
        );
        return 1;
    }

    let data = read_file(path);

    match fmt {
        Format::Mix => {
            let name_map = match names {
                Some(path) => match load_name_map(path) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("Error loading names file: {e}");
                        return 1;
                    }
                },
                None => build_mix_name_map(&data),
            };
            list_mix(&data, &name_map)
        }
        #[cfg(feature = "meg")]
        Format::Meg => {
            if names.is_some() {
                eprintln!(
                    "Warning: --names is ignored for MEG/PGM archives; filenames are stored in the archive."
                );
            }
            list_meg(&data)
        }
        _ => {
            eprintln!("Error: unsupported archive format for `list`.");
            1
        }
    }
}

/// Returns `true` if the format is an archive type that `list` can handle.
fn is_archive_format(fmt: &Format) -> bool {
    #[cfg(feature = "meg")]
    if matches!(fmt, Format::Meg) {
        return true;
    }
    matches!(fmt, Format::Mix)
}

// ── MIX listing ──────────────────────────────────────────────────────────────

/// Parse a MIX archive and print a per-entry table to stdout.
///
/// With no name map: columns are CRC and Size.
/// With a name map: columns are CRC, Size, and Name.
fn list_mix(data: &[u8], name_map: &HashMap<MixCrc, String>) -> i32 {
    let archive = match cnc_formats::mix::MixArchive::parse(data) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    let entries = archive.entries();
    let has_names = !name_map.is_empty();

    // ── Header ───────────────────────────────────────────────────────────
    if has_names {
        println!("CRC              Size  Name");
        println!("──────────────  ──────────  ────────────────");
    } else {
        println!("CRC              Size");
        println!("──────────────  ──────────");
    }

    // ── Entries ──────────────────────────────────────────────────────────
    for entry in entries {
        if has_names {
            let name = name_map
                .get(&entry.crc)
                .map(|s| s.as_str())
                .unwrap_or("(unknown)");
            println!(
                "0x{:08X}     {:>10}  {}",
                entry.crc.to_raw(),
                entry.size,
                name
            );
        } else {
            println!("0x{:08X}     {:>10}", entry.crc.to_raw(), entry.size);
        }
    }

    // ── Summary ──────────────────────────────────────────────────────────
    let total_size: u64 = entries.iter().map(|e| u64::from(e.size)).sum();
    println!("\n{} entries, {} bytes total", entries.len(), total_size);

    0
}

// ── MEG listing ──────────────────────────────────────────────────────────────

/// Parse a MEG archive and print a per-entry table to stdout.
///
/// MEG archives store filenames directly, so no `--names` file is needed.
#[cfg(feature = "meg")]
fn list_meg(data: &[u8]) -> i32 {
    let archive = match cnc_formats::meg::MegArchive::parse(data) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    let entries = archive.entries();

    // ── Header ───────────────────────────────────────────────────────────
    println!("Name                                      Size");
    println!("────────────────────────────────────  ──────────");

    // ── Entries ──────────────────────────────────────────────────────────
    for entry in entries {
        println!("{:<36}  {:>10}", entry.name, entry.size);
    }

    // ── Summary ──────────────────────────────────────────────────────────
    let total_size: u64 = entries.iter().map(|e| e.size).sum();
    println!("\n{} entries, {} bytes total", entries.len(), total_size);

    0
}
