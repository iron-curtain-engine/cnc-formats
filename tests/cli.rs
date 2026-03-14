// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! CLI integration tests — verifies the `cnc-formats` binary's validate,
//! inspect, and convert subcommands produce correct exit codes and output.
//!
//! These tests spawn the binary via `std::process::Command` and check
//! exit codes and stdout/stderr.  Each test creates a temporary file with
//! known content, invokes the binary, and verifies the result.

#![cfg(feature = "cli")]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Returns the path to the `cnc-formats` binary built by cargo.
fn bin_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cncf"))
}

/// Creates a temporary file with the given name and content under a
/// test-specific directory.  The caller should remove the file after use.
fn temp_file(name: &str, content: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join("cnc_formats_cli_tests");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path
}

/// Builds a valid 768-byte PAL file (256 colors, all black).
fn valid_pal_bytes() -> Vec<u8> {
    vec![0u8; 768]
}

/// Builds a valid INI file with one section and one key.
fn valid_ini_bytes() -> Vec<u8> {
    b"[General]\nName=Test\n".to_vec()
}

// ── validate subcommand ──────────────────────────────────────────────────────

/// `validate` on a valid PAL file exits 0 and prints "OK".
///
/// Why: proves the happy-path validate pipeline (auto-detect from extension,
/// parse, report success) works end-to-end through the binary.
#[test]
fn validate_valid_pal_exits_zero() {
    let path = temp_file("valid.pal", &valid_pal_bytes());
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(stdout.contains("OK"), "stdout should contain OK: {stdout}");
    fs::remove_file(&path).ok();
}

/// `validate` on an invalid (truncated) PAL file exits 1 and prints "INVALID".
///
/// Why: proves the error-path pipeline correctly reports parse failures
/// with a nonzero exit code.
#[test]
fn validate_invalid_pal_exits_one() {
    let path = temp_file("invalid.pal", &[0u8; 100]);
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "expected nonzero exit");
    assert!(
        stderr.contains("INVALID"),
        "stderr should contain INVALID: {stderr}",
    );
    fs::remove_file(&path).ok();
}

/// `validate --format pal` overrides auto-detection from extension.
///
/// Why: proves the `--format` flag overrides extension-based format
/// detection, allowing users to validate files with non-standard extensions.
#[test]
fn validate_format_override() {
    let path = temp_file("override.dat", &valid_pal_bytes());
    let output = Command::new(bin_path())
        .args(["validate", "--format", "pal", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "expected exit 0 with --format override, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(stdout.contains("OK"), "stdout should contain OK: {stdout}");
    fs::remove_file(&path).ok();
}

/// `validate` on a valid INI file exits 0.
///
/// Why: verifies validate works for text-based formats, not just binary.
#[test]
fn validate_valid_ini() {
    let path = temp_file("valid.ini", &valid_ini_bytes());
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    fs::remove_file(&path).ok();
}

/// `validate` on a nonexistent file exits nonzero.
///
/// Why: proves file-read errors are reported gracefully (not a panic).
#[test]
fn validate_nonexistent_file() {
    let path = std::env::temp_dir().join("cnc_formats_nonexistent.pal");
    // Ensure the file does not exist.
    fs::remove_file(&path).ok();
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "expected nonzero exit for missing file",
    );
}

/// `validate` with an unknown extension exits nonzero and mentions
/// format detection.
///
/// Why: proves the auto-detection failure path reports a helpful message.
#[test]
fn validate_unknown_extension() {
    let path = temp_file("unknown.xyz", &[0u8; 100]);
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected nonzero exit for unknown extension",
    );
    assert!(
        stderr.contains("Cannot detect format"),
        "stderr should mention format detection: {stderr}",
    );
    fs::remove_file(&path).ok();
}

// ── inspect subcommand ───────────────────────────────────────────────────────

