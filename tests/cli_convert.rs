// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! CLI integration tests for the `convert` subcommand.

#![cfg(feature = "cli")]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Returns the path to the `cnc-formats` binary built by cargo.
fn bin_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cncf"))
}

/// Creates a temporary file with the given name and content under a
/// test-specific directory.
fn temp_file(name: &str, content: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join("cnc_formats_cli_tests");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path
}

// ── convert subcommand ───────────────────────────────────────────────────────

/// `convert --format miniyaml --to yaml` on valid MiniYAML exits 0 and
/// produces YAML output.
///
/// Why: proves the convert pipeline works end-to-end — parsing MiniYAML,
/// converting to YAML, and writing to stdout.
#[cfg(feature = "miniyaml")]
#[test]
fn convert_miniyaml_to_yaml() {
    let input = b"Key: Value\nParent:\n\tChild: 42\n";
    let path = temp_file("convert.miniyaml", input);
    let output = Command::new(bin_path())
        .args([
            "convert",
            "--format",
            "miniyaml",
            "--to",
            "yaml",
            path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        stdout.contains("Key:"),
        "stdout should contain YAML output: {stdout}",
    );
    fs::remove_file(&path).ok();
}

/// `convert` with `.miniyaml` extension auto-detects source format.
///
/// Why: proves auto-detection works for `.miniyaml` extension, so
/// `--format` can be omitted when the extension is unambiguous.
#[cfg(feature = "miniyaml")]
#[test]
fn convert_auto_detect_miniyaml_extension() {
    let input = b"Section:\n\tKey: Val\n";
    let path = temp_file("autodetect.miniyaml", input);
    let output = Command::new(bin_path())
        .args(["convert", "--to", "yaml", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected exit 0 with .miniyaml auto-detect, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    fs::remove_file(&path).ok();
}

/// `convert` with an invalid palette path exits nonzero and names the file.
///
/// Why: palette-driven conversions fail before parsing the source asset, so
/// the CLI should point directly at the missing or malformed palette input.
#[cfg(feature = "convert")]
#[test]
fn convert_invalid_palette_path_reports_palette_error() {
    let png_path = temp_file(
        "palette_error.png",
        &[
            0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n', 0, 0, 0, 0,
        ],
    );
    let missing_palette = std::env::temp_dir()
        .join("cnc_formats_cli_tests")
        .join("missing_palette.pal");
    fs::remove_file(&missing_palette).ok();

    let output = Command::new(bin_path())
        .args([
            "convert",
            "--to",
            "shp",
            png_path.to_str().unwrap(),
            "--palette",
            missing_palette.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "expected nonzero exit for bad palette"
    );
    assert!(
        stderr.contains("palette"),
        "stderr should mention the palette input: {stderr}",
    );
    assert!(
        stderr.contains("missing_palette.pal"),
        "stderr should name the bad palette path: {stderr}",
    );

    fs::remove_file(&png_path).ok();
}
