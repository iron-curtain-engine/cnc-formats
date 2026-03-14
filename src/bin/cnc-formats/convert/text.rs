// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Text-format conversion helpers for the CLI `convert` subcommand.

use super::Format;

/// Convert a MiniYAML file to standard YAML and write to stdout.
pub(super) fn convert_miniyaml_to_yaml(path: &str, explicit_format: Option<Format>) -> i32 {
    // Auto-detect or require explicit format for ambiguous extensions.
    let fmt = explicit_format.map(|_| ());
    let is_miniyaml = fmt.is_some()
        || path
            .rsplit('.')
            .next()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("miniyaml"));
    if !is_miniyaml && path != "-" {
        eprintln!(
            "Cannot auto-detect MiniYAML from extension. \
             Use --format miniyaml to specify explicitly."
        );
        return 1;
    }

    let input = if path == "-" {
        use std::io::Read;
        let mut buf = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
            eprintln!("Error reading stdin: {e}");
            return 1;
        }
        buf
    } else {
        match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading {path}: {e}");
                return 1;
            }
        }
    };

    match cnc_formats::miniyaml::MiniYamlDoc::parse_str(&input) {
        Ok(doc) => {
            let yaml = cnc_formats::miniyaml::to_yaml(&doc);
            print!("{yaml}");
            0
        }
        Err(e) => {
            super::super::report_parse_error(path, &e);
            1
        }
    }
}
