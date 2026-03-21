// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

/// Create a unique temporary directory for extraction-path tests.
///
/// Why: each test needs an isolated filesystem boundary so path
/// validation and file creation do not interfere with parallel runs.
fn temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("cnc_formats_{prefix}_{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Safe nested filenames keep their relative subdirectories.
///
/// Why: MIX name maps may contain legitimate path components.  The
/// extractor should preserve them instead of flattening everything.
#[test]
fn resolve_output_path_preserves_nested_relative_name() {
    let dir = temp_dir("extract_nested");
    let boundary = PathBoundary::try_new(&dir).unwrap();

    let (strict_path, relative, warning) = resolve_output_name(
        &boundary,
        Some("tiles/desert/unit.shp"),
        MixCrc::from_raw(0x1234_5678),
        None,
    )
    .unwrap();

    assert_eq!(relative, "tiles/desert/unit.shp");
    assert!(warning.is_none());
    let path = strict_path.expect("safe metadata path should stay strict");
    path.create_parent_dir_all().unwrap();
    path.write([0xAA, 0xBB]).unwrap();
    assert!(dir.join("tiles").join("desert").join("unit.shp").exists());

    fs::remove_dir_all(&dir).ok();
}

/// Path traversal names fall back to a deterministic CRC filename.
///
/// Why: hostile `--names` input must not escape the extraction
/// boundary, but it also should not abort the whole archive dump.
#[test]
fn resolve_output_path_traversal_falls_back_to_crc_name() {
    let dir = temp_dir("extract_traversal");
    let boundary = PathBoundary::try_new(&dir).unwrap();
    let crc = MixCrc::from_raw(0xDEAD_BEEF);

    let (strict_path, relative, warning) =
        resolve_output_name(&boundary, Some("../../evil.shp"), crc, None).unwrap();

    assert_eq!(relative, "DEADBEEF.bin");
    assert!(warning.is_some());
    assert!(strict_path.is_none());
    let path = generated_flat_output_path(&boundary, &relative).unwrap();
    fs::write(&path, [0xCC]).unwrap();
    assert!(dir.join("DEADBEEF.bin").exists());
    assert!(!dir.join("evil.shp").exists());

    fs::remove_dir_all(&dir).ok();
}

/// Duplicate logical filenames are rewritten instead of overwriting.
#[test]
fn duplicate_output_name_uses_unique_fallback() {
    let mut used = HashSet::new();
    let first = make_output_name_unique(None, "ALLY1.VQP".to_string(), "1234ABCD.vqp", &mut used);
    let second = make_output_name_unique(None, "ALLY1.VQP".to_string(), "1234ABCD.vqp", &mut used);

    assert_eq!(first.1, "ALLY1.VQP");
    assert!(first.2.is_none());
    assert_eq!(second.1, "1234ABCD.vqp");
    assert!(second.2.is_some());

    let third = make_output_name_unique(None, "ALLY1.VQP".to_string(), "1234ABCD.vqp", &mut used);
    assert_eq!(third.1, "1234ABCD__2.vqp");
}

/// Duplicate stored archive paths get suffixed in place instead of
/// flattening to unrelated fallback names.
#[test]
fn duplicate_stored_name_preserves_parent_dirs() {
    let mut used = HashSet::new();
    let first = make_stored_name_unique("Data/Audio/test.wav", &mut used);
    let second = make_stored_name_unique("Data/Audio/test.wav", &mut used);

    assert_eq!(first.0, "Data/Audio/test.wav");
    assert!(first.1.is_none());
    assert_eq!(second.0, "Data/Audio/test__2.wav");
    assert!(second.1.is_some());
}

/// Sniffed LUT content gets a `.lut` fallback extension.
#[test]
fn fallback_filename_uses_lut_extension() {
    let mut data = Vec::with_capacity(cnc_formats::lut::LUT_FILE_SIZE);
    for i in 0..cnc_formats::lut::LUT_ENTRY_COUNT {
        data.push((i % 64) as u8);
        data.push(((i / 64) % 64) as u8);
        data.push(((i / 256) % 16) as u8);
    }

    let name = fallback_filename(MixCrc::from_raw(0xDEAD_BEEF), None, Some(&data));
    assert_eq!(name, "DEADBEEF.lut");
}

/// Segmented setup-data DIPs get a `.dip` fallback extension.
#[test]
fn fallback_filename_uses_dip_extension() {
    let data = [
        0x02, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0x3C,
        0x3C, 0x01, 0x80, 0x00, 0x00, 0x0B, 0x80,
    ];

    let name = fallback_filename(MixCrc::from_raw(0xDEAD_BEEF), None, Some(&data));
    assert_eq!(name, "DEADBEEF.dip");
}
