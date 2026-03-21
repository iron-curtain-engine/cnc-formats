// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! `extract` subcommand — decompose MIX archives into individual files.
//!
//! Replaces XCC Mixer for the most common modding operation: pulling
//! individual assets out of a `.mix` archive.
//!
//! ## Output naming
//!
//! MIX archives store CRC hashes, not filenames.  Without a `--names`
//! file, extracted entries are named `{CRC:08X}.bin`.  With `--names`,
//! resolved entries use their real filename.
//!
//! ## Progress output
//!
//! All progress and summary output goes to stderr so stdout stays clean
//! for piping.

mod mix;
mod paths;
mod stored;
#[cfg(test)]
mod tests;

use super::{
    build_mix_name_map_archive, build_mix_name_map_reader, load_name_map, open_file, read_file,
    resolve_format, supported_archive_list, Format, MixAccessMode,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use self::mix::extract_mix;
use self::paths::{
    create_generated_output_file, create_strict_output_file, fallback_filename,
    generated_flat_output_path, make_output_name_unique, make_stored_name_unique,
    resolve_output_name, validate_candidate_path,
};
use self::stored::extract_big;
#[cfg(feature = "meg")]
use self::stored::extract_meg;
use cnc_formats::mix::MixCrc;
use strict_path::{PathBoundary, StrictPath};

// ── extract ──────────────────────────────────────────────────────────────────

/// Parse an archive and write each entry to an individual file.
pub(crate) fn cmd_extract(
    path: &str,
    explicit: Option<Format>,
    mix_access: MixAccessMode,
    output: Option<&str>,
    names: Option<&str>,
    filter: Option<&str>,
) -> i32 {
    let fmt = resolve_format(path, explicit);

    if !is_archive_format(&fmt) {
        eprintln!(
            "Error: `extract` only supports archive formats ({}).\n  \
             To convert non-archive files, use `cncf convert`.",
            supported_archive_list()
        );
        return 1;
    }

    // Determine output directory.
    let out_dir = match output {
        Some(d) => PathBuf::from(d),
        None => {
            // Default: input filename stem + "_extracted".
            let stem = Path::new(path)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            PathBuf::from(format!("{stem}_extracted"))
        }
    };

    match fmt {
        Format::Mix => extract_mix_with_policy(path, mix_access, &out_dir, names, filter),
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
            extract_big(file, &out_dir, filter)
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
            extract_meg(file, &out_dir, filter)
        }
        _ => {
            eprintln!("Error: unsupported archive format for `extract`.");
            1
        }
    }
}

fn extract_mix_with_policy(
    path: &str,
    mix_access: MixAccessMode,
    out_dir: &Path,
    names: Option<&str>,
    filter: Option<&str>,
) -> i32 {
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
                    Ok(m) => {
                        eprintln!("Loaded {} names from {path}", m.len());
                        m
                    }
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
            extract_mix(&mut archive, out_dir, &name_map, filter)
        }
        MixAccessMode::Eager => {
            let data = read_file(path);
            let mut archive = match cnc_formats::mix::MixArchive::parse(&data) {
                Ok(archive) => archive,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return 1;
                }
            };
            let name_map = match names {
                Some(path) => match load_name_map(path) {
                    Ok(m) => {
                        eprintln!("Loaded {} names from {path}", m.len());
                        m
                    }
                    Err(e) => {
                        eprintln!("Error loading names file: {e}");
                        return 1;
                    }
                },
                None => build_mix_name_map_archive(&archive),
            };
            extract_mix(&mut archive, out_dir, &name_map, filter)
        }
    }
}

/// Returns `true` if the format is an archive type that `extract` can handle.
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
