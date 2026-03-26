// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Format detection by content inspection ("magic byte sniffing").
//!
//! MIX archives store CRC hashes, not filenames, and most C&C binary
//! formats lack magic bytes.  This module provides best-effort format
//! detection by trying lightweight header probes in priority order.
//!
//! ## Reliability
//!
//! Detection is **heuristic** — some formats (SHP, WSA, TMP) have no
//! magic bytes, so detection works by checking whether the header fields
//! form a self-consistent structure.  False positives are possible for
//! short files.  Callers should treat the result as a hint, not a proof.
//!
//! ## Usage
//!
//! ```rust
//! use cnc_formats::sniff::sniff_format;
//!
//! // A 768-byte buffer with all values ≤ 63 is detected as a PAL palette.
//! let data = vec![32u8; 768];
//! assert_eq!(sniff_format(&data), Some("pal"));
//!
//! // Unknown data returns None.
//! assert_eq!(sniff_format(&[0xDE, 0xAD]), None);
//! ```

use crate::read::{read_u16_le, read_u32_le};

/// Inspects the first bytes of `data` and returns a file extension hint.
///
/// Returns `Some("shp")`, `Some("pal")`, etc. for recognised formats,
/// or `None` if the data doesn't match any known pattern.
///
/// Probes are ordered from most-specific (magic-byte formats) to
/// least-specific (heuristic formats) to minimise false positives.
pub fn sniff_format(data: &[u8]) -> Option<&'static str> {
    // ── Formats with strong magic bytes (most reliable) ─────────────
    if is_vqa(data) {
        return Some("vqa");
    }
    if is_big(data) {
        return Some("big");
    }
    if is_meg(data) {
        return Some("meg");
    }
    if is_vxl(data) {
        return Some("vxl");
    }
    if is_csf(data) {
        return Some("csf");
    }
    if is_voc(data) {
        return Some("voc");
    }
    if is_dds(data) {
        return Some("dds");
    }
    if is_apt(data) {
        return Some("apt");
    }
    if is_map_sage(data) {
        return Some("map_sage");
    }
    if is_jpg(data) {
        return Some("jpg");
    }
    if is_tga_footer(data) {
        return Some("tga");
    }
    if is_mix(data) {
        return Some("mix");
    }
    if is_ini(data) {
        return Some("ini");
    }

    // ── Formats with structural signatures (good reliability) ───────
    if is_fnt(data) {
        return Some("fnt");
    }
    if is_dip(data) {
        return Some("dip");
    }
    if is_lut(data) {
        return Some("lut");
    }
    if is_shp_ts(data) {
        return Some("shp_ts");
    }

    // ── Exact-size format ───────────────────────────────────────────
    if is_pal(data) {
        return Some("pal");
    }
    if is_vqp(data) {
        return Some("vqp");
    }
    if is_eng(data) {
        return Some("eng");
    }

    // ── Heuristic formats (try parser, least reliable) ──────────────
    if is_cps(data) {
        return Some("cps");
    }
    if is_shp(data) {
        return Some("shp");
    }
    if is_wsa(data) {
        return Some("wsa");
    }
    if is_aud(data) {
        return Some("aud");
    }
    if is_w3d(data) {
        return Some("w3d");
    }
    if is_hva(data) {
        return Some("hva");
    }
    if is_tga(data) {
        return Some("tga");
    }

    None
}

/// VXL: strong 16-byte magic "Voxel Animation\0".
fn is_vxl(data: &[u8]) -> bool {
    data.len() >= 16 && data.get(..16) == Some(b"Voxel Animation\0")
}

/// CSF: strong 4-byte magic " FSC" (space + F + S + C).
fn is_csf(data: &[u8]) -> bool {
    data.len() >= 4 && data.get(..4) == Some(b" FSC")
}

/// TS/RA2 SHP: first u16 is 0, followed by plausible dimensions and frame count.
/// Placed after FNT/DIP/LUT (structural) but before heuristic formats.
fn is_shp_ts(data: &[u8]) -> bool {
    crate::shp_ts::ShpTsFile::parse(data).is_ok()
}

/// CPS: 10-byte header with plausible compression and buffer size.
/// CPS files are small (typically 64 KB range for 320×200 images), so
/// reject files that are implausibly large or too small for the header.
fn is_cps(data: &[u8]) -> bool {
    // Must have at least the 10-byte header.
    if data.len() < 10 {
        return false;
    }
    // Plausibility: compression must be 0 (raw) or 4 (LCW).
    let comp = match crate::read::read_u16_le(data, 6) {
        Ok(c) => c,
        Err(_) => return false,
    };
    if comp != 0 && comp != 4 {
        return false;
    }
    crate::cps::CpsFile::parse(data).is_ok()
}

/// W3D: recursive chunk-based 3D mesh format. Requires at least one valid
/// chunk (8-byte header minimum) and successful parse with at least one chunk.
fn is_w3d(data: &[u8]) -> bool {
    if data.len() < 8 {
        return false;
    }
    match crate::w3d::W3dFile::parse(data) {
        Ok(w3d) => !w3d.chunks.is_empty(),
        Err(_) => false,
    }
}