/// `inspect` on a valid PAL file exits 0 and prints "PAL palette".
///
/// Why: proves the inspect pipeline produces format-specific metadata
/// output for binary formats.
#[test]
fn inspect_valid_pal() {
    let path = temp_file("inspect.pal", &valid_pal_bytes());
    let output = Command::new(bin_path())
        .args(["inspect", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        stdout.contains("PAL palette"),
        "stdout should contain 'PAL palette': {stdout}",
    );
    fs::remove_file(&path).ok();
}

/// `inspect` on a valid INI file exits 0 and includes section info.
///
/// Why: verifies inspect works for text-based formats with structured output.
#[test]
fn inspect_valid_ini() {
    let path = temp_file("inspect.ini", &valid_ini_bytes());
    let output = Command::new(bin_path())
        .args(["inspect", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        stdout.contains("INI"),
        "stdout should contain INI: {stdout}",
    );
    assert!(
        stdout.contains("General"),
        "stdout should mention the section name: {stdout}",
    );
    fs::remove_file(&path).ok();
}

/// `inspect` on an invalid file exits nonzero with a stderr error.
///
/// Why: proves inspect's error path reports parse failures cleanly.
#[test]
fn inspect_invalid_file_exits_nonzero() {
    let path = temp_file("inspect_bad.pal", &[0u8; 100]);
    let output = Command::new(bin_path())
        .args(["inspect", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "expected nonzero exit for invalid file",
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "stderr should contain error message");
    fs::remove_file(&path).ok();
}

// ── .tmp ambiguity ───────────────────────────────────────────────────────────

/// `.tmp` extension is ambiguous (TD vs RA) and must not auto-detect.
///
/// Why: TD and RA `.tmp` files use incompatible formats.  Auto-detecting
/// to one parser silently corrupts the other.  The CLI must reject `.tmp`
/// and ask for explicit `--format tmp` or `--format tmp-ra`.
#[test]
fn validate_tmp_extension_is_ambiguous() {
    let path = temp_file("terrain.tmp", &[0u8; 256]);
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected nonzero exit for ambiguous .tmp",
    );
    assert!(
        stderr.contains("Cannot detect format"),
        "stderr should mention format detection: {stderr}",
    );
    fs::remove_file(&path).ok();
}

/// `.tmp` with explicit `--format tmp` routes to the TD parser.
///
/// Why: proves that the `--format` override resolves the `.tmp` ambiguity
/// and the file reaches the TD parser instead of the "cannot detect" path.
#[test]
fn validate_tmp_with_explicit_format_td() {
    let path = temp_file("explicit_td.tmp", &[0u8; 256]);
    let output = Command::new(bin_path())
        .args(["validate", "--format", "tmp", path.to_str().unwrap()])
        .output()
        .unwrap();
    // With --format override, the file reaches the parser (not the
    // "Cannot detect format" path).  The all-zero file may parse as a
    // degenerate valid TD TMP, so we check it did not hit the detection error.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Cannot detect format"),
        "should not hit format detection error with --format override: {stderr}",
    );
    fs::remove_file(&path).ok();
}

/// `.tmp` with explicit `--format tmp-ra` routes to the RA parser.
///
/// Why: proves `--format tmp-ra` reaches the RA parser, which rejects the
/// all-zero file (RA TMP requires non-zero tile dimensions).
#[test]
fn validate_tmp_with_explicit_format_ra() {
    let path = temp_file("explicit_ra.tmp", &[0u8; 256]);
    let output = Command::new(bin_path())
        .args(["validate", "--format", "tmp-ra", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The RA parser rejects zero tile dimensions → INVALID, not detection error.
    assert!(
        stderr.contains("INVALID"),
        "should reach the RA parser with --format override: {stderr}",
    );
    assert!(
        !stderr.contains("Cannot detect format"),
        "should not hit format detection error: {stderr}",
    );
    fs::remove_file(&path).ok();
}

// ── list subcommand ──────────────────────────────────────────────────────────

/// Builds a basic MIX archive from (filename, data) pairs.
///
/// Replicates the binary layout expected by `MixArchive::parse`: 2-byte
/// count + 4-byte data_size + sorted entry table + concatenated data.
fn build_mix(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut entries: Vec<(u32, &[u8])> = files
        .iter()
        .map(|(name, data)| (cnc_formats::mix::crc(name).to_raw(), *data))
        .collect();
    // Sort by unsigned u32 CRC, matching MixArchive's internal order.
    entries.sort_by_key(|(c, _)| *c);

    let count = entries.len() as u16;
    let mut offsets = Vec::with_capacity(entries.len());
    let mut cur = 0u32;
    for (_, data) in &entries {
        offsets.push(cur);
        cur += data.len() as u32;
    }
    let data_size = cur;

    let mut out = Vec::new();
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&data_size.to_le_bytes());
    for (i, (c, data)) in entries.iter().enumerate() {
        out.extend_from_slice(&c.to_le_bytes());
        out.extend_from_slice(&offsets[i].to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    }
    for (_, data) in &entries {
        out.extend_from_slice(data);
    }
    out
}

/// Builds a basic MEG archive from (filename, data) pairs.
#[cfg(feature = "meg")]
fn build_meg(files: &[(&str, &[u8])]) -> Vec<u8> {
    let count = files.len() as u32;
    let mut out = Vec::new();

    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());

    for (name, _) in files {
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
    }

    let records_total = files.len() * 18;
    let data_start = out.len() + records_total;

    let mut offset = data_start as u32;
    let mut offsets = Vec::with_capacity(files.len());
    for (_, data) in files {
        offsets.push(offset);
        offset = offset.saturating_add(data.len() as u32);
    }

    for (i, (name, data)) in files.iter().enumerate() {
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(i as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offsets[i].to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    }

    for (_, data) in files {
        out.extend_from_slice(data);
    }

    out
}

/// `list` on a valid MIX archive exits 0 and prints entry count.
///
/// Why: proves the `list` subcommand parses a MIX archive and outputs
/// a per-entry inventory with CRC and size columns.
#[test]
fn list_mix_exits_zero() {
    let mix = build_mix(&[("UNIT.SHP", &[0xAA; 100]), ("SPEECH.AUD", &[0xBB; 50])]);
    let path = temp_file("test_list.mix", &mix);
    let output = Command::new(bin_path())
        .args(["list", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    // Should show 2 entries in the summary line.
    assert!(
        stdout.contains("2 entries"),
        "stdout should mention 2 entries: {stdout}",
    );
    fs::remove_file(&path).ok();
}

/// `list` with `--names` resolves CRC hashes to filenames.
///
/// Why: proves the name resolution pipeline (load text file, hash each
/// line, match against archive CRCs) produces human-readable output.
#[test]
fn list_mix_with_names() {
    let mix = build_mix(&[("UNIT.SHP", &[0xAA; 100])]);
    let mix_path = temp_file("test_list_names.mix", &mix);
    let names_path = temp_file("test_names.txt", b"UNIT.SHP\n");
    let output = Command::new(bin_path())
        .args([
            "list",
            mix_path.to_str().unwrap(),
            "--names",
            names_path.to_str().unwrap(),
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
        stdout.contains("UNIT.SHP"),
        "should resolve UNIT.SHP: {stdout}",
    );
    fs::remove_file(&mix_path).ok();
    fs::remove_file(&names_path).ok();
}

/// `list` on a non-archive format (PAL) exits 1 with an error.
///
/// Why: `list` only supports archive formats.  Non-archive files should
/// produce a clear error directing users to `inspect` instead.
#[test]
fn list_non_archive_exits_one() {
    let path = temp_file("test_list.pal", &valid_pal_bytes());
    let output = Command::new(bin_path())
        .args(["list", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "expected nonzero exit");
    assert!(
        stderr.contains("archive"),
        "stderr should mention archive: {stderr}",
    );
    fs::remove_file(&path).ok();
}

// ── extract subcommand ───────────────────────────────────────────────────────

/// `extract` on a valid MIX archive exits 0 and creates files.
///
/// Why: proves the `extract` subcommand parses a MIX, creates an output
/// directory, and writes individual entry files.
#[test]
fn extract_mix_exits_zero() {
    let mix = build_mix(&[("UNIT.SHP", &[0xAA; 100])]);
    let mix_path = temp_file("test_extract.mix", &mix);
    let out_dir = std::env::temp_dir()
        .join("cnc_formats_cli_tests")
        .join("test_extract_out");
    // Clean up any previous run.
    let _ = fs::remove_dir_all(&out_dir);

    let output = Command::new(bin_path())
        .args([
            "extract",
            mix_path.to_str().unwrap(),
            "--output",
            out_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    // Output directory should contain exactly one file (CRC-named .bin).
    let files: Vec<_> = fs::read_dir(&out_dir).unwrap().collect();
    assert_eq!(files.len(), 1, "expected 1 extracted file");
    // Clean up.
    fs::remove_dir_all(&out_dir).ok();
    fs::remove_file(&mix_path).ok();
}

/// `extract` with `--names` writes files with resolved filenames.
///
/// Why: proves extracted files are named by their resolved name, not
/// just by CRC hex, when a names file is provided.
#[test]
fn extract_mix_with_names() {
    let mix = build_mix(&[("UNIT.SHP", &[0xAA; 50])]);
    let mix_path = temp_file("test_extract_names.mix", &mix);
    let names_path = temp_file("test_extract_names.txt", b"UNIT.SHP\n");
    let out_dir = std::env::temp_dir()
        .join("cnc_formats_cli_tests")
        .join("test_extract_names_out");
    let _ = fs::remove_dir_all(&out_dir);

    let output = Command::new(bin_path())
        .args([
            "extract",
            mix_path.to_str().unwrap(),
            "--output",
            out_dir.to_str().unwrap(),
            "--names",
            names_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    // Should have a file named UNIT.SHP (resolved), not a CRC hex name.
    let unit_shp = out_dir.join("UNIT.SHP");
    assert!(
        unit_shp.exists(),
        "expected UNIT.SHP in output, dir contents: {:?}",
        fs::read_dir(&out_dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .collect::<Vec<_>>(),
    );
    // Verify file contents match.
    let content = fs::read(&unit_shp).unwrap();
    assert_eq!(content, vec![0xAA; 50]);
    // Clean up.
    fs::remove_dir_all(&out_dir).ok();
    fs::remove_file(&mix_path).ok();
    fs::remove_file(&names_path).ok();
}

/// `extract` with `--filter` only extracts matching entries.
///
/// Why: proves the `--filter` flag correctly limits extraction to entries
/// whose resolved filename contains the filter substring.
#[test]
fn extract_mix_with_filter() {
    let mix = build_mix(&[("UNIT.SHP", &[0xAA; 50]), ("SPEECH.AUD", &[0xBB; 30])]);
    let mix_path = temp_file("test_extract_filter.mix", &mix);
    let names_path = temp_file("test_extract_filter_names.txt", b"UNIT.SHP\nSPEECH.AUD\n");
    let out_dir = std::env::temp_dir()
        .join("cnc_formats_cli_tests")
        .join("test_extract_filter_out");
    let _ = fs::remove_dir_all(&out_dir);

    let output = Command::new(bin_path())
        .args([
            "extract",
            mix_path.to_str().unwrap(),
            "--output",
            out_dir.to_str().unwrap(),
            "--names",
            names_path.to_str().unwrap(),
            "--filter",
            ".shp",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    // Only UNIT.SHP should be extracted, not SPEECH.AUD.
    let files: Vec<_> = fs::read_dir(&out_dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.file_name()))
        .collect();
    assert_eq!(files.len(), 1, "expected 1 filtered file, got: {files:?}");
    assert!(
        out_dir.join("UNIT.SHP").exists(),
        "expected UNIT.SHP in output",
    );
    assert!(
        !out_dir.join("SPEECH.AUD").exists(),
        "SPEECH.AUD should be filtered out",
    );
    // Clean up.
    fs::remove_dir_all(&out_dir).ok();
    fs::remove_file(&mix_path).ok();
    fs::remove_file(&names_path).ok();
}

// ── MEG archive CLI coverage ────────────────────────────────────────────────

/// `list` auto-detects `.pgm` as a MEG archive when the feature is enabled.
#[cfg(feature = "meg")]
#[test]
fn list_pgm_exits_zero() {
    let meg = build_meg(&[("DATA/UNIT.SHP", &[0xAA; 16])]);
    let path = temp_file("test_list.pgm", &meg);
    let output = Command::new(bin_path())
        .args(["list", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        stdout.contains("DATA/UNIT.SHP"),
        "stdout should contain MEG filename: {stdout}",
    );
    fs::remove_file(&path).ok();
}

/// `list` ignores `--names` for MEG/PGM archives.
#[cfg(feature = "meg")]
#[test]
fn list_meg_ignores_names_flag() {
    let meg = build_meg(&[("DATA/UNIT.SHP", &[0xAA; 16])]);
    let path = temp_file("test_list.meg", &meg);
    let missing_names = std::env::temp_dir().join("cnc_formats_missing_names.txt");
    fs::remove_file(&missing_names).ok();

    let output = Command::new(bin_path())
        .args([
            "list",
            path.to_str().unwrap(),
            "--names",
            missing_names.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "expected exit 0, stderr: {stderr}",);
    assert!(
        stderr.contains("ignored"),
        "stderr should mention ignored names flag: {stderr}",
    );
    fs::remove_file(&path).ok();
}

/// `extract` on a MEG archive preserves stored filenames and subdirectories.
#[cfg(feature = "meg")]
#[test]
fn extract_meg_exits_zero() {
    let meg = build_meg(&[("DATA/UNIT.SHP", &[0xAA; 12])]);
    let meg_path = temp_file("test_extract.meg", &meg);
    let out_dir = std::env::temp_dir()
        .join("cnc_formats_cli_tests")
        .join("test_extract_meg_out");
    let _ = fs::remove_dir_all(&out_dir);

    let output = Command::new(bin_path())
        .args([
            "extract",
            meg_path.to_str().unwrap(),
            "--output",
            out_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let unit_shp = out_dir.join("DATA").join("UNIT.SHP");
    assert!(
        unit_shp.exists(),
        "expected nested MEG file to be extracted"
    );
    assert_eq!(fs::read(&unit_shp).unwrap(), vec![0xAA; 12]);
    fs::remove_dir_all(&out_dir).ok();
    fs::remove_file(&meg_path).ok();
}

// ── check subcommand ────────────────────────────────────────────────────────

/// `check` on a valid archive exits 0 and prints "OK".
#[test]
fn check_valid_mix_exits_zero() {
    let mix = build_mix(&[("UNIT.SHP", &[0xAA; 32])]);
    let path = temp_file("test_check.mix", &mix);
    let output = Command::new(bin_path())
        .args(["check", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(stdout.contains("OK"), "stdout should contain OK: {stdout}");
    fs::remove_file(&path).ok();
}

// ── fingerprint subcommand ─────────────────────────────────────────────────

/// `fingerprint` prints a sha256sum-compatible digest of raw file bytes.
#[test]
fn fingerprint_known_sha256() {
    let path = temp_file("fingerprint.bin", b"abc");
    let output = Command::new(bin_path())
        .args(["fingerprint", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        stdout.contains("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        "stdout should contain the SHA-256 for 'abc': {stdout}",
    );
    fs::remove_file(&path).ok();
}

// ── error path coverage ─────────────────────────────────────────────────────

/// `validate` on a corrupted MIX file (truncated garbage bytes) exits 1
/// and prints "INVALID".
///
/// Why: proves the MIX parser rejects clearly-invalid data through the
/// CLI pipeline with the correct exit code and user-facing error label.
#[test]
fn validate_corrupted_mix_exits_one() {
    let path = temp_file("corrupted.mix", &[0xFF; 5]);
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "expected nonzero exit");
    assert!(
        stderr.contains("INVALID"),
        "stderr should contain INVALID: {stderr}",
    );
    fs::remove_file(&path).ok();
}

/// `validate --format shp` on a valid PAL file exits 1 because the SHP
/// parser rejects PAL data.
///
/// Why: proves that `--format` truly overrides extension-based detection
/// and that format mismatches are reported as failures, not silently
/// accepted.
///
/// Note: uses a PAL with non-zero data because an all-zero 768-byte buffer
/// accidentally parses as a valid zero-frame SHP (frame_count=0).
#[test]
fn validate_wrong_format_exits_one() {
    // Build a PAL that won't be mistaken for a valid SHP: set bytes that
    // make the SHP header claim a large frame count requiring more data
    // than 768 bytes provides.
    let mut pal_data = valid_pal_bytes();
    pal_data[0] = 0xFF; // frame_count low byte = 255 (when read as u16 LE)
    pal_data[1] = 0x00; // frame_count high byte = 0 → 255 frames
                        // SHP parser will need (255+2)*8 + 14 = 2070 bytes, but only 768 available.
    let path = temp_file("mislabeled.pal", &pal_data);
    let output = Command::new(bin_path())
        .args(["validate", "--format", "shp", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "expected nonzero exit when SHP parser sees PAL data",
    );
    fs::remove_file(&path).ok();
}

/// `check` on a corrupted (truncated) MIX file exits 1 and prints "FAIL".
///
/// Why: proves the `check` subcommand reports parse errors with the
/// correct "FAIL" label and nonzero exit code, distinct from the
/// `validate` "INVALID" label.
#[test]
fn check_corrupted_mix_exits_one() {
    let path = temp_file("check_corrupted.mix", &[0xFF; 5]);
    let output = Command::new(bin_path())
        .args(["check", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "expected nonzero exit");
    assert!(
        stderr.contains("FAIL"),
        "stderr should contain FAIL: {stderr}",
    );
    fs::remove_file(&path).ok();
}

/// `extract` on a non-archive format (PAL) exits 1 with an error
/// mentioning "only supports archive formats".
///
/// Why: proves the `extract` subcommand rejects non-archive files with
/// a clear, actionable error message before attempting any file I/O.
#[test]
fn extract_non_archive_exits_one() {
    let path = temp_file("extract_reject.pal", &valid_pal_bytes());
    let output = Command::new(bin_path())
        .args(["extract", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "expected nonzero exit");
    assert!(
        stderr.contains("only supports archive formats"),
        "stderr should mention 'only supports archive formats': {stderr}",
    );
    fs::remove_file(&path).ok();
}

/// `fingerprint` on a nonexistent file exits nonzero.
///
/// Why: proves file-read errors in the `fingerprint` subcommand are
/// reported gracefully (not a panic) with a nonzero exit code.
#[test]
fn fingerprint_nonexistent_file_exits_nonzero() {
    let path = std::env::temp_dir().join("cnc_formats_nonexistent_fp.bin");
    // Ensure the file does not exist.
    fs::remove_file(&path).ok();
    let output = Command::new(bin_path())
        .args(["fingerprint", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "expected nonzero exit for missing file",
    );
}
