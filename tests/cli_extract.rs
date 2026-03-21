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

fn valid_lut_bytes() -> Vec<u8> {
    let mut out = Vec::with_capacity(cnc_formats::lut::LUT_FILE_SIZE);
    for i in 0..cnc_formats::lut::LUT_ENTRY_COUNT {
        out.push((i % 64) as u8);
        out.push(((i / 64) % 64) as u8);
        out.push(((i / 256) % 16) as u8);
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
fn extract_mix_exits_zero() {
    let mix = build_mix(&[("UNIT.SHP", &[0xAA; 100])]);
    let mix_path = temp_file("test_extract.mix", &mix);
    let out_dir = std::env::temp_dir()
        .join("cnc_formats_cli_tests")
        .join("test_extract_out");
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
    assert!(output.status.success());
    let files: Vec<_> = fs::read_dir(&out_dir).unwrap().collect();
    assert_eq!(files.len(), 1);
    fs::remove_dir_all(&out_dir).ok();
    fs::remove_file(&mix_path).ok();
}

#[test]
fn extract_mix_eager_access_exits_zero() {
    let mix = build_mix(&[("UNIT.SHP", &[0xAA; 100])]);
    let mix_path = temp_file("test_extract_eager.mix", &mix);
    let out_dir = std::env::temp_dir()
        .join("cnc_formats_cli_tests")
        .join("test_extract_eager_out");
    let _ = fs::remove_dir_all(&out_dir);

    let output = Command::new(bin_path())
        .args([
            "extract",
            mix_path.to_str().unwrap(),
            "--mix-access",
            "eager",
            "--output",
            out_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let files: Vec<_> = fs::read_dir(&out_dir).unwrap().collect();
    assert_eq!(files.len(), 1);
    fs::remove_dir_all(&out_dir).ok();
    fs::remove_file(&mix_path).ok();
}

#[test]
fn extract_mix_unknown_lut_uses_lut_extension() {
    let mix = build_mix(&[("UNKNOWN.DAT", &valid_lut_bytes())]);
    let mix_path = temp_file("test_extract_unknown_lut.mix", &mix);
    let out_dir = std::env::temp_dir()
        .join("cnc_formats_cli_tests")
        .join("test_extract_unknown_lut_out");
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
    assert!(output.status.success());

    let crc_name = format!("{:08X}.lut", cnc_formats::mix::crc("UNKNOWN.DAT").to_raw());
    assert!(out_dir.join(&crc_name).exists());

    fs::remove_dir_all(&out_dir).ok();
    fs::remove_file(&mix_path).ok();
}

#[test]
fn extract_mix_unknown_dip_uses_dip_extension() {
    let mix = build_mix(&[("UNKNOWN.DAT", &valid_segmented_dip_bytes())]);
    let mix_path = temp_file("test_extract_unknown_dip.mix", &mix);
    let out_dir = std::env::temp_dir()
        .join("cnc_formats_cli_tests")
        .join("test_extract_unknown_dip_out");
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
    assert!(output.status.success());

    let crc_name = format!("{:08X}.dip", cnc_formats::mix::crc("UNKNOWN.DAT").to_raw());
    assert!(out_dir.join(&crc_name).exists());

    fs::remove_dir_all(&out_dir).ok();
    fs::remove_file(&mix_path).ok();
}

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
    assert!(output.status.success());
    let unit_shp = out_dir.join("UNIT.SHP");
    assert!(unit_shp.exists());
    let content = fs::read(&unit_shp).unwrap();
    assert_eq!(content, vec![0xAA; 50]);
    fs::remove_dir_all(&out_dir).ok();
    fs::remove_file(&mix_path).ok();
    fs::remove_file(&names_path).ok();
}

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
    assert!(output.status.success());
    let files: Vec<_> = fs::read_dir(&out_dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.file_name()))
        .collect();
    assert_eq!(files.len(), 1);
    assert!(out_dir.join("UNIT.SHP").exists());
    assert!(!out_dir.join("SPEECH.AUD").exists());
    fs::remove_dir_all(&out_dir).ok();
    fs::remove_file(&mix_path).ok();
    fs::remove_file(&names_path).ok();
}

#[test]
fn extract_mix_duplicate_names_get_unique_files() {
    let vqp_a = valid_vqp_bytes(1);
    let vqp_b = valid_vqp_bytes(2);
    let mix = build_mix(&[("DUP.VQP", &vqp_a), ("DUP.VQP", &vqp_b)]);
    let mix_path = temp_file("test_extract_duplicate_names.mix", &mix);
    let names_path = temp_file("test_extract_duplicate_names.txt", b"DUP.VQP\n");
    let out_dir = std::env::temp_dir()
        .join("cnc_formats_cli_tests")
        .join("test_extract_duplicate_names_out");
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
    assert!(output.status.success());

    let files: Vec<_> = fs::read_dir(&out_dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.file_name()))
        .collect();
    assert_eq!(files.len(), 2);

    let crc = cnc_formats::mix::crc("DUP.VQP").to_raw();
    let fallback = format!("{crc:08X}.vqp");
    assert!(out_dir.join("DUP.VQP").exists());
    assert!(out_dir.join(&fallback).exists());
    fs::remove_dir_all(&out_dir).ok();
    fs::remove_file(&mix_path).ok();
    fs::remove_file(&names_path).ok();
}

