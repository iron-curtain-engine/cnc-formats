// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

//! XCC "local mix database" (LMD) reader.
//!
//! XCC Mixer embeds a filename database as a regular entry inside MIX
//! archives.  The entry uses the well-known CRC of `"local mix database.dat"`
//! (`0x54C2_D545`).  The game engine ignores it (unrecognised CRC), but
//! tools that know the convention can read it to recover original filenames.
//!
//! ## Binary Format
//!
//! ```text
//! count:   u32 LE          — number of name entries
//! entries: count x {
//!     name:        NUL-terminated ASCII string
//!     description: NUL-terminated ASCII string (ignored by us)
//! }
//! ```
//!
//! This is a community convention (XCC Utilities, Olaf van der Spek),
//! not an official EA/Westwood format.

use super::{crc, MixCrc};
use std::collections::HashMap;

/// CRC of `"local mix database.dat"` — the well-known key for XCC's
/// embedded filename database.
pub const LMD_CRC: MixCrc = MixCrc(0x54C2_D545);

/// Parse an XCC local mix database blob into a CRC-to-filename map.
///
/// `data` is the raw bytes of the LMD entry (the file content, not the
/// whole archive).  Returns a map from Westwood CRC to filename string.
///
/// Tolerant of truncated or malformed data — stops parsing on any error
/// and returns whatever names were successfully read.
pub fn parse_lmd(data: &[u8]) -> HashMap<MixCrc, String> {
    let mut map = HashMap::new();
    if data.len() < 4 {
        return map;
    }
    let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    // V38: cap at 131,072 to match MAX_MIX_ENTRIES.
    let count = count.min(super::MAX_MIX_ENTRIES);

    let mut pos = 4usize;
    for _ in 0..count {
        // Read name (NUL-terminated).
        let name = match read_nul_string(data, pos) {
            Some((s, next)) => {
                pos = next;
                s
            }
            None => break,
        };
        // Read description (NUL-terminated) — we skip it.
        match read_nul_string(data, pos) {
            Some((_, next)) => pos = next,
            None => break,
        }
        if !name.is_empty() {
            map.insert(crc(&name), name);
        }
    }
    map
}

/// Read a NUL-terminated string starting at `pos`.
///
/// Returns `(string, next_pos)` where `next_pos` is the byte after the NUL.
/// Returns `None` if no NUL terminator is found before end of data.
fn read_nul_string(data: &[u8], pos: usize) -> Option<(String, usize)> {
    let remaining = data.get(pos..)?;
    let nul_idx = remaining.iter().position(|&b| b == 0)?;
    let s = String::from_utf8_lossy(remaining.get(..nul_idx).unwrap_or(&[])).into_owned();
    Some((s, pos + nul_idx + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Empty data returns an empty map.
    #[test]
    fn parse_lmd_empty() {
        assert!(parse_lmd(&[]).is_empty());
    }

    /// Minimal LMD with one entry.
    ///
    /// Why: verifies the basic count + name + description parsing.
    #[test]
    fn parse_lmd_single_entry() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_le_bytes()); // count = 1
        data.extend_from_slice(b"RULES.INI\0"); // name
        data.extend_from_slice(b"Game rules\0"); // description
        let map = parse_lmd(&data);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&crc("RULES.INI")), Some(&"RULES.INI".to_string()));
    }

    /// Multiple entries are all resolved.
    #[test]
    fn parse_lmd_multiple_entries() {
        let mut data = Vec::new();
        data.extend_from_slice(&3u32.to_le_bytes());
        for (name, desc) in [
            ("CONQUER.SHP", "sprites"),
            ("TEMPERAT.PAL", "palette"),
            ("SPEECH.AUD", "audio"),
        ] {
            data.extend_from_slice(name.as_bytes());
            data.push(0);
            data.extend_from_slice(desc.as_bytes());
            data.push(0);
        }
        let map = parse_lmd(&data);
        assert_eq!(map.len(), 3);
        assert!(map.contains_key(&crc("CONQUER.SHP")));
        assert!(map.contains_key(&crc("TEMPERAT.PAL")));
        assert!(map.contains_key(&crc("SPEECH.AUD")));
    }

    /// Truncated data stops gracefully without panic.
    #[test]
    fn parse_lmd_truncated() {
        let mut data = Vec::new();
        data.extend_from_slice(&5u32.to_le_bytes()); // claims 5 entries
        data.extend_from_slice(b"FIRST.DAT\0DESC\0"); // only 1 complete entry
                                                      // no more data — should stop after first entry
        let map = parse_lmd(&data);
        assert_eq!(map.len(), 1);
    }

    /// The well-known LMD CRC matches our CRC function.
    #[test]
    fn lmd_crc_matches() {
        assert_eq!(LMD_CRC, crc("local mix database.dat"));
    }
}
