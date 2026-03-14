// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! `cnc-formats` — CLI tool for validating, inspecting, extracting, and
//! converting C&C game format files.
//!
//! ## Subcommands
//!
//! ```text
//! cncf validate <file>                                  # Parse file, report OK or error
//! cncf inspect  <file>                                  # Dump metadata / contents
//! cncf list     <archive>                               # Quick archive inventory
//! cncf extract  <archive>                               # Extract archive to directory
//! cncf convert  <file> --format miniyaml --to yaml      # Convert (.yaml is ambiguous)
//! cncf convert  rules.miniyaml --to yaml                # .miniyaml auto-detects
//! ```
//!
//! `validate` and `inspect` auto-detect format from file extension.  Use
//! `--format <fmt>` to override when the extension is ambiguous (e.g.
//! `.yaml` could be standard YAML or MiniYAML).
//!
//! `list` and `extract` operate on archive formats (`.mix`, plus `.meg`/`.pgm`
//! when built with the `meg` feature).  Use `--names <file>` to provide known
//! filenames for MIX CRC resolution.
//!
//! `convert` uses `--format` and `--to` to specify the source and target
//! formats.  `--format` can be omitted when the file extension is unambiguous
//! (`.miniyaml`), but `.yaml`/`.yml` always require explicit `--format`.

mod check;
mod extract;
mod fingerprint;
mod inspect;
mod list;

#[cfg(any(feature = "convert", feature = "miniyaml"))]
mod convert;

use clap::{Parser, Subcommand, ValueEnum};
use std::collections::HashMap;
use std::process;

// ── CLI argument types ───────────────────────────────────────────────────────

/// CLI tool for working with classic Command & Conquer game assets.
///
/// Supports MIX, SHP, PAL, AUD, TMP, VQA, WSA, FNT, INI, and MiniYAML.
/// Auto-detects format from file extension; use --format to override.
#[derive(Parser)]
#[command(
    name = "cncf",
    version,
    about = "CLI tool for working with classic Command & Conquer game assets",
    long_about = "CLI tool for working with classic Command & Conquer game assets.\n\n\
        Supports MIX archives, SHP sprites, PAL palettes, AUD audio, TMP tiles,\n\
        VQA video, WSA animations, FNT bitmap fonts, INI rules, and MiniYAML.\n\n\
        Format is auto-detected from file extension. Use --format to override\n\
        when the extension is missing or ambiguous (e.g. .tmp files need\n\
        --format tmp or --format tmp-ra).",
    after_long_help = "\
EXAMPLES:\n  cncf validate CONQUER.MIX\n  cncf inspect  units.shp"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse a file and report whether it is structurally valid.
    ///
    /// Exits with code 0 if the file is valid, 1 on error.
    #[command(after_long_help = "\
EXAMPLES:\n  cncf validate CONQUER.MIX\n  cncf validate desert.tmp --format tmp\n  cncf validate snow.tmp --format tmp-ra")]
    Validate {
        /// Path to the input file.
        file: String,
        /// Override auto-detected format (required for .tmp files).
        ///
        /// Auto-detection works for: .mix, .shp, .pal, .aud, .vqa, .wsa,
        /// .fnt, .ini, .miniyaml.  The .tmp extension is ambiguous (TD and
        /// RA use incompatible formats) — specify --format tmp or --format
        /// tmp-ra.
        #[arg(long, value_enum)]
        format: Option<Format>,
    },
    /// Dump file metadata and contents summary.
    ///
    /// Shows header info, entry counts, dimensions, frame counts, etc.
    #[command(after_long_help = "\
EXAMPLES:\n  cncf inspect CONQUER.MIX        # List MIX archive entries\n  cncf inspect units.shp           # Show frame count, dimensions\n  cncf inspect speech.aud          # Show sample rate, codec info\n  cncf inspect intro.vqa           # Show video dimensions, FPS")]
    Inspect {
        /// Path to the input file.
        file: String,
        /// Override auto-detected format (required for .tmp files).
        #[arg(long, value_enum)]
        format: Option<Format>,
    },
    /// Convert between C&C formats and common formats (PNG, GIF, WAV, AVI).
    ///
    /// Conversions are bidirectional where possible: SHP↔PNG, SHP↔GIF,
    /// WSA↔PNG, WSA↔GIF, AUD↔WAV, VQA↔AVI, PAL↔PNG, TMP↔PNG.
    /// MiniYAML→YAML is one-way.
    #[cfg(any(feature = "convert", feature = "miniyaml"))]
    #[command(after_long_help = "\