#[test]
fn extract_big_exits_zero() {
    let big = build_big(&[("Data\\Audio\\Sounds\\test.wav", &[0xAA; 12])]);
    let big_path = temp_file("test_extract.big", &big);
    let out_dir = std::env::temp_dir()
        .join("cnc_formats_cli_tests")
        .join("test_extract_big_out");
    let _ = fs::remove_dir_all(&out_dir);

    let output = Command::new(bin_path())
        .args([
            "extract",
            big_path.to_str().unwrap(),
            "--output",
            out_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let extracted = out_dir
        .join("Data")
        .join("Audio")
        .join("Sounds")
        .join("test.wav");
    assert!(extracted.exists());
    assert_eq!(fs::read(&extracted).unwrap(), vec![0xAA; 12]);
    fs::remove_dir_all(&out_dir).ok();
    fs::remove_file(&big_path).ok();
}

#[test]
fn extract_big_duplicate_names_get_unique_files() {
    let big = build_big(&[
        ("Data\\Audio\\dup.wav", b"first"),
        ("Data\\Audio\\dup.wav", b"second"),
    ]);
    let big_path = temp_file("test_extract_duplicate.big", &big);
    let out_dir = std::env::temp_dir()
        .join("cnc_formats_cli_tests")
        .join("test_extract_duplicate_big_out");
    let _ = fs::remove_dir_all(&out_dir);

    let output = Command::new(bin_path())
        .args([
            "extract",
            big_path.to_str().unwrap(),
            "--output",
            out_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    let first = out_dir.join("Data").join("Audio").join("dup.wav");
    let second = out_dir.join("Data").join("Audio").join("dup__2.wav");
    assert!(first.exists());
    assert!(second.exists());
    assert_eq!(fs::read(&first).unwrap(), b"first");
    assert_eq!(fs::read(&second).unwrap(), b"second");

    fs::remove_dir_all(&out_dir).ok();
    fs::remove_file(&big_path).ok();
}

#[test]
fn extract_big_warns_when_mix_access_is_ignored() {
    let big = build_big(&[("Data\\Audio\\Sounds\\test.wav", &[0xAA; 12])]);
    let big_path = temp_file("test_extract_mix_access_ignored.big", &big);
    let out_dir = std::env::temp_dir()
        .join("cnc_formats_cli_tests")
        .join("test_extract_mix_access_ignored_out");
    let _ = fs::remove_dir_all(&out_dir);

    let output = Command::new(bin_path())
        .args([
            "extract",
            big_path.to_str().unwrap(),
            "--mix-access",
            "eager",
            "--output",
            out_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success());
    assert!(stderr.contains("--mix-access is ignored for BIG archives"));

    fs::remove_dir_all(&out_dir).ok();
    fs::remove_file(&big_path).ok();
}

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
    assert!(output.status.success());
    let unit_shp = out_dir.join("DATA").join("UNIT.SHP");
    assert!(unit_shp.exists());
    assert_eq!(fs::read(&unit_shp).unwrap(), vec![0xAA; 12]);
    fs::remove_dir_all(&out_dir).ok();
    fs::remove_file(&meg_path).ok();
}

#[test]
fn extract_non_archive_exits_one() {
    let path = temp_file("extract_reject.pal", &valid_pal_bytes());
    let output = Command::new(bin_path())
        .args(["extract", path.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("only supports archive formats"));
    fs::remove_file(&path).ok();
}
