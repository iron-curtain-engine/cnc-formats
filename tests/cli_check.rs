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

fn invalid_shp_decode_bytes() -> Vec<u8> {
    let lcw_frame = [0xFEu8, 0x05, 0x00, 0x11, 0x80];
    let frame_count = 1u16;
    let header_size = 14u32;
    let offset_table_size = 3u32 * 8u32;
    let data_start = header_size + offset_table_size;
    let frame_offset = ((0x80u32) << 24) | data_start;
    let eof_offset = data_start + lcw_frame.len() as u32;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame_count.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&(lcw_frame.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&frame_offset.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&eof_offset.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&lcw_frame);
    bytes
}

fn invalid_wsa_decode_bytes() -> Vec<u8> {
    let frame = [0x22u8, 0x22, 0x22, 0x22];
    let mut bytes = cnc_formats::wsa::encode_frames(&[&frame], 2, 2).unwrap();
    let last = bytes.len().checked_sub(1).unwrap();
    bytes[last] = 0x00;
    bytes
}

fn build_mix(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut entries: Vec<(u32, &[u8])> = files
        .iter()
        .map(|(name, data)| (cnc_formats::mix::crc(name).to_raw(), *data))
        .collect();
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
fn check_valid_mix_exits_zero() {
    let mix = build_mix(&[("UNIT.SHP", &[0xAA; 32])]);
    let path = temp_file("test_check.mix", &mix);
    let output = Command::new(bin_path())
        .args(["check", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("OK"));
    fs::remove_file(&path).ok();
}

#[test]
fn check_valid_big_exits_zero() {
    let path = temp_file(
        "test_check.big",
        &build_big(&[("Data\\Audio\\Sounds\\test.wav", &[0xAA; 32])]),
    );
    let output = Command::new(bin_path())
        .args(["check", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("OK"));
    fs::remove_file(&path).ok();
}

#[test]
fn check_valid_vqp_exits_zero() {
    let path = temp_file("test_check.vqp", &valid_vqp_bytes(1));
    let output = Command::new(bin_path())
        .args(["check", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("OK"));
    fs::remove_file(&path).ok();
}

#[test]
fn check_valid_lut_exits_zero() {
    let path = temp_file("test_check.lut", &valid_lut_bytes());
    let output = Command::new(bin_path())
        .args(["check", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("OK"));
    fs::remove_file(&path).ok();
}

#[test]
fn check_valid_dip_exits_zero() {
    let path = temp_file("test_check.dip", &valid_segmented_dip_bytes());
    let output = Command::new(bin_path())
        .args(["check", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("OK"));
    fs::remove_file(&path).ok();
}

#[test]
fn check_valid_eng_exits_zero() {
    let path = temp_file("test_check.eng", &valid_eng_bytes());
    let output = Command::new(bin_path())
        .args(["check", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("OK"));
    fs::remove_file(&path).ok();
}

#[test]
fn fingerprint_known_sha256() {
    let path = temp_file("fingerprint.bin", b"abc");
    let output = Command::new(bin_path())
        .args(["fingerprint", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
    fs::remove_file(&path).ok();
}

#[test]
fn check_corrupted_mix_exits_one() {
    let path = temp_file("check_corrupted.mix", &[0xFF; 5]);
    let output = Command::new(bin_path())
        .args(["check", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("FAIL"));
    fs::remove_file(&path).ok();
}

#[test]
fn check_invalid_shp_decode_matches_validate_failure() {
    let path = temp_file("check_invalid_decode.shp", &invalid_shp_decode_bytes());
    let validate = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    let validate_stderr = String::from_utf8_lossy(&validate.stderr);
    assert!(
        !validate.status.success(),
        "validate should fail on decode corruption: {validate_stderr}",
    );

    let output = Command::new(bin_path())
        .args(["check", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "check should fail on decode corruption"
    );
    assert!(stderr.contains("frame decode failed"), "stderr: {stderr}");
    fs::remove_file(&path).ok();
}

#[test]
fn check_invalid_wsa_decode_exits_one() {
    let path = temp_file("check_invalid_decode.wsa", &invalid_wsa_decode_bytes());
    let validate = Command::new(bin_path())
        .args(["validate", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        validate.status.success(),
        "validate should still succeed: {}",
        String::from_utf8_lossy(&validate.stderr),
    );

    let output = Command::new(bin_path())
        .args(["check", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "check should fail on decode corruption"
    );
    assert!(stderr.contains("frame decode failed"), "stderr: {stderr}");
    fs::remove_file(&path).ok();
}

#[test]
fn fingerprint_nonexistent_file_exits_nonzero() {
    let path = std::env::temp_dir().join("cnc_formats_nonexistent_fp.bin");
    fs::remove_file(&path).ok();
    let output = Command::new(bin_path())
        .args(["fingerprint", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
}