EXPORT (C&C → common):\n  cncf convert units.shp  --to png --palette temperat.pal\n  cncf convert units.shp  --to gif --palette temperat.pal\n  cncf convert speech.aud --to wav\n  cncf convert intro.vqa  --to avi\n  cncf convert desert.tmp --to png --palette temperat.pal --format tmp\n  cncf convert font.fnt   --to png --palette temperat.pal\n  cncf convert temperat.pal --to png\n\n\
IMPORT (common → C&C):\n  cncf convert frame_00.png --to shp --palette temperat.pal\n  cncf convert anim.gif     --to shp --palette temperat.pal\n  cncf convert frame_00.png --to wsa --palette temperat.pal\n  cncf convert anim.gif     --to wsa --palette temperat.pal\n  cncf convert sound.wav    --to aud\n  cncf convert video.avi    --to vqa\n  cncf convert swatch.png   --to pal\n  cncf convert tile_00.png  --to tmp\n\n\
TEXT:\n  cncf convert rules.miniyaml --to yaml\n\n\
NOTES:\n  --palette is REQUIRED for SHP, TMP, WSA, and FNT exports.\n  Multi-frame PNG export writes numbered files to a directory.\n  Multi-frame PNG import reads all numbered PNGs from a directory.")]
    Convert {
        /// Path to the input file, or `-` for stdin (MiniYAML only).
        ///
        /// For multi-frame PNG import (--to shp, --to wsa), pass the first
        /// frame (e.g. frame_00.png) — all numbered frames in the same
        /// directory will be included.
        file: String,
        /// Target format to convert to.
        #[arg(long, value_enum)]
        to: ConvertTarget,
        /// Override auto-detected source format.
        #[arg(long, value_enum)]
        format: Option<Format>,
        /// Palette file (.pal) — REQUIRED for indexed-color formats.
        ///
        /// Needed when exporting SHP, TMP, WSA, or FNT to PNG/GIF.
        /// Also needed when importing PNG/GIF to SHP, WSA, or TMP
        /// (for color quantisation to the 256-color palette).
        #[cfg(feature = "convert")]
        #[arg(long)]
        palette: Option<String>,
        /// Output file or directory path.
        ///
        /// For single-output formats (WAV, GIF, AVI, AUD, VQA, PAL, YAML):
        /// output file path.  For multi-frame PNG export (SHP, WSA, TMP):
        /// output directory where numbered PNGs are written.
        /// Defaults to the input filename with the target extension.
        #[cfg(feature = "convert")]
        #[arg(long, short)]
        output: Option<String>,
    },
    /// List entries in an archive.
    ///
    /// Quick inventory showing CRC hash, size, and optionally the resolved
    /// filename for each entry.  Lighter than `inspect` for answering
    /// "what's in this archive?".  Enable the `meg` feature for MEG/PGM
    /// archive support.
    #[command(after_long_help = "\
EXAMPLES:\n  cncf list CONQUER.MIX\n  cncf list CONQUER.MIX --names td_filenames.txt")]
    List {
        /// Path to the archive file.
        file: String,
        /// Override auto-detected format.
        #[arg(long, value_enum)]
        format: Option<Format>,
        /// Text file with known filenames (one per line) for MIX CRC resolution.
        ///
        /// Each non-empty, non-comment line is hashed with the MIX CRC
        /// algorithm and matched against archive entries.  Lines starting
        /// with '#' are ignored.  Ignored for MEG/PGM archives.
        #[arg(long)]
        names: Option<String>,
    },
    /// Extract all files from an archive to a directory.
    ///
    /// Replaces XCC Mixer for the most common modding operation.
    /// Without `--names`, MIX entries are written as `{CRC:08X}.bin`.
    /// With `--names`, resolved entries use their real filename.
    /// Enable the `meg` feature for MEG/PGM archive support.
    #[command(after_long_help = "\
