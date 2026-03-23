// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use clap::{Parser, Subcommand, ValueEnum};

/// CLI tool for working with classic Command & Conquer game assets.
///
/// Supports all classic C&C formats from Dune II through Generals.
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
  Auto-detected: .mix .big .shp .pal .aud .lut .vqa .vqp .wsa .fnt .dip .eng .ger .fre .ini\n\
                   .vxl .hva .csf .cps .w3d .voc .pak .icn .bin .mpr .bag .idx .wnd .str .apt\n\
                   .dds .tga .jpg .jpeg .miniyaml .meg .pgm .mid .adl .xmi .avi\n\
  Ambiguous:     .tmp — use --format tmp (TD), --format tmp-ra (RA), or --format tmp-ts (TS/RA2)\n\
                   .shp — TD/RA1 by default; use --format shp-d2 (Dune II) or --format shp-ts (TS/RA2)\n\
                   .map — use --format map-ra2 (RA2) or --format map-sage (Generals)\n\
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
  cncf convert  intro.vqa --to mkv\n\
  cncf convert  rules.miniyaml --to yaml\n\
\n\
FORMAT DETECTION:\n\
  Extension is auto-detected for most formats. Exceptions:\n\
    .tmp       → ambiguous: use --format tmp (Tiberian Dawn) or tmp-ra (Red Alert)\n\
    .yaml .yml → not detected: use --format miniyaml if applicable\n\
    (none)     → always requires --format"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
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
  VQA  → mkv         no               BGR24 + PCM in Matroska (see --mkv-codec)\n\
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
        #[cfg(any(feature = "convert", feature = "miniyaml"))]
        #[arg(long, short)]
        output: Option<String>,
        /// Video codec for MKV output (VQA → MKV only).
        ///
        /// `uncompressed` (default): V_UNCOMPRESSED — native Matroska
        /// uncompressed video per RFC 9559.  Modern players (ffplay, mpv).
        /// `vfw`: V_MS/VFW/FOURCC — legacy Video for Windows mapping.
        /// Maximum compatibility (VLC 3.x, Windows Media Player, etc.).
        #[cfg(feature = "convert")]
        #[arg(long, value_enum, default_value_t = CliMkvCodec::Uncompressed)]
        mkv_codec: CliMkvCodec,
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
        /// MIX loading policy.
        ///
        /// `stream` parses the MIX table once and reads entry bytes on demand.
        /// `eager` loads the full archive into memory before listing.
        /// Ignored for BIG/MEG/PGM archives.
        #[arg(long, value_enum, default_value_t = MixAccessMode::Stream)]
        mix_access: MixAccessMode,
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
        /// MIX loading policy.
        ///
        /// `stream` parses the MIX table once and reads entry payloads on demand.
        /// `eager` loads the full archive into memory before extraction starts.
        /// Ignored for BIG/MEG/PGM archives.
        #[arg(long, value_enum, default_value_t = MixAccessMode::Stream)]
        mix_access: MixAccessMode,
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
    Vxl,
    Hva,
    ShpTs,
    Csf,
    Cps,
    W3d,
    TmpTs,
    /// Creative Voice File audio (Dune II).
    Voc,
    /// Dune II PAK archive.
    Pak,
    /// Dune II SHP sprites (Format80/LCW).
    ShpD2,
    /// Dune II icon/tile graphics (.icn).
    Icn,
    /// Dune II scenario/mission.
    D2Map,
    /// TD/RA1 terrain grid (.bin).
    BinTd,
    /// TD/RA1 map package (.mpr).
    Mpr,
    /// RA2 audio archive (.bag + .idx).
    BagIdx,
    /// RA2 map file (.map, INI-based).
    MapRa2,
    /// Generals UI layout (.wnd).
    Wnd,
    /// Generals SAGE string table (.str).
    SageStr,
    /// Generals binary map (.map, SAGE).
    MapSage,
    /// Generals GUI animation (.apt).
    Apt,
    /// DirectDraw Surface texture (.dds).
    Dds,
    /// Truevision TGA image (.tga).
    Tga,
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

/// MIX archive loading policy for CLI archive operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum MixAccessMode {
    /// Parse the archive table once and read entry bytes on demand.
    Stream,
    /// Read the full archive into memory up front before operating on it.
    Eager,
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
    /// Export to Matroska video (from VQA).
    #[cfg(feature = "convert")]
    Mkv,
    /// Import to VQA video (from AVI).
    #[cfg(feature = "convert")]
    Vqa,
    /// Standard YAML (for MiniYAML sources).
    #[cfg(feature = "miniyaml")]
    Yaml,
}

/// MKV video codec selection for the CLI `--mkv-codec` flag.
#[cfg(feature = "convert")]
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum CliMkvCodec {
    /// V_UNCOMPRESSED — native Matroska uncompressed video (RFC 9559).
    Uncompressed,
    /// V_MS/VFW/FOURCC — legacy VFW mapping for broad player compatibility.
    Vfw,
}
