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
    if is_mix(data) {
        return Some("mix");
    }
    if is_ini(data) {
        return Some("ini");
    }

    // ── Formats with structural signatures (good reliability) ───────
    if is_aud(data) {
        return Some("aud");
    }
    if is_fnt(data) {
        return Some("fnt");
    }

    // ── Exact-size format ───────────────────────────────────────────
    if is_pal(data) {
        return Some("pal");
    }

    // ── Heuristic formats (try parser, least reliable) ──────────────
    if is_shp(data) {
        return Some("shp");
    }
    if is_wsa(data) {
        return Some("wsa");
    }

    None
}

/// VQA: IFF container with `FORM` + `WVQA` magic.
fn is_vqa(data: &[u8]) -> bool {
    data.len() >= 12 && data.get(..4) == Some(b"FORM") && data.get(8..12) == Some(b"WVQA")
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
    let w0 = u16::from_le_bytes([data[0], data[1]]);
    if w0 == 0 {
        // Extended format marker — flags must be 0x0001..0x0003.
        // (Flags 0x0000 with marker 0x0000 is just 4 zero bytes, too ambiguous.)
        // Encrypted archives (flag 0x0002) must be >= 90 bytes (4 + 80 key + header).
        if data.len() >= 90 {
            let flags = u16::from_le_bytes([data[2], data[3]]);
            return (1..=0x0003).contains(&flags);
        }
        return false;
    }
    // Basic format: count is non-zero; heuristic check.
    let count = w0 as usize;
    let data_size = u32::from_le_bytes([data[2], data[3], data[4], data[5]]) as usize;
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

/// AUD: 12-byte header with plausible fields.
///
/// Checks: sample_rate 1..65535, compression ID is 1 (WS ADPCM) or 99 (IMA),
/// and compressed_size + 12 ≈ file size.
fn is_aud(data: &[u8]) -> bool {
    if data.len() < 12 {
        return false;
    }
    let sample_rate = u16::from_le_bytes([data[0], data[1]]);
    if sample_rate == 0 {
        return false;
    }
    let compressed_size = u32::from_le_bytes([data[2], data[3], data[4], data[5]]) as usize;
    let compression = data[11];
    // Known Westwood compression IDs: 1 (WS ADPCM) or 99 (IMA ADPCM).
    if compression != 1 && compression != 99 {
        return false;
    }
    // compressed_size + 12 should be close to file size.
    let expected = compressed_size.saturating_add(12);
    expected <= data.len().saturating_add(64) && expected >= data.len().saturating_sub(64)
}

/// FNT: 20-byte header with `data_blocks == 5` at offset 6.
fn is_fnt(data: &[u8]) -> bool {
    if data.len() < 20 {
        return false;
    }
    let data_blocks = u16::from_le_bytes([data[6], data[7]]);
    let compress = u16::from_le_bytes([data[8], data[9]]);
    data_blocks == 5 && compress == 0
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
///
/// Checks: frame_count > 0, width/height > 0 and ≤ 640,
/// offset table fits within file, first frame offset has valid format code.
fn is_shp(data: &[u8]) -> bool {
    if data.len() < 30 {
        return false;
    }
    let frame_count = u16::from_le_bytes([data[0], data[1]]) as usize;
    if frame_count == 0 || frame_count > 10000 {
        return false;
    }
    let width = u16::from_le_bytes([data[6], data[7]]);
    let height = u16::from_le_bytes([data[8], data[9]]);
    if width == 0 || height == 0 || width > 640 || height > 480 {
        return false;
    }
    // Offset table: (frame_count + 2) entries × 8 bytes, starting at byte 14.
    let offset_table_end = 14usize.saturating_add((frame_count + 2).saturating_mul(8));
    if offset_table_end > data.len() {
        return false;
    }
    // First frame offset entry: high byte should be a valid format code.
    let first_offset_raw = u32::from_le_bytes([data[14], data[15], data[16], data[17]]);
    let format_code = (first_offset_raw >> 24) & 0xFF;
    // Valid format codes: 0x80 (LCW), 0x40 (XorLcw), 0x20 (XorPrev).
    matches!(format_code, 0x80 | 0x40 | 0x20)
}

/// WSA: 14-byte header with plausible frame count, dimensions, and offset table.
fn is_wsa(data: &[u8]) -> bool {
    if data.len() < 22 {
        return false;
    }
    let frame_count = u16::from_le_bytes([data[0], data[1]]) as usize;
    if frame_count == 0 || frame_count > 8192 {
        return false;
    }
    let width = u16::from_le_bytes([data[6], data[7]]);
    let height = u16::from_le_bytes([data[8], data[9]]);
    if width == 0 || height == 0 || width > 640 || height > 480 {
        return false;
    }
    // Offset table: (frame_count + 2) × 4 bytes, starting at byte 14.
    let table_end = 14usize.saturating_add((frame_count + 2).saturating_mul(4));
    if table_end > data.len() {
        return false;
    }
    // First offset should point somewhere after the header + offset table.
    let first_offset = u32::from_le_bytes([data[14], data[15], data[16], data[17]]) as usize;
    first_offset >= table_end || first_offset == 0
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

    /// PAL files are detected by exact size (768) and value range (0–63).
    #[test]
    fn sniff_pal() {
        let data = vec![32u8; 768];
        assert_eq!(sniff_format(&data), Some("pal"));
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
