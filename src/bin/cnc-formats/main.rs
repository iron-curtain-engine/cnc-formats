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
//! cncf identify <file>                                  # Content-sniff likely format
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
//! `list` and `extract` operate on archive formats (`.mix`, `.big`, plus
//! `.meg`/`.pgm` when built with the `meg` feature).  MIX stores CRC hashes of
//! filenames, not the filenames themselves; use `--names <file>` to provide
//! candidate filenames for CRC resolution.
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
/// Supports MIX, BIG, SHP, PAL, AUD, LUT, TMP, VQA, VQP, WSA, FNT, DIP, ENG,
/// INI, and MiniYAML.
/// Auto-detects format from file extension; use --format to override.
#[derive(Parser)]
#[command(
    name = "cncf",
    version,
    about = "CLI tool for working with classic Command & Conquer game assets",
    long_about = "\
CLI tool for working with classic Command & Conquer game assets.\n\
\n\
SUPPORTED FORMATS:\n\
  Auto-detected: .mix .big .shp .pal .aud .lut .vqa .vqp .wsa .fnt .dip .eng .ger .fre .ini .miniyaml .meg .pgm .mid .adl .xmi .avi\n\
  Ambiguous:     .tmp — MUST use --format tmp (TD) or --format tmp-ra (RA)\n\
  Not detected:  .yaml .yml — use --format miniyaml if the file is MiniYAML\n\
\n\
EXIT CODES:\n\
  0 = success (valid file, operation completed)\n\
  1 = error (parse failure, missing file, invalid arguments)\n\
\n\
IMPORTANT:\n\
  .tmp files ALWAYS require --format (TD and RA are incompatible formats)\n\
  SHP, TMP, WSA, FNT visual exports require --palette <file.pal>\n\
  MIX stores CRC hashes of filenames, not filenames or content checksums\n\
  MIX names auto-resolve via built-in TD/RA1/RA2 candidate corpus\n\
  BIG/MEG/PGM archives store filenames directly (--names is ignored)",
    after_long_help = "\
QUICK REFERENCE:\n\
  cncf validate <file>                   Parse and report OK or error\n\
  cncf inspect  <file>                   Dump metadata and structure\n\
  cncf identify <file>                   Sniff likely format from contents\n\
  cncf list     <archive>                List archive entries\n\
  cncf extract  <archive>                Extract entries to directory\n\
  cncf convert  <file> --to <fmt>        Convert between formats\n\
  cncf check    <file>                   Deep integrity verification\n\
  cncf fingerprint <file>                SHA-256 hash\n\
\n\
EXAMPLES:\n\
  cncf validate CONQUER.MIX\n\
  cncf inspect  units.shp\n\
  cncf identify unknown.bin\n\
  cncf extract  CONQUER.MIX --output ./assets/ --filter .shp\n\
  cncf convert  units.shp --to png --palette temperat.pal\n\
  cncf convert  speech.aud --to wav\n\
  cncf convert  intro.vqa --to avi\n\
  cncf convert  rules.miniyaml --to yaml\n\
\n\
FORMAT DETECTION:\n\
  Extension is auto-detected for most formats. Exceptions:\n\
    .tmp       → ambiguous: use --format tmp (Tiberian Dawn) or tmp-ra (Red Alert)\n\
    .yaml .yml → not detected: use --format miniyaml if applicable\n\
    (none)     → always requires --format"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Sniff the most likely format from file contents.
    ///
    /// Uses content-based probes instead of extension-based detection.
    /// This is advisory and intentionally conservative: if the file does not
    /// match a known signature strongly enough, the command prints `unknown`.
    #[command(after_long_help = "\
EXAMPLES:\n\
  cncf identify unknown.bin\n\
  cncf identify 54C2D545.bin\n\
\n\
OUTPUT:\n\
  shp        a likely SHP file\n\
  mix        a likely MIX archive\n\
  unknown    no sufficiently strong signature matched")]
    Identify {
        /// Path to the input file.
        file: String,
    },
    /// Parse a file and report validity (exit 0=OK, 1=error).
    ///
    /// Reads the file, attempts to parse it as the detected (or specified)
    /// format, and prints OK or INVALID with a diagnostic message.
    #[command(after_long_help = "\
EXAMPLES:\n\
  cncf validate CONQUER.MIX\n\
  cncf validate desert.tmp --format tmp\n\
  cncf validate snow.tmp --format tmp-ra\n\
\n\
OUTPUT:\n\
  OK: <path>                    on success (exit 0)\n\
  INVALID: <path>: <reason>     on failure (exit 1)")]
    Validate {
        /// Path to the input file.
        file: String,
        /// Override auto-detected format (required for .tmp files).
        ///
        /// Auto-detection works for: .mix, .big, .shp, .pal, .aud, .lut, .vqa,
        /// .vqp, .wsa, .fnt, .dip, .eng/.ger/.fre, .ini, .miniyaml.  The .tmp extension is
        /// ambiguous (TD and
        /// RA use incompatible formats) — specify --format tmp or --format
        /// tmp-ra.
        #[arg(long, value_enum)]
        format: Option<Format>,
    },
    /// Dump metadata: entries, dimensions, frame counts, codec info.
    ///
    /// Parses the file and prints a human-readable summary of its structure.
    /// For archives: entry count and listing.  For sprites: frame count and
    /// dimensions.  For audio: sample rate, channels, codec.  For video:
    /// resolution, FPS, frame count.
    #[command(after_long_help = "\