EXAMPLES:\n  cncf extract CONQUER.MIX\n  cncf extract CONQUER.MIX --output ./assets/\n  cncf extract CONQUER.MIX --names td_filenames.txt\n  cncf extract CONQUER.MIX --names td_filenames.txt --filter .shp")]
    Extract {
        /// Path to the archive file.
        file: String,
        /// Override auto-detected format.
        #[arg(long, value_enum)]
        format: Option<Format>,
        /// Output directory (default: `<filename>_extracted`).
        #[arg(long, short)]
        output: Option<String>,
        /// Text file with known filenames (one per line) for MIX CRC resolution.
        ///
        /// Ignored for MEG/PGM archives, which store filenames directly.
        #[arg(long)]
        names: Option<String>,
        /// Filter entries by name substring (case-insensitive).
        ///
        /// Only entries whose filename contains this substring are
        /// extracted.  Without `--names`, matches against the CRC hex
        /// string.
        #[arg(long)]
        filter: Option<String>,
    },
    /// Deep structural integrity verification.
    ///
    /// Goes beyond `validate` (which only checks "does this parse?") to
    /// verify internal consistency: overlapping archive entries, CRC
    /// ordering, and other format-specific invariants.
    #[command(after_long_help = "\
EXAMPLES:\n  cncf check CONQUER.MIX\n  cncf check desert.tmp --format tmp")]
    Check {
        /// Path to the input file.
        file: String,
        /// Override auto-detected format.
        #[arg(long, value_enum)]
        format: Option<Format>,
    },
    /// Compute SHA-256 fingerprint of a file.
    ///
    /// Prints the hash in sha256sum-compatible format:
    /// `<hex_hash>  <filename>`
    #[command(after_long_help = "\
EXAMPLES:\n  cncf fingerprint CONQUER.MIX\n  cncf fingerprint rules.ini")]
    Fingerprint {
        /// Path to the input file.
        file: String,
    },
}
#[derive(Clone, ValueEnum)]
pub(crate) enum Format {
    Mix,
    Shp,
    Pal,
    Aud,
    Tmp,
    TmpRa,
    Vqa,
    Wsa,
    Fnt,
    Ini,
    #[cfg(feature = "convert")]
    Avi,
    #[cfg(feature = "miniyaml")]
    Miniyaml,
    #[cfg(feature = "midi")]
    Mid,
    #[cfg(feature = "adl")]
    Adl,
    #[cfg(feature = "xmi")]
    Xmi,
    #[cfg(feature = "meg")]
    Meg,
}

/// Target format for the `convert` subcommand.
#[cfg(any(feature = "convert", feature = "miniyaml"))]
#[derive(Clone, ValueEnum)]
pub(crate) enum ConvertTarget {
    /// Export to PNG image (from SHP, PAL, TMP, WSA, FNT — needs --palette).
    #[cfg(feature = "convert")]
    Png,
    /// Export to WAV audio (from AUD).
    #[cfg(feature = "convert")]
    Wav,
    /// Export to animated GIF (from SHP, WSA — needs --palette).
    #[cfg(feature = "convert")]
    Gif,
    /// Import to SHP sprite (from PNG or GIF — needs --palette).
    #[cfg(feature = "convert")]
    Shp,
    /// Import to AUD audio (from WAV).
    #[cfg(feature = "convert")]
    Aud,
    /// Import to WSA animation (from PNG or GIF — needs --palette).
    #[cfg(feature = "convert")]
    Wsa,
    /// Import to PAL palette (from PNG — extracts colors).
    #[cfg(feature = "convert")]
    Pal,
    /// Import to TMP terrain tiles (from PNG, TD format).
    #[cfg(feature = "convert")]
    Tmp,
    /// Export to AVI video (from VQA).
    #[cfg(feature = "convert")]
    Avi,
    /// Import to VQA video (from AVI).
    #[cfg(feature = "convert")]
    Vqa,
    /// Standard YAML (for MiniYAML sources).
    #[cfg(feature = "miniyaml")]
    Yaml,
}

