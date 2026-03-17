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
    if is_shp(data) {
        return Some("shp");
    }
    if is_wsa(data) {
        return Some("wsa");
    }
    if is_aud(data) {
        return Some("aud");
    }

    None
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

#[cfg(test)]
mod tests {
    use super::*;

    /// VQA files are detected by FORM/WVQA magic.
    #[test]
    fn sniff_vqa() {
        let mut data = vec![0u8; 64];
        data[..4].copy_from_slice(b"FORM");
        data[8..12].copy_from_slice(b"WVQA");
        assert_eq!(sniff_format(&data), Some("vqa"));
    }

    /// BIG archives are detected by BIGF/BIG4 magic.
    #[test]
    fn sniff_big() {
        let mut data = vec![0u8; 32];
        data[..4].copy_from_slice(b"BIGF");
        assert_eq!(sniff_format(&data), Some("big"));
    }

    /// PAL files are detected by exact size (768) and value range (0–63).
    #[test]
    fn sniff_pal() {
        let data = vec![32u8; 768];
        assert_eq!(sniff_format(&data), Some("pal"));
    }

    /// ENG-family string tables are detected from their offset table layout.
    #[test]
    fn sniff_eng() {
        let data = [6u8, 0, 6, 0, 7, 0, 0, b'A', 0];
        assert_eq!(sniff_format(&data), Some("eng"));
    }

    /// Segmented DIP files are detected as installer data rather than generic blobs.
    #[test]
    fn sniff_segmented_dip() {
        let data = [
            0x02, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00,
            0x3C, 0x3C, 0x01, 0x80, 0x00, 0x00, 0x0B, 0x80,
        ];
        assert_eq!(sniff_format(&data), Some("dip"));
    }

    /// Chrono Vortex LUT files are detected by their exact size and bounds.
    #[test]
    fn sniff_lut() {
        let mut data = Vec::with_capacity(crate::lut::LUT_FILE_SIZE);
        for i in 0..crate::lut::LUT_ENTRY_COUNT {
            data.push((i % 64) as u8);
            data.push(((i / 64) % 64) as u8);
            data.push(((i / 256) % 16) as u8);
        }
        assert_eq!(sniff_format(&data), Some("lut"));
    }

    /// VQP files are detected by exact packed-table size.
    #[test]
    fn sniff_vqp() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_le_bytes());
        data.resize(4 + crate::vqp::VQP_TABLE_SIZE, 0);
        assert_eq!(sniff_format(&data), Some("vqp"));
    }

    /// PAL with out-of-range values (>63) is not detected.
    #[test]
    fn sniff_pal_out_of_range_rejected() {
        let mut data = vec![32u8; 768];
        data[0] = 200;
        assert_ne!(sniff_format(&data), Some("pal"));
    }

    /// INI text is detected by ASCII content and `[` bracket.
    #[test]
    fn sniff_ini() {
        let data = b"[General]\nSpeed=5\n";
        assert_eq!(sniff_format(data), Some("ini"));
    }

    /// Binary data without brackets is not detected as INI.
    #[test]
    fn sniff_not_ini() {
        let data = vec![0xFFu8; 100];
        assert_ne!(sniff_format(&data), Some("ini"));
    }

    /// Empty data returns None.
    #[test]
    fn sniff_empty() {
        assert_eq!(sniff_format(&[]), None);
    }

    /// Very short data returns None (not a panic).
    #[test]
    fn sniff_short() {
        assert_eq!(sniff_format(&[0x42]), None);
    }

    /// SHP detection uses structural heuristics.
    #[cfg(feature = "convert")]
    #[test]
    fn sniff_shp_from_parsed_file() {
        // Build a minimal valid SHP: 1 frame, 2×2 pixels, LCW-compressed.
        let shp_bytes = crate::shp::build_test_shp_helper(2, 2, 0xAA);
        assert_eq!(sniff_format(&shp_bytes), Some("shp"));
    }
}