/// HVA: hierarchical voxel animation. No magic bytes; requires at least the
/// 24-byte header and successful parse with at least one section.
fn is_hva(data: &[u8]) -> bool {
    if data.len() < 24 {
        return false;
    }
    match crate::hva::HvaFile::parse(data) {
        Ok(hva) => hva.header.num_sections > 0 && hva.header.num_frames > 0,
        Err(_) => false,
    }
}

/// Segmented DIP installer data: section table followed by control streams.
///
/// String-table DIPs intentionally continue to sniff as `eng`; only the
/// non-string segmented variant needs content-based recovery from anonymous
/// archive blobs.
fn is_dip(data: &[u8]) -> bool {
    crate::dip::DipSegmentedFile::parse(data).is_ok()
}

/// ENG-family string table: offset table followed by NUL-terminated strings.
fn is_eng(data: &[u8]) -> bool {
    if data.len() < 8 {
        return false;
    }
    let eng = match crate::eng::EngFile::parse(data) {
        Ok(eng) => eng,
        Err(_) => return false,
    };

    let mut total = 0usize;
    let mut printable = 0usize;
    for entry in eng
        .strings
        .iter()
        .filter(|entry| !entry.bytes.is_empty())
        .take(8)
    {
        for &byte in entry.bytes.iter().take(64) {
            total = total.saturating_add(1);
            if byte.is_ascii_graphic() || byte == b' ' || byte == b'\t' {
                printable = printable.saturating_add(1);
            }
        }
    }

    total > 0 && printable.saturating_mul(10) >= total.saturating_mul(9)
}

/// Petroglyph MEG archives use a strong 8-byte header marker.
fn is_meg(data: &[u8]) -> bool {
    if data.len() < 8 {
        return false;
    }
    matches!(
        (read_u32_le(data, 0).ok(), read_u32_le(data, 4).ok(),),
        (Some(0xFFFF_FFFF), Some(0x3F7D_70A4)) | (Some(0x8FFF_FFFF), Some(0x3F7D_70A4))
    )
}

/// Red Alert Chrono Vortex LUT: 4,096 triplets with tightly bounded values.
fn is_lut(data: &[u8]) -> bool {
    crate::lut::LutFile::parse(data).is_ok()
}

/// VQP: 4-byte count followed by `count * 32,896` bytes of packed tables.
fn is_vqp(data: &[u8]) -> bool {
    let header = match data.get(..4) {
        Some(header) => header,
        None => return false,
    };

    let mut buf = [0u8; 4];
    buf.copy_from_slice(header);
    let table_count = u32::from_le_bytes(buf) as usize;
    let table_bytes = match table_count.checked_mul(crate::vqp::VQP_TABLE_SIZE) {
        Some(bytes) => bytes,
        None => return false,
    };
    let expected = match 4usize.checked_add(table_bytes) {
        Some(expected) => expected,
        None => return false,
    };

    data.len() == expected
}

/// VQA: IFF container with `FORM` + `WVQA` magic.
fn is_vqa(data: &[u8]) -> bool {
    data.len() >= 12 && data.get(..4) == Some(b"FORM") && data.get(8..12) == Some(b"WVQA")
}

/// BIG: EA archive with `BIGF`/`BIG4` magic.
fn is_big(data: &[u8]) -> bool {
    data.len() >= 16 && matches!(data.get(..4), Some(b"BIGF") | Some(b"BIG4"))
}

/// MIX: extended format starts with `0x0000` followed by flags, OR
/// basic format with a small count and plausible data_size.
///
/// Basic format: count (u16) != 0, data_size (u32) at offset 2.
/// `6 + count*12 + data_size` should roughly equal the file size.
fn is_mix(data: &[u8]) -> bool {
    // MIX archives are always at least a few hundred bytes in practice.
    // Require a minimum to avoid false positives on small files whose
    // first bytes happen to be zeros.
    if data.len() < 100 {
        return false;
    }
    let w0 = match read_u16_le(data, 0) {
        Ok(word) => word,
        Err(_) => return false,
    };
    if w0 == 0 {
        // Extended format marker — flags must be 0x0001..0x0003.
        // (Flags 0x0000 with marker 0x0000 is just 4 zero bytes, too ambiguous.)
        // Encrypted archives (flag 0x0002) must be >= 90 bytes (4 + 80 key + header).
        if data.len() >= 90 {
            let flags = match read_u16_le(data, 2) {
                Ok(flags) => flags,
                Err(_) => return false,
            };
            return (1..=0x0003).contains(&flags);
        }
        return false;
    }
    // Basic format: count is non-zero; heuristic check.
    let count = w0 as usize;
    let data_size = match read_u32_le(data, 2) {
        Ok(size) => size as usize,
        Err(_) => return false,
    };
    let header_plus_index = 6usize.saturating_add(count.saturating_mul(12));
    let expected_total = header_plus_index.saturating_add(data_size);
    // File should be at least header + index, and the total (header +
    // index + data) should be close to the actual file size (within 20
    // bytes for trailing SHA-1 digests and padding).
    data.len() >= header_plus_index
        && expected_total <= data.len().saturating_add(20)
        && expected_total >= data.len().saturating_sub(20)
}