EXAMPLES:\n\
  cncf inspect CONQUER.MIX   → archive entry count and listing\n\
  cncf inspect units.shp     → frame count, width, height\n\
  cncf inspect speech.aud    → sample rate, channels, compression\n\
  cncf inspect intro.vqa     → resolution, FPS, frame/chunk counts")]
    Inspect {
        /// Path to the input file.
        file: String,
        /// Override auto-detected format (required for .tmp files).
        #[arg(long, value_enum)]
        format: Option<Format>,
    },
    /// Convert between C&C and common formats (--palette required for indexed-color).
    ///
    /// Bidirectional: SHP↔PNG/GIF, WSA↔PNG/GIF, AUD↔WAV, VQA↔AVI, PAL↔PNG, TMP↔PNG.
    /// One-way: MiniYAML→YAML.
    #[cfg(any(feature = "convert", feature = "miniyaml"))]
    #[command(after_long_help = "\
CONVERSION MATRIX:\n\
  Source → Target     Palette needed?  Notes\n\
  ────────────────────────────────────────────────────\n\
  SHP  → png, gif    YES              multi-frame: writes numbered files\n\
  WSA  → png, gif    YES              multi-frame: writes numbered files\n\
  TMP  → png         YES              use --format tmp or tmp-ra\n\
  FNT  → png         YES              renders all glyphs\n\
  PAL  → png         no               256-color swatch image\n\
  AUD  → wav         no\n\
  VQA  → avi         no               includes audio track\n\
  ────────────────────────────────────────────────────\n\
  png  → shp         YES              multi-frame: reads numbered files\n\
  gif  → shp         YES              reads animation frames\n\
  png  → wsa         YES              multi-frame: reads numbered files\n\
  gif  → wsa         YES\n\
  png  → tmp         no\n\
  png  → pal         no               extracts colors from image\n\
  wav  → aud         no\n\
  avi  → vqa         no\n\
  ────────────────────────────────────────────────────\n\
  miniyaml → yaml    no               one-way; .miniyaml auto-detects\n\
\n\
EXPORT EXAMPLES:\n\
  cncf convert units.shp    --to png --palette temperat.pal\n\
  cncf convert units.shp    --to gif --palette temperat.pal\n\
  cncf convert speech.aud   --to wav\n\
  cncf convert intro.vqa    --to avi\n\
  cncf convert desert.tmp   --to png --palette temperat.pal --format tmp\n\
  cncf convert font.fnt     --to png --palette temperat.pal\n\
  cncf convert temperat.pal --to png\n\
\n\
IMPORT EXAMPLES:\n\
  cncf convert frame_00.png --to shp --palette temperat.pal\n\
  cncf convert anim.gif     --to shp --palette temperat.pal\n\
  cncf convert sound.wav    --to aud\n\
  cncf convert video.avi    --to vqa\n\
\n\
IMPORTANT:\n\
  --palette is REQUIRED for SHP, TMP, WSA, and FNT visual exports/imports.\n\
  Multi-frame PNG export writes numbered files to --output directory.\n\
  Multi-frame PNG import reads all numbered PNGs from the input directory.")]
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
    /// List archive entries with CRC, size, and resolved filenames.
    ///
    /// Prints a tabular inventory of every entry in the archive.
    /// MIX entries show CRC hash and size; filenames are resolved via
    /// --names file, embedded XCC database, or built-in TD/RA1/RA2
    /// unique-CRC resolver (checked in that order).
    /// The CRC is a hash of the filename text, not a checksum of file contents.
    /// MEG/PGM archives always show stored filenames.
    #[command(after_long_help = "\
EXAMPLES:\n\
  cncf list CONQUER.MIX\n\
  cncf list CONQUER.MIX --names td_filenames.txt\n\
\n\
MIX FILENAME RESOLUTION (checked in order):\n\
  MIX index stores CRC(filename), offset, and size; it does not store names\n\
  1. --names <file>         user-supplied filename list\n\
  2. Embedded XCC LMD       \"local mix database.dat\" inside the archive\n\
  3. Built-in resolver      TD/RA1/RA2 candidate corpus, unique CRCs only")]
    List {
        /// Path to the archive file.
        file: String,
        /// Override auto-detected format.
        #[arg(long, value_enum)]
        format: Option<Format>,
        /// Text file with known filenames (one per line) for MIX CRC resolution.
        ///
        /// Each non-empty, non-comment line is hashed with the MIX CRC
        /// algorithm and matched against archive entries by filename CRC.
        /// Lines starting with '#' are ignored.  Ignored for MEG/PGM archives.
        #[arg(long)]
        names: Option<String>,
    },
    /// Extract archive entries to files (auto-resolves MIX filenames).
    ///
    /// Writes each archive entry as a separate file.
    /// MIX: entries named by resolved filename or {CRC:08X}.bin if unknown.
    /// MIX CRCs are hashes of filenames, not content checksums.
    /// MEG/PGM: entries use their stored filenames directly.
    /// Default output directory: `ARCHIVE_extracted/`.
    #[command(after_long_help = "\