// ── Entry point ──────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::Validate { file, format } => cmd_validate(&file, format),
        Command::Inspect { file, format } => inspect::cmd_inspect(&file, format),
        #[cfg(any(feature = "convert", feature = "miniyaml"))]
        Command::Convert {
            file,
            to,
            format,
            #[cfg(feature = "convert")]
            palette,
            #[cfg(feature = "convert")]
            output,
        } => convert::cmd_convert(
            &file,
            to,
            format,
            #[cfg(feature = "convert")]
            palette.as_deref(),
            #[cfg(feature = "convert")]
            output.as_deref(),
        ),
        Command::List {
            file,
            format,
            names,
        } => list::cmd_list(&file, format, names.as_deref()),
        Command::Extract {
            file,
            format,
            output,
            names,
            filter,
        } => extract::cmd_extract(
            &file,
            format,
            output.as_deref(),
            names.as_deref(),
            filter.as_deref(),
        ),
        Command::Check { file, format } => check::cmd_check(&file, format),
        Command::Fingerprint { file } => fingerprint::cmd_fingerprint(&file),
    };
    process::exit(code);
}

// ── Shared helpers ───────────────────────────────────────────────────────────

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

/// Detect the format from the file extension.  Returns `None` for unknown
/// extensions.
fn detect_format(path: &str) -> Option<Format> {
    let ext = path.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "mix" => Some(Format::Mix),
        "shp" => Some(Format::Shp),
        "pal" => Some(Format::Pal),
        "aud" => Some(Format::Aud),
        // ".tmp" is ambiguous: TD and RA use incompatible tile formats.
        // Require explicit --format tmp or --format tmp-ra.
        "vqa" => Some(Format::Vqa),
        "wsa" => Some(Format::Wsa),
        "fnt" => Some(Format::Fnt),
        "ini" => Some(Format::Ini),
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
        #[cfg(feature = "meg")]
        "meg" | "pgm" => Some(Format::Meg),
        _ => None,
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
    // .tmp is a known ambiguous extension (TD vs RA).
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
        ["mix", "meg", "pgm"].join(", ")
    }
    #[cfg(not(feature = "meg"))]
    {
        "mix".to_string()
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
        // Skip empty lines and comments.
        if name.is_empty() || name.starts_with('#') {
            continue;
        }
        let crc = cnc_formats::mix::crc(name);
        map.insert(crc, name.to_string());
    }
    Ok(map)
}

/// Build a CRC→filename map for a MIX archive from embedded + built-in sources.
///
/// Checks for an XCC "local mix database.dat" entry inside the archive first,
/// then falls back to the compiled-in TD/RA1 filename database.  Logs the
/// source to stderr.
pub(crate) fn build_mix_name_map(data: &[u8]) -> HashMap<cnc_formats::mix::MixCrc, String> {
    // Try parsing the archive to check for embedded XCC LMD.
    if let Ok(archive) = cnc_formats::mix::MixArchive::parse(data) {
        let embedded = archive.embedded_names();
        if !embedded.is_empty() {
            eprintln!(
                "Using embedded XCC filename database ({} names)",
                embedded.len()
            );
            return embedded;
        }
    }
    // Fall back to built-in database.
    let builtin = cnc_formats::mix::builtin_name_map();
    eprintln!(
        "Using built-in filename database ({} known names)",
        builtin.len()
    );
    builtin
}

// ── validate ─────────────────────────────────────────────────────────────────

/// Parse the file and report success or failure.  Exit code 0 = valid,
/// 1 = parse error.
fn cmd_validate(path: &str, explicit: Option<Format>) -> i32 {
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

/// Attempt to parse `data` as the given format.  Returns `Ok(())` if
/// parsing succeeds.
fn validate_data(data: &[u8], fmt: &Format) -> Result<(), cnc_formats::Error> {
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