/// INI: ASCII text that contains `[` within the first 4 KB.
fn is_ini(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    let check_len = data.len().min(4096);
    let prefix = data.get(..check_len).unwrap_or(data);
    // Must be mostly printable ASCII.
    let ascii_count = prefix
        .iter()
        .filter(|&&b| b.is_ascii_graphic() || b.is_ascii_whitespace())
        .count();
    if ascii_count < prefix.len() * 9 / 10 {
        return false;
    }
    // Must contain a section header `[`.
    prefix.contains(&b'[')
}

fn is_aud(data: &[u8]) -> bool {
    if data.len() < 12 {
        return false;
    }

    let sample_rate = match read_u16_le(data, 0) {
        Ok(rate) => rate,
        Err(_) => return false,
    };
    if sample_rate == 0 {
        return false;
    }

    let compressed_size = match read_u32_le(data, 2) {
        Ok(size) => size as usize,
        Err(_) => return false,
    };
    let compression = match data.get(11) {
        Some(&compression) => compression,
        None => return false,
    };
    if compression != 1 && compression != 99 {
        return false;
    }

    let expected = compressed_size.saturating_add(12);
    expected <= data.len().saturating_add(64) && expected >= data.len().saturating_sub(64)
}

/// FNT: 20-byte header with `data_blocks == 5` at offset 6.
fn is_fnt(data: &[u8]) -> bool {
    crate::fnt::FntFile::parse(data).is_ok()
}

/// PAL: exactly 768 bytes with all values in 0–63 range (6-bit VGA).
fn is_pal(data: &[u8]) -> bool {
    if data.len() != 768 {
        return false;
    }
    // All bytes should be 0–63 for a valid VGA palette.
    data.iter().all(|&b| b <= 63)
}

/// SHP: try parsing the header to see if it forms a consistent structure.
fn is_shp(data: &[u8]) -> bool {
    let shp = match crate::shp::ShpFile::parse(data) {
        Ok(shp) => shp,
        Err(_) => return false,
    };
    let pixel_count = shp.frame_pixel_count();
    for frame in &shp.frames {
        if crate::lcw::decompress(frame.data, pixel_count).is_err() {
            return false;
        }
    }
    true
}

/// WSA: 14-byte header with plausible frame count, dimensions, and offset table.
fn is_wsa(data: &[u8]) -> bool {
    crate::wsa::WsaFile::parse(data).is_ok()
}

/// VOC: strong 20-byte magic "Creative Voice File\x1a".
fn is_voc(data: &[u8]) -> bool {
    data.len() >= 26 && data.get(..20) == Some(b"Creative Voice File\x1a")
}

/// DDS: strong 4-byte magic "DDS " followed by header size 124.
fn is_dds(data: &[u8]) -> bool {
    if data.len() < 128 {
        return false;
    }
    if data.get(..4) != Some(b"DDS ") {
        return false;
    }
    // Header size at offset 4 must be 124.
    read_u32_le(data, 4).ok() == Some(124)
}

/// APT: strong 4-byte magic "Apt\0".
fn is_apt(data: &[u8]) -> bool {
    data.len() >= 8 && data.get(..4) == Some(b"Apt\0")
}

/// SAGE map: strong 4-byte magic "CkMp".
fn is_map_sage(data: &[u8]) -> bool {
    data.len() >= 4 && data.get(..4) == Some(b"CkMp")
}

/// JPEG: strong 3-byte magic `FF D8 FF`.
fn is_jpg(data: &[u8]) -> bool {
    data.len() >= 3 && data.get(..3) == Some(&[0xFF, 0xD8, 0xFF])
}

/// TGA 2.0: strong footer signature "TRUEVISION-XFILE.\0" in last 18 bytes.
fn is_tga_footer(data: &[u8]) -> bool {
    data.len() >= 18 + 26 && data.get(data.len() - 18..) == Some(b"TRUEVISION-XFILE.\0")
}

/// TGA: heuristic — valid image type + reasonable dimensions (no footer).
fn is_tga(data: &[u8]) -> bool {
    if data.len() < 18 {
        return false;
    }
    let image_type = match data.get(2) {
        Some(&t) => t,
        None => return false,
    };
    matches!(image_type, 1 | 2 | 3 | 9 | 10 | 11)
        && read_u16_le(data, 12)
            .ok()
            .is_some_and(|w| w > 0 && w <= 16384)
        && read_u16_le(data, 14)
            .ok()
            .is_some_and(|h| h > 0 && h <= 16384)
}

#[cfg(test)]
mod tests;