EXAMPLES:\n\
  cncf extract CONQUER.MIX\n\
  cncf extract CONQUER.MIX --output ./assets/\n\
  cncf extract CONQUER.MIX --names td_filenames.txt\n\
  cncf extract CONQUER.MIX --names td_filenames.txt --filter .shp\n\
\n\
OUTPUT NAMING:\n\
  MIX without --names:  {CRC:08X}.bin  (e.g. 4BF58B8E.bin)\n\
  MIX with --names:     resolved.ext   (e.g. RULES.INI)\n\
  MIX with built-in DB: auto-resolves built-in TD/RA1/RA2 filename candidates\n\
  MEG/PGM:              stored filename (e.g. DATA/ART/UNIT.TGA)")]
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
        /// Each line is hashed as a filename and matched against archive CRCs.
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
    /// Deep integrity check beyond parsing (overlapping entries, CRC order).
    ///
    /// Goes beyond `validate` (which only checks "does this parse?") to
    /// verify internal consistency.  For archives: detects overlapping
    /// entry ranges and unexpected CRC ordering.  For other formats:
    /// equivalent to validate (parse success = pass).
    /// Exit 0=OK, 1=errors found.  Warnings do not affect exit code.
    #[command(after_long_help = "\
EXAMPLES:\n\
  cncf check CONQUER.MIX\n\
  cncf check desert.tmp --format tmp\n\
\n\
OUTPUT:\n\
  OK: <path>              no errors (may have warnings)\n\
  OK: <path> (N warnings) passed with warnings\n\
  FAIL: <path> (...)      errors found (exit 1)\n\
\n\
CHECKS PERFORMED:\n\
  MIX:  overlapping entry byte ranges, CRC sort order\n\
  MEG:  overlapping entry byte ranges\n\
  Other: parse success only")]
    Check {
        /// Path to the input file.
        file: String,
        /// Override auto-detected format.
        #[arg(long, value_enum)]
        format: Option<Format>,
    },
    /// SHA-256 hash in sha256sum-compatible format: `HEX  FILENAME`.
    ///
    /// Computes SHA-256 of the raw file bytes and prints in the standard
    /// sha256sum format: `<64-char hex>  <filename>`.  Works on any file
    /// regardless of format.
    #[command(after_long_help = "\
EXAMPLES:\n\
  cncf fingerprint CONQUER.MIX\n\
  cncf fingerprint rules.ini\n\
\n\
OUTPUT FORMAT:\n\
  a1b2c3d4...  CONQUER.MIX")]
    Fingerprint {
        /// Path to the input file.
        file: String,
    },
}
#[derive(Clone, ValueEnum)]
pub(crate) enum Format {
    Mix,
    Big,
    Shp,
    Pal,
    Aud,
    Lut,
    Dip,
    Tmp,
    TmpRa,
    Vqa,
    Vqp,
    Wsa,
    Fnt,
    Eng,
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
        Command::Identify { file } => cmd_identify(&file),
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
        "big" => Some(Format::Big),
        "shp" => Some(Format::Shp),
        "pal" => Some(Format::Pal),
        "aud" => Some(Format::Aud),
        "lut" => Some(Format::Lut),
        "dip" => Some(Format::Dip),
        // ".tmp" is ambiguous: TD and RA use incompatible tile formats.
        // Require explicit --format tmp or --format tmp-ra.
        "vqa" => Some(Format::Vqa),
        "vqp" => Some(Format::Vqp),
        "wsa" => Some(Format::Wsa),
        "fnt" => Some(Format::Fnt),
        "eng" | "ger" | "fre" => Some(Format::Eng),
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
        ["mix", "big", "meg", "pgm"].join(", ")
    }
    #[cfg(not(feature = "meg"))]
    {
        "mix, big".to_string()
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
/// then falls back to the compiled-in TD/RA1/RA2 unique-CRC resolver.  Logs
/// the source to stderr.
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
    // Fall back to the built-in unique-CRC resolver.
    let builtin = cnc_formats::mix::builtin_name_map();
    let stats = cnc_formats::mix::builtin_name_stats();
    eprintln!(
        "Using built-in MIX name resolver ({} unique CRCs, {} ambiguous CRCs omitted)",
        stats.resolved_crc_count, stats.ambiguous_crc_count
    );
    builtin
}

/// Sniff a likely format from file contents.
fn cmd_identify(path: &str) -> i32 {
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
