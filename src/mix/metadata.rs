// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

//! CNFM trailing metadata block for MIX archives.
//!
//! This is a cnc-formats extension that stores filename and format-type
//! metadata **after** the MIX data section, in a region the original game
//! engine never reads.  It is invisible to the game and to older tools.
//!
//! ## Binary Format
//!
//! The block is appended after the last byte the MIX parser consumes
//! (after the data section, or after the SHA-1 digest if present):
//!
//! ```text
//! magic:   [u8; 4]     "CNFM"
//! version: u16 LE      schema version (currently 1)
//! count:   u16 LE      number of entries
//! entries: count x {
//!     crc:       u32 LE   Westwood MIX CRC
//!     type_hint: u8       sniffed format (0=unknown, see TYPE_* constants)
//!     name_len:  u16 LE   filename length in bytes
//!     name:      [u8]     UTF-8 filename (NOT NUL-terminated)
//! }
//! ```
//!
//! ## Compatibility
//!
//! The original C&C game engine reads `header + index + data_size` bytes
//! and ignores everything after.  The CNFM block lives entirely in that
//! trailing region, so it cannot break any game or existing tool.

use super::MixCrc;
use crate::read::{read_u16_le, read_u32_le, read_u8};
use std::collections::HashMap;

/// Magic bytes identifying a CNFM metadata block.
pub const CNFM_MAGIC: &[u8; 4] = b"CNFM";

/// Current schema version.
pub const CNFM_VERSION: u16 = 1;

// ── Type hint constants ─────────────────────────────────────────────────

/// Unknown or undetected format.
pub const TYPE_UNKNOWN: u8 = 0;
/// SHP sprite.
pub const TYPE_SHP: u8 = 1;
/// PAL palette.
pub const TYPE_PAL: u8 = 2;
/// AUD audio.
pub const TYPE_AUD: u8 = 3;
/// VQA video.
pub const TYPE_VQA: u8 = 4;
/// WSA animation.
pub const TYPE_WSA: u8 = 5;
/// TMP terrain tile.
pub const TYPE_TMP: u8 = 6;
/// FNT bitmap font.
pub const TYPE_FNT: u8 = 7;
/// INI configuration.
pub const TYPE_INI: u8 = 8;
/// MIX archive (nested).
pub const TYPE_MIX: u8 = 9;

/// One entry in the CNFM metadata block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CnfmEntry {
    /// Westwood MIX CRC of the filename.
    pub crc: MixCrc,
    /// Detected format type (see `TYPE_*` constants).
    pub type_hint: u8,
    /// Original filename.
    pub name: String,
}

/// Convert a sniff extension string to a type hint constant.
pub fn ext_to_type_hint(ext: &str) -> u8 {
    match ext {
        "shp" => TYPE_SHP,
        "pal" => TYPE_PAL,
        "aud" => TYPE_AUD,
        "vqa" => TYPE_VQA,
        "wsa" => TYPE_WSA,
        "tmp" => TYPE_TMP,
        "fnt" => TYPE_FNT,
        "ini" => TYPE_INI,
        "mix" => TYPE_MIX,
        _ => TYPE_UNKNOWN,
    }
}

/// Convert a type hint constant to a file extension string.
pub fn type_hint_to_ext(hint: u8) -> &'static str {
    match hint {
        TYPE_SHP => "shp",
        TYPE_PAL => "pal",
        TYPE_AUD => "aud",
        TYPE_VQA => "vqa",
        TYPE_WSA => "wsa",
        TYPE_TMP => "tmp",
        TYPE_FNT => "fnt",
        TYPE_INI => "ini",
        TYPE_MIX => "mix",
        _ => "bin",
    }
}

/// Encode a CNFM metadata block to bytes.
///
/// The returned bytes can be appended directly to a MIX file after the
/// data section (and optional SHA-1 digest).
pub fn encode_cnfm(entries: &[CnfmEntry]) -> Vec<u8> {
    let count = entries.len().min(u16::MAX as usize);
    let mut buf = Vec::new();
    buf.extend_from_slice(CNFM_MAGIC);
    buf.extend_from_slice(&CNFM_VERSION.to_le_bytes());
    buf.extend_from_slice(&(count as u16).to_le_bytes());
    for entry in entries.iter().take(count) {
        buf.extend_from_slice(&entry.crc.to_raw().to_le_bytes());
        buf.push(entry.type_hint);
        let name_bytes = entry.name.as_bytes();
        let name_len = name_bytes.len().min(u16::MAX as usize);
        buf.extend_from_slice(&(name_len as u16).to_le_bytes());
        buf.extend_from_slice(name_bytes.get(..name_len).unwrap_or(name_bytes));
    }
    buf
}

