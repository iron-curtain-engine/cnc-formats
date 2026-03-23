// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;
use crate::error::Error;

// ── Test helpers ─────────────────────────────────────────────────────────────

/// Builds a raw MPR byte buffer from section name + key-value pairs.
fn build_mpr(sections: &[(&str, &[(&str, &str)])]) -> Vec<u8> {
    let mut out = String::new();
    for (section_name, entries) in sections {
        out.push('[');
        out.push_str(section_name);
        out.push_str("]\n");
        for (key, value) in *entries {
            out.push_str(key);
            out.push('=');
            out.push_str(value);
            out.push('\n');
        }
        out.push('\n');
    }
    out.into_bytes()
}

// ── Core parsing ─────────────────────────────────────────────────────────────

/// Parses a complete MPR with Basic, Map, and MapPack sections.
///
/// Why: happy-path baseline for the full MPR structure.
#[test]
fn parse_valid_mpr() {
    let data = build_mpr(&[
        ("Basic", &[("Name", "Test Mission"), ("Intro", "BMAP")]),
        (
            "Map",
            &[
                ("X", "1"),
                ("Y", "1"),
                ("Width", "62"),
                ("Height", "62"),
                ("Theater", "TEMPERATE"),
            ],
        ),
        ("MapPack", &[("1", "AAAA"), ("2", "BBBB")]),
        ("Terrain", &[("100", "T08")]),
    ]);

    let mpr = MprFile::parse(&data).unwrap();
    assert_eq!(mpr.name(), Some("Test Mission"));
    assert_eq!(mpr.theater(), Some("TEMPERATE"));
    assert!(mpr.has_section("Terrain"));
    assert!(mpr.has_section("MapPack"));
}

// ── name() and theater() ─────────────────────────────────────────────────────

/// Verifies name() and theater() return the correct values.
///
/// Why: these are the most frequently queried fields in map loading.
#[test]
fn name_and_theater() {
    let data = build_mpr(&[
        ("Basic", &[("Name", "SCG01EA")]),
        ("Map", &[("Theater", "SNOW")]),
    ]);
    let mpr = MprFile::parse(&data).unwrap();
    assert_eq!(mpr.name(), Some("SCG01EA"));
    assert_eq!(mpr.theater(), Some("SNOW"));
}

// ── bounds() ─────────────────────────────────────────────────────────────────

/// Verifies bounds() returns correct MapBounds values.
///
/// Why: bounds are essential for determining the playable map area.
#[test]
fn bounds_parsing() {
    let data = build_mpr(&[(
        "Map",
        &[
            ("X", "3"),
            ("Y", "5"),
            ("Width", "50"),
            ("Height", "40"),
            ("Theater", "DESERT"),
        ],
    )]);
    let mpr = MprFile::parse(&data).unwrap();
    let bounds = mpr.bounds().unwrap();
    assert_eq!(bounds.x, 3);
    assert_eq!(bounds.y, 5);
    assert_eq!(bounds.width, 50);
    assert_eq!(bounds.height, 40);
}

/// Bounds with zero origin are valid.
///
/// Why: some maps start at (0,0); the parser must not treat zero as missing.
#[test]
fn bounds_zero_origin() {
    let data = build_mpr(&[(
        "Map",
        &[("X", "0"), ("Y", "0"), ("Width", "128"), ("Height", "128")],
    )]);
    let mpr = MprFile::parse(&data).unwrap();
    let bounds = mpr.bounds().unwrap();
    assert_eq!(bounds.x, 0);
    assert_eq!(bounds.y, 0);
}

// ── MapPack / OverlayPack concatenation ──────────────────────────────────────

/// Multiple MapPack lines are concatenated in numeric key order.
///
/// Why: the game reassembles base64 chunks by key number; out-of-order
/// concatenation would produce corrupt decoded data.
#[test]
fn map_pack_concatenation() {
    let data = build_mpr(&[("MapPack", &[("1", "AAAA"), ("2", "BBBB"), ("3", "CCCC")])]);
    let mpr = MprFile::parse(&data).unwrap();
    assert_eq!(mpr.map_pack_raw(), Some("AAAABBBBCCCC".to_string()));
}

/// MapPack lines are sorted numerically, not lexicographically.
///
/// Why: key "10" must come after "9", not after "1".
#[test]
fn map_pack_numeric_sort() {
    let data = build_mpr(&[("MapPack", &[("10", "CC"), ("2", "BB"), ("1", "AA")])]);
    let mpr = MprFile::parse(&data).unwrap();
    assert_eq!(mpr.map_pack_raw(), Some("AABBCC".to_string()));
}

/// OverlayPack lines are concatenated the same way as MapPack.
///
/// Why: OverlayPack uses the same numbered-key format.
#[test]
fn overlay_pack_concatenation() {
    let data = build_mpr(&[("OverlayPack", &[("1", "XXXX"), ("2", "YYYY")])]);
    let mpr = MprFile::parse(&data).unwrap();
    assert_eq!(mpr.overlay_pack_raw(), Some("XXXXYYYY".to_string()));
}

