// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! `list` subcommand — quick archive inventory showing CRC, size, and
//! optionally resolved filenames for each entry.
//!
//! Lighter than `inspect` for answering "what's in this `.mix`?".
//! Output goes to stdout so it can be piped to other tools.

use super::{
    build_mix_name_map_archive, build_mix_name_map_reader, load_name_map, open_file, read_file,
    resolve_format, supported_archive_list, Format, MixAccessMode,
};
use std::collections::HashMap;

use cnc_formats::mix::MixCrc;

// ── list ─────────────────────────────────────────────────────────────────────

/// Parse an archive and print a per-entry inventory.
pub(crate) fn cmd_list(
    path: &str,
    explicit: Option<Format>,
    mix_access: MixAccessMode,
    names: Option<&str>,
) -> i32 {
    let fmt = resolve_format(path, explicit);

    if !is_archive_format(&fmt) {
        eprintln!(
            "Error: `list` only supports archive formats ({}).\n  \
             To view non-archive files, use `cncf inspect`.",
            supported_archive_list()
        );
        return 1;
    }

    match fmt {
        Format::Mix => list_mix_with_policy(path, mix_access, names),
        Format::Big => {
            if mix_access != MixAccessMode::Stream {
                eprintln!("Warning: --mix-access is ignored for BIG archives.");
            }
            if names.is_some() {
                eprintln!(
                    "Warning: --names is ignored for BIG archives; filenames are stored in the archive."
                );
            }
            let file = open_file(path);
            list_big(file)
        }
        #[cfg(feature = "meg")]
        Format::Meg => {
            if mix_access != MixAccessMode::Stream {
                eprintln!("Warning: --mix-access is ignored for MEG/PGM archives.");
            }
            if names.is_some() {
                eprintln!(
                    "Warning: --names is ignored for MEG/PGM archives; filenames are stored in the archive."
                );
            }
            let file = open_file(path);
            list_meg(file)
        }
        _ => {
            eprintln!("Error: unsupported archive format for `list`.");
            1
        }
    }
}

fn list_mix_with_policy(path: &str, mix_access: MixAccessMode, names: Option<&str>) -> i32 {
    match mix_access {
        MixAccessMode::Stream => {
            let file = open_file(path);
            let mut archive = match cnc_formats::mix::MixArchiveReader::open(file) {
                Ok(archive) => archive,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return 1;
                }
            };
            let name_map = match names {
                Some(path) => match load_name_map(path) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("Error loading names file: {e}");
                        return 1;
                    }
                },
                None => match build_mix_name_map_reader(&mut archive) {
                    Ok(map) => map,
                    Err(e) => {
                        eprintln!("Error: {e}");
                        return 1;
                    }
                },
            };
            list_mix_entries(archive.entries(), &name_map)
        }
        MixAccessMode::Eager => {
            let data = read_file(path);
            let archive = match cnc_formats::mix::MixArchive::parse(&data) {
                Ok(archive) => archive,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return 1;
                }
            };
            let name_map = match names {
                Some(path) => match load_name_map(path) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("Error loading names file: {e}");
                        return 1;
                    }
                },
                None => build_mix_name_map_archive(&archive),
            };
            list_mix_entries(archive.entries(), &name_map)
        }
    }
}

/// Returns `true` if the format is an archive type that `list` can handle.
fn is_archive_format(fmt: &Format) -> bool {
    if matches!(fmt, Format::Big) {
        return true;
    }
    #[cfg(feature = "meg")]
    if matches!(fmt, Format::Meg) {
        return true;
    }
    matches!(fmt, Format::Mix)
}

// ── MIX listing ──────────────────────────────────────────────────────────────

/// Print a MIX entry table to stdout.
///
/// With no name map: columns are CRC and Size.
/// With a name map: columns are CRC, Size, and Name.
fn list_mix_entries(
    entries: &[cnc_formats::mix::MixEntry],
    name_map: &HashMap<MixCrc, String>,
) -> i32 {
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

// ── BIG listing ──────────────────────────────────────────────────────────────

/// Parse a BIG archive and print a per-entry table to stdout.
fn list_big<R: std::io::Read + std::io::Seek>(file: R) -> i32 {
    let archive = match cnc_formats::big::BigArchiveReader::open(file) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    let entries = archive.entries();

    println!("Name                                      Size");
    println!("────────────────────────────────────  ──────────");

    for entry in entries {
        println!("{:<36}  {:>10}", entry.name, entry.size);
    }

    let total_size: u64 = entries.iter().map(|e| e.size).sum();
    println!("\n{} entries, {} bytes total", entries.len(), total_size);

    0
}

// ── MEG listing ──────────────────────────────────────────────────────────────

/// Parse a MEG archive and print a per-entry table to stdout.
///
/// MEG archives store filenames directly, so no `--names` file is needed.
#[cfg(feature = "meg")]
fn list_meg<R: std::io::Read + std::io::Seek>(file: R) -> i32 {
    let archive = match cnc_formats::meg::MegArchiveReader::open(file) {
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
