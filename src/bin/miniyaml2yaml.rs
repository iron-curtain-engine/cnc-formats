// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! `miniyaml2yaml` — CLI converter from MiniYAML to standard YAML.
//!
//! Reads a MiniYAML file (or stdin) and writes the equivalent YAML to
//! stdout.  Useful for tooling interop and pre-processing OpenRA mod
//! files for standard YAML consumers.
//!
//! ## Usage
//!
//! ```text
//! miniyaml2yaml <input.yaml>     # file → stdout
//! cat input.yaml | miniyaml2yaml # stdin → stdout
//! ```

use std::io::{self, Read};
use std::process;

fn main() {
    let input = match std::env::args().nth(1) {
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Error reading {path}: {e}");
                process::exit(1);
            }
        },
        None => {
            let mut buf = String::new();
            if let Err(e) = io::stdin().read_to_string(&mut buf) {
                eprintln!("Error reading stdin: {e}");
                process::exit(1);
            }
            buf
        }
    };

    match cnc_formats::miniyaml::MiniYamlDoc::parse_str(&input) {
        Ok(doc) => {
            let yaml = cnc_formats::miniyaml::to_yaml(&doc);
            print!("{yaml}");
        }
        Err(e) => {
            eprintln!("Parse error: {e}");
            process::exit(1);
        }
    }
}
