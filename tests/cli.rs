// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

#![cfg(feature = "cli")]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn bin_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cncf"))
}

fn temp_file(name: &str, content: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join("cnc_formats_cli_tests");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path
}

fn valid_pal_bytes() -> Vec<u8> {
    vec![0u8; 768]
}

fn valid_ini_bytes() -> Vec<u8> {
    b"[General]\nName=Test\n".to_vec()
}

fn valid_lut_bytes() -> Vec<u8> {
    let mut out = Vec::with_capacity(cnc_formats::lut::LUT_FILE_SIZE);
    for i in 0..cnc_formats::lut::LUT_ENTRY_COUNT {
        out.push((i % 64) as u8);
        out.push(((i / 64) % 64) as u8);
        out.push(((i / 256) % 16) as u8);
    }
    out
}

fn valid_eng_bytes() -> Vec<u8> {
    let strings: [&[u8]; 3] = [b"", b"Sell", b"Mission Failed"];
    let table_len = strings.len() * 2;
    let mut out = vec![0u8; table_len];
    let mut offset = table_len as u16;
    for (i, bytes) in strings.iter().enumerate() {
        out[i * 2..i * 2 + 2].copy_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(bytes);
        out.push(0);
        offset = offset.saturating_add(bytes.len() as u16).saturating_add(1);
    }
    out
}

fn valid_segmented_dip_bytes() -> Vec<u8> {
    let section0 = [0x00, 0x00, 0x3C, 0x3C];
    let section1 = [0x01, 0x80, 0x00, 0x00];
    let trailer = [0x0B, 0x80];
    let header_size = 12u16;
    let end0 = header_size as usize + section0.len();
    let end1 = end0 + section1.len();

    let mut out = Vec::new();
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&header_size.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(end0 as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(end1 as u16).to_le_bytes());
    out.extend_from_slice(&section0);
    out.extend_from_slice(&section1);
    out.extend_from_slice(&trailer);
    out
}

fn valid_vqp_bytes(table_count: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&table_count.to_le_bytes());
    for table in 0..table_count {
        for row in 0u16..256 {
            for col in 0u16..=row {
                let value = ((table as u16).wrapping_add(row).wrapping_add(col) & 0xFF) as u8;
                out.push(value);
            }
        }
    }
    out
}

fn build_big(files: &[(&str, &[u8])]) -> Vec<u8> {
    let table_size: usize = files.iter().map(|(name, _)| 8 + name.len() + 1).sum();
    let data_start = 16 + table_size;
    let archive_size = data_start + files.iter().map(|(_, data)| data.len()).sum::<usize>();

    let mut out = Vec::with_capacity(archive_size);
    out.extend_from_slice(b"BIGF");
    out.extend_from_slice(&(archive_size as u32).to_le_bytes());
    out.extend_from_slice(&(files.len() as u32).to_be_bytes());
    out.extend_from_slice(&(data_start as u32).to_be_bytes());

    let mut offset = data_start as u32;
    for (name, data) in files {
        out.extend_from_slice(&offset.to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        offset = offset.saturating_add(data.len() as u32);
    }

    for (_, data) in files {
        out.extend_from_slice(data);
    }

    out
}

#[test]
fn validate_valid_pal_exits_zero() {
    let path = temp_file("valid.pal", &valid_pal_bytes());
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("OK"));
    fs::remove_file(&path).ok();
}

#[test]
fn validate_invalid_pal_exits_one() {
    let path = temp_file("invalid.pal", &[0u8; 100]);
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("INVALID"));
    fs::remove_file(&path).ok();
}

#[test]
fn validate_format_override() {
    let path = temp_file("override.dat", &valid_pal_bytes());
    let output = Command::new(bin_path())
        .args(["validate", "--format", "pal", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("OK"));
    fs::remove_file(&path).ok();
}

#[test]
fn validate_valid_ini() {
    let path = temp_file("valid.ini", &valid_ini_bytes());
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    fs::remove_file(&path).ok();
}

#[test]
fn validate_valid_eng() {
    let path = temp_file("valid.eng", &valid_eng_bytes());
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("OK"));
    fs::remove_file(&path).ok();
}

#[test]
fn validate_valid_lut() {
    let path = temp_file("valid.lut", &valid_lut_bytes());
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("OK"));
    fs::remove_file(&path).ok();
}

#[test]
fn validate_valid_dip() {
    let path = temp_file("valid.dip", &valid_segmented_dip_bytes());
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("OK"));
    fs::remove_file(&path).ok();
}

#[test]
fn validate_localized_eng_extension() {
    let path = temp_file("valid.ger", &valid_eng_bytes());
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    fs::remove_file(&path).ok();
}

#[test]
fn validate_valid_vqp() {
    let path = temp_file("valid.vqp", &valid_vqp_bytes(1));
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("OK"));
    fs::remove_file(&path).ok();
}

#[test]
fn validate_valid_big() {
    let path = temp_file(
        "valid.big",
        &build_big(&[("Data\\INI\\GameData.ini", b"[A]\nB=C\n")]),
    );
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("OK"));
    fs::remove_file(&path).ok();
}

#[test]
fn validate_nonexistent_file() {
    let path = std::env::temp_dir().join("cnc_formats_nonexistent.pal");
    fs::remove_file(&path).ok();
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn validate_unknown_extension() {
    let path = temp_file("unknown.xyz", &[0u8; 100]);
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("Cannot detect format"));
    fs::remove_file(&path).ok();
}

#[test]
fn validate_tmp_extension_is_ambiguous() {
    let path = temp_file("terrain.tmp", &[0u8; 256]);
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("Cannot detect format"));
    fs::remove_file(&path).ok();
}

#[test]
fn validate_tmp_with_explicit_format_td() {
    let path = temp_file("explicit_td.tmp", &[0u8; 256]);
    let output = Command::new(bin_path())
        .args(["validate", "--format", "tmp", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("Cannot detect format"));
    fs::remove_file(&path).ok();
}

#[test]
fn validate_tmp_with_explicit_format_ra() {
    let path = temp_file("explicit_ra.tmp", &[0u8; 256]);
    let output = Command::new(bin_path())
        .args(["validate", "--format", "tmp-ra", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("INVALID"));
    assert!(!stderr.contains("Cannot detect format"));
    fs::remove_file(&path).ok();
}

#[test]
fn identify_big_exits_zero() {
    let big = build_big(&[("DATA/UNIT.INI", b"[Unit]\nName=Tank\n")]);
    let path = temp_file("unknown.bin", &big);
    let output = Command::new(bin_path())
        .args(["identify", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert_eq!(stdout.trim(), "big");
    fs::remove_file(&path).ok();
}

#[test]
fn validate_pal_with_trailing_data_exits_one() {
    let mut data = valid_pal_bytes();
    data.push(0xFF);
    let path = temp_file("oversized.bin", &data);
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap(), "--format", "pal"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("PAL file size"));
    fs::remove_file(&path).ok();
}

#[test]
fn validate_corrupted_mix_exits_one() {
    let path = temp_file("corrupted.mix", &[0xFF; 5]);
    let output = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("INVALID"));
    fs::remove_file(&path).ok();
}

#[test]
fn validate_wrong_format_exits_one() {
    let mut pal_data = valid_pal_bytes();
    pal_data[0] = 0xFF;
    pal_data[1] = 0x00;
    let path = temp_file("mislabeled.pal", &pal_data);
    let output = Command::new(bin_path())
        .args(["validate", "--format", "shp", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    fs::remove_file(&path).ok();
}
