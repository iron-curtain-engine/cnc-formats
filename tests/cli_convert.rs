// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! CLI integration tests for the `convert` subcommand.

#![cfg(feature = "cli")]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[cfg(feature = "convert")]
use cnc_formats::aud::{encode_adpcm, AUD_FLAG_16BIT, SCOMP_WESTWOOD};

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

/// `convert --format miniyaml --to yaml --output ...` exits 0 and writes YAML.
///
/// Why: proves the convert pipeline works end-to-end — parsing MiniYAML,
/// converting to YAML, and honoring the single-file output contract.
#[cfg(feature = "miniyaml")]
#[test]
fn convert_miniyaml_to_yaml() {
    let input = b"Key: Value\nParent:\n\tChild: 42\n";
    let path = temp_file("convert.miniyaml", input);
    let output_path = std::env::temp_dir()
        .join("cnc_formats_cli_tests")
        .join("convert.yaml");
    fs::remove_file(&output_path).ok();
    let output = Command::new(bin_path())
        .args([
            "convert",
            "--format",
            "miniyaml",
            "--to",
            "yaml",
            "--output",
            output_path.to_str().unwrap(),
            path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let written = fs::read_to_string(&output_path).unwrap();
    assert!(
        written.contains("Key:"),
        "output should contain YAML content: {written}",
    );
    fs::remove_file(&path).ok();
    fs::remove_file(&output_path).ok();
}

/// `convert` with `.miniyaml` extension auto-detects source format.
///
/// Why: proves auto-detection works for `.miniyaml` extension, so
/// `--format` can be omitted when the extension is unambiguous and the
/// default output path still works.
#[cfg(feature = "miniyaml")]
#[test]
fn convert_auto_detect_miniyaml_extension() {
    let input = b"Section:\n\tKey: Val\n";
    let path = temp_file("autodetect.miniyaml", input);
    let output_path = path.with_extension("yaml");
    fs::remove_file(&output_path).ok();
    let output = Command::new(bin_path())
        .args(["convert", "--to", "yaml", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected exit 0 with .miniyaml auto-detect, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let written = fs::read_to_string(&output_path).unwrap();
    assert!(
        written.contains("Section:"),
        "expected default output file to contain YAML: {written}",
    );
    fs::remove_file(&path).ok();
    fs::remove_file(&output_path).ok();
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

#[cfg(feature = "convert")]
#[test]
fn convert_aud_to_wav_streams_to_output_file() {
    let samples = [0i16, 200, -200, 400, -400, 0];
    let compressed = encode_adpcm(&samples, false);

    // SCOMP_WESTWOOD payload: each chunk starts with a 4-byte header
    // (u16 compressed_size, u16 uncompressed_size) before the ADPCM bytes.
    let chunk_compressed = compressed.len() as u16;
    let chunk_uncompressed = (samples.len() * 2) as u16;
    let mut payload = Vec::new();
    payload.extend_from_slice(&chunk_compressed.to_le_bytes());
    payload.extend_from_slice(&chunk_uncompressed.to_le_bytes());
    payload.extend_from_slice(&compressed);

    let mut aud = Vec::new();
    aud.extend_from_slice(&22050u16.to_le_bytes());
    // compressed_size in the file header includes the 4-byte chunk header.
    aud.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    aud.extend_from_slice(&((samples.len() * 2) as u32).to_le_bytes());
    aud.push(AUD_FLAG_16BIT);
    aud.push(SCOMP_WESTWOOD);
    aud.extend_from_slice(&payload);

    let input = temp_file("stream_convert.aud", &aud);
    let output = std::env::temp_dir()
        .join("cnc_formats_cli_tests")
        .join("stream_convert.wav");
    fs::remove_file(&output).ok();

    let result = Command::new(bin_path())
        .args([
            "convert",
            "--to",
            "wav",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        result.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&result.stderr),
    );

    let wav = fs::read(&output).unwrap();
    assert_eq!(&wav[..4], b"RIFF");

    fs::remove_file(&input).ok();
    fs::remove_file(&output).ok();
}