/// Try to parse a CNFM metadata block from trailing bytes.
///
/// `trailing` should be the bytes after the MIX data section (and
/// optional SHA-1 digest).  Returns a CRC-to-entry map if a valid
/// CNFM block is found, or an empty map if not.
pub fn parse_cnfm(trailing: &[u8]) -> HashMap<MixCrc, CnfmEntry> {
    let mut map = HashMap::new();
    if trailing.len() < 8 {
        return map;
    }
    if trailing.get(..4) != Some(CNFM_MAGIC.as_slice()) {
        return map;
    }
    let version = match read_u16_le(trailing, 4) {
        Ok(version) => version,
        Err(_) => return map,
    };
    if version == 0 || version > CNFM_VERSION {
        return map; // Unknown version — don't attempt parsing.
    }
    let count = match read_u16_le(trailing, 6) {
        Ok(count) => count as usize,
        Err(_) => return map,
    };
    let mut pos = 8usize;
    for _ in 0..count {
        if pos + 7 > trailing.len() {
            break;
        }
        let crc = match read_u32_le(trailing, pos) {
            Ok(value) => MixCrc::from_raw(value),
            Err(_) => break,
        };
        let type_hint = match read_u8(trailing, pos + 4) {
            Ok(value) => value,
            Err(_) => break,
        };
        let name_len = match read_u16_le(trailing, pos + 5) {
            Ok(value) => value as usize,
            Err(_) => break,
        };
        pos += 7;
        if pos + name_len > trailing.len() {
            break;
        }
        let name =
            String::from_utf8_lossy(trailing.get(pos..pos + name_len).unwrap_or(&[])).into_owned();
        pos += name_len;
        map.insert(
            crc,
            CnfmEntry {
                crc,
                type_hint,
                name,
            },
        );
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mix::crc;

    /// Encode → parse round-trip preserves all fields.
    ///
    /// Why: the metadata block is useless if encode and parse disagree on
    /// the wire format.
    #[test]
    fn cnfm_round_trip() {
        let entries = vec![
            CnfmEntry {
                crc: crc("RULES.INI"),
                type_hint: TYPE_INI,
                name: "RULES.INI".to_string(),
            },
            CnfmEntry {
                crc: crc("TEMPERAT.PAL"),
                type_hint: TYPE_PAL,
                name: "TEMPERAT.PAL".to_string(),
            },
            CnfmEntry {
                crc: crc("UNITS.SHP"),
                type_hint: TYPE_SHP,
                name: "UNITS.SHP".to_string(),
            },
        ];
        let encoded = encode_cnfm(&entries);
        let parsed = parse_cnfm(&encoded);

        assert_eq!(parsed.len(), 3);
        for e in &entries {
            let got = parsed.get(&e.crc).expect("missing entry");
            assert_eq!(got.name, e.name);
            assert_eq!(got.type_hint, e.type_hint);
        }
    }

    /// Empty entries produce a valid but empty block.
    #[test]
    fn cnfm_empty_round_trip() {
        let encoded = encode_cnfm(&[]);
        assert_eq!(&encoded[..4], b"CNFM");
        let parsed = parse_cnfm(&encoded);
        assert!(parsed.is_empty());
    }

    /// Truncated block returns whatever was parseable.
    #[test]
    fn cnfm_truncated_graceful() {
        let entries = vec![
            CnfmEntry {
                crc: crc("A.BIN"),
                type_hint: TYPE_UNKNOWN,
                name: "A.BIN".to_string(),
            },
            CnfmEntry {
                crc: crc("B.BIN"),
                type_hint: TYPE_SHP,
                name: "B.BIN".to_string(),
            },
        ];
        let encoded = encode_cnfm(&entries);
        // Truncate mid-way through second entry.
        let truncated = &encoded[..encoded.len() - 3];
        let parsed = parse_cnfm(truncated);
        assert_eq!(parsed.len(), 1); // Only first entry survives.
        assert!(parsed.contains_key(&crc("A.BIN")));
    }

    /// Non-CNFM data returns empty map (no panic).
    #[test]
    fn cnfm_garbage_returns_empty() {
        assert!(parse_cnfm(b"NOT_CNFM_DATA").is_empty());
        assert!(parse_cnfm(&[]).is_empty());
        assert!(parse_cnfm(&[0xFF; 100]).is_empty());
    }

    /// Type hint round-trip through ext_to_type_hint / type_hint_to_ext.
    #[test]
    fn type_hint_round_trip() {
        for ext in [
            "shp", "pal", "aud", "vqa", "wsa", "tmp", "fnt", "ini", "mix",
        ] {
            let hint = ext_to_type_hint(ext);
            assert_eq!(type_hint_to_ext(hint), ext);
        }
        // Unknown extensions map to TYPE_UNKNOWN → "bin".
        assert_eq!(ext_to_type_hint("xyz"), TYPE_UNKNOWN);
        assert_eq!(type_hint_to_ext(TYPE_UNKNOWN), "bin");
    }
}
