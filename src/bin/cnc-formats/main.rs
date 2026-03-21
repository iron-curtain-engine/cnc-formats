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

#![forbid(unsafe_code)]

mod args;
mod check;
mod extract;
mod fingerprint;
mod inspect;
mod list;
mod shared;

#[cfg(any(feature = "convert", feature = "miniyaml"))]
mod convert;

use clap::Parser;
use std::process;

#[cfg(any(feature = "convert", feature = "miniyaml"))]
pub(crate) use args::ConvertTarget;
pub(crate) use args::{Cli, Command, Format, MixAccessMode};
#[cfg(feature = "convert")]
pub(crate) use shared::report_convert_error;
#[cfg(any(feature = "convert", feature = "miniyaml"))]
pub(crate) use shared::report_parse_error;
pub(crate) use shared::{
    build_mix_name_map_archive, build_mix_name_map_reader, cmd_identify, cmd_validate,
    load_name_map, open_file, print_format_hint, read_file, resolve_format, supported_archive_list,
};

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
            output,
        } => convert::cmd_convert(
            &file,
            to,
            format,
            #[cfg(feature = "convert")]
            palette.as_deref(),
            output.as_deref(),
        ),
        Command::List {
            file,
            format,
            mix_access,
            names,
        } => list::cmd_list(&file, format, mix_access, names.as_deref()),
        Command::Extract {
            file,
            format,
            mix_access,
            output,
            names,
            filter,
        } => extract::cmd_extract(
            &file,
            format,
            mix_access,
            output.as_deref(),
            names.as_deref(),
            filter.as_deref(),
        ),
        Command::Check { file, format } => check::cmd_check(&file, format),
        Command::Fingerprint { file } => fingerprint::cmd_fingerprint(&file),
    };
    process::exit(code);
}
