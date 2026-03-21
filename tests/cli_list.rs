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

    let records_total = files.len() * 20;
    let data_start = out.len() + records_total;

    let mut offset = data_start as u32;
    let mut offsets = Vec::with_capacity(files.len());
    for (_, data) in files {
        offsets.push(offset);
        offset = offset.saturating_add(data.len() as u32);
    }

    for (i, (_, data)) in files.iter().enumerate() {
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(i as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offsets[i].to_le_bytes());
        out.extend_from_slice(&(i as u32).to_le_bytes());
    }

    for (_, data) in files {
        out.extend_from_slice(data);
    }

    out
}

#[test]
fn list_mix_exits_zero() {
    let mix = build_mix(&[("UNIT.SHP", &[0xAA; 100]), ("SPEECH.AUD", &[0xBB; 50])]);
    let path = temp_file("test_list.mix", &mix);
    let output = Command::new(bin_path())
        .args(["list", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("2 entries"));
    fs::remove_file(&path).ok();
}

#[test]
fn list_big_exits_zero() {
    let big = build_big(&[("Data\\Audio\\Sounds\\test.wav", &[0xAA; 8])]);
    let path = temp_file("test_list.big", &big);
    let output = Command::new(bin_path())
        .args(["list", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("Data\\Audio\\Sounds\\test.wav"));
    fs::remove_file(&path).ok();
}

#[test]
fn list_big_ignores_names_flag() {
    let big = build_big(&[("Data\\Audio\\Sounds\\test.wav", &[0xAA; 8])]);
    let path = temp_file("test_names_ignored.big", &big);
    let missing_names = std::env::temp_dir().join("cnc_formats_missing_big_names.txt");
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
    assert!(output.status.success());
    assert!(stderr.contains("ignored"));
    fs::remove_file(&path).ok();
}

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
    assert!(output.status.success());
    assert!(stdout.contains("UNIT.SHP"));
    fs::remove_file(&mix_path).ok();
    fs::remove_file(&names_path).ok();
}

#[test]
fn list_mix_eager_access_matches_stream_output() {
    let mix = build_mix(&[("UNIT.SHP", &[0xAA; 100]), ("SPEECH.AUD", &[0xBB; 50])]);
    let path = temp_file("test_list_access_modes.mix", &mix);

    let stream_output = Command::new(bin_path())
        .args(["list", path.to_str().unwrap(), "--mix-access", "stream"])
        .output()
        .unwrap();
    let eager_output = Command::new(bin_path())
        .args(["list", path.to_str().unwrap(), "--mix-access", "eager"])
        .output()
        .unwrap();

    assert!(stream_output.status.success());
    assert!(eager_output.status.success());
    assert_eq!(stream_output.stdout, eager_output.stdout);

    fs::remove_file(&path).ok();
}

#[test]
fn list_big_warns_when_mix_access_is_ignored() {
    let big = build_big(&[("Data\\Audio\\Sounds\\test.wav", &[0xAA; 8])]);
    let path = temp_file("test_list_mix_access_ignored.big", &big);

    let output = Command::new(bin_path())
        .args(["list", path.to_str().unwrap(), "--mix-access", "eager"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success());
    assert!(stderr.contains("--mix-access is ignored for BIG archives"));

    fs::remove_file(&path).ok();
}

#[test]
fn list_non_archive_exits_one() {
    let path = temp_file("test_list.pal", &valid_pal_bytes());
    let output = Command::new(bin_path())
        .args(["list", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("archive"));
    fs::remove_file(&path).ok();
}

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
    assert!(output.status.success());
    assert!(stdout.contains("DATA/UNIT.SHP"));
    fs::remove_file(&path).ok();
}

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
    assert!(output.status.success());
    assert!(stderr.contains("ignored"));
    fs::remove_file(&path).ok();
}