// ── Missing sections ─────────────────────────────────────────────────────────

/// name(), theater(), and bounds() return None for missing data.
///
/// Why: callers must be able to safely query incomplete MPR files
/// without panicking.
#[test]
fn missing_sections() {
    let data = build_mpr(&[("Terrain", &[("100", "T08")])]);
    let mpr = MprFile::parse(&data).unwrap();
    assert_eq!(mpr.name(), None);
    assert_eq!(mpr.theater(), None);
    assert_eq!(mpr.bounds(), None);
    assert_eq!(mpr.map_pack_raw(), None);
    assert_eq!(mpr.overlay_pack_raw(), None);
}

/// bounds() returns None when a required key is missing.
///
/// Why: partial [Map] sections (e.g. Width present but Height missing)
/// must not produce bogus bounds.
#[test]
fn bounds_partial_map_section() {
    let data = build_mpr(&[("Map", &[("X", "1"), ("Y", "1"), ("Width", "62")])]);
    let mpr = MprFile::parse(&data).unwrap();
    assert_eq!(mpr.bounds(), None);
}

/// bounds() returns None when a value is not a valid integer.
///
/// Why: non-numeric values in coordinate fields must be handled gracefully.
#[test]
fn bounds_non_numeric_value() {
    let data = build_mpr(&[(
        "Map",
        &[("X", "abc"), ("Y", "1"), ("Width", "62"), ("Height", "62")],
    )]);
    let mpr = MprFile::parse(&data).unwrap();
    assert_eq!(mpr.bounds(), None);
}

// ── has_section ──────────────────────────────────────────────────────────────

/// Verifies section existence checks for both present and absent sections.
///
/// Why: has_section is the primary way callers detect optional game data
/// sections like Terrain, Units, Infantry, etc.
#[test]
fn has_section() {
    let data = build_mpr(&[
        ("Terrain", &[("100", "T08")]),
        ("Units", &[("0", "GoodGuy,E1,50,50,128,Guard,None")]),
        ("Waypoints", &[("0", "3838")]),
    ]);
    let mpr = MprFile::parse(&data).unwrap();
    assert!(mpr.has_section("Terrain"));
    assert!(mpr.has_section("Units"));
    assert!(mpr.has_section("Waypoints"));
    assert!(!mpr.has_section("Infantry"));
    assert!(!mpr.has_section("Structures"));
}

/// has_section is case-insensitive (inherited from IniFile).
///
/// Why: the underlying INI parser is case-insensitive; MPR should
/// preserve that behaviour.
#[test]
fn has_section_case_insensitive() {
    let data = build_mpr(&[("Terrain", &[("100", "T08")])]);
    let mpr = MprFile::parse(&data).unwrap();
    assert!(mpr.has_section("terrain"));
    assert!(mpr.has_section("TERRAIN"));
}

// ── Empty and edge cases ─────────────────────────────────────────────────────

/// Empty input parses as an empty INI with no sections.
///
/// Why: degenerate input must not cause errors.
#[test]
fn parse_empty() {
    let mpr = MprFile::parse(b"").unwrap();
    assert_eq!(mpr.name(), None);
    assert_eq!(mpr.theater(), None);
    assert_eq!(mpr.bounds(), None);
    assert!(!mpr.has_section("Basic"));
}

/// Input exceeding MAX_INPUT_SIZE is rejected.
///
/// Why: prevents unbounded memory allocation from maliciously large files.
#[test]
fn reject_too_large() {
    let data = vec![b' '; MAX_INPUT_SIZE + 1];
    let err = MprFile::parse(&data).unwrap_err();
    match err {
        Error::InvalidSize {
            value,
            limit,
            context,
        } => {
            assert_eq!(value, MAX_INPUT_SIZE + 1);
            assert_eq!(limit, MAX_INPUT_SIZE);
            assert_eq!(context, "MPR file");
        }
        other => panic!("Expected InvalidSize, got {other:?}"),
    }
}

/// ini() accessor provides full INI access for non-standard sections.
///
/// Why: game-specific or modded sections that MprFile does not have
/// dedicated accessors for must remain reachable through the raw INI.
#[test]
fn ini_accessor() {
    let data = build_mpr(&[
        ("Basic", &[("Name", "test")]),
        ("CellTriggers", &[("3838", "trg1")]),
    ]);
    let mpr = MprFile::parse(&data).unwrap();
    assert_eq!(mpr.ini().get("CellTriggers", "3838"), Some("trg1"));
    assert_eq!(mpr.ini().section_count(), 2);
}

/// All-0xFF input does not panic; fails to parse as valid INI text.
#[test]
fn adversarial_all_ff() {
    let data = vec![0xFF_u8; 64];
    assert!(MprFile::parse(&data).is_err());
}

/// All-zero input does not panic; parses as empty map.
#[test]
fn adversarial_all_zero() {
    let data = vec![0u8; 64];
    // NUL bytes are treated as empty text; parse succeeds with no useful data.
    let mpr = MprFile::parse(&data).unwrap();
    assert!(mpr.name().is_none());
}
