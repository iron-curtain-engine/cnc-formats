// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;
use crate::error::Error;

/// Helper: builds a valid SAGE binary map from a list of (name, version, data)
/// chunk descriptors.
fn build_sage_map(chunks: &[(&str, u32, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"CkMp");
    for (name, version, data) in chunks {
        out.extend_from_slice(&(name.len() as u32).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
    }
    out
}

// ── Happy-path tests ────────────────────────────────────────────────────────

/// Two chunks with different names, versions, and payloads parse correctly.
#[test]
fn parse_valid_map() {
    let payload_a = b"hello";
    let payload_b = [0xDE, 0xAD, 0xBE, 0xEF];
    let data = build_sage_map(&[
        ("HeightMapData", 1, payload_a),
        ("WorldInfo", 2, &payload_b),
    ]);
    let map = MapSageFile::parse(&data).unwrap();

    assert_eq!(map.chunk_count(), 2);

    let c0 = &map.chunks()[0];
    assert_eq!(c0.name, "HeightMapData");
    assert_eq!(c0.version, 1);
    assert_eq!(c0.data, b"hello");

    let c1 = &map.chunks()[1];
    assert_eq!(c1.name, "WorldInfo");
    assert_eq!(c1.version, 2);
    assert_eq!(c1.data, &[0xDE, 0xAD, 0xBE, 0xEF]);
}

/// A single HeightMapData chunk parses correctly.
#[test]
fn parse_single_chunk() {
    let payload = [1, 2, 3, 4, 5, 6, 7, 8];
    let data = build_sage_map(&[("HeightMapData", 5, &payload)]);
    let map = MapSageFile::parse(&data).unwrap();

    assert_eq!(map.chunk_count(), 1);
    let c = &map.chunks()[0];
    assert_eq!(c.name, "HeightMapData");
    assert_eq!(c.version, 5);
    assert_eq!(c.data, &[1, 2, 3, 4, 5, 6, 7, 8]);
}

/// Chunks with zero-length data are valid.
#[test]
fn parse_empty_chunks() {
    let data = build_sage_map(&[("BlendTileData", 0, &[]), ("FogSettings", 3, &[])]);
    let map = MapSageFile::parse(&data).unwrap();

    assert_eq!(map.chunk_count(), 2);
    assert_eq!(map.chunks()[0].name, "BlendTileData");
    assert!(map.chunks()[0].data.is_empty());
    assert_eq!(map.chunks()[1].name, "FogSettings");
    assert!(map.chunks()[1].data.is_empty());
}

/// `chunk()` finds the first chunk by name.
#[test]
fn chunk_lookup() {
    let data = build_sage_map(&[
        ("WorldInfo", 1, &[0x01]),
        ("MPPositionList", 2, &[0x02]),
        ("WorldInfo", 3, &[0x03]),
    ]);
    let map = MapSageFile::parse(&data).unwrap();

    let found = map.chunk("MPPositionList").unwrap();
    assert_eq!(found.version, 2);
    assert_eq!(found.data, &[0x02]);

    // Should return the first WorldInfo, not the second.
    let first_wi = map.chunk("WorldInfo").unwrap();
    assert_eq!(first_wi.version, 1);
}

/// `chunks_by_name()` collects all matching chunks.
#[test]
fn chunks_by_name() {
    let data = build_sage_map(&[
        ("Teams", 1, &[0xAA]),
        ("ObjectsList", 2, &[0xBB]),
        ("Teams", 3, &[0xCC]),
        ("Teams", 4, &[0xDD]),
    ]);
    let map = MapSageFile::parse(&data).unwrap();

    let teams = map.chunks_by_name("Teams");
    assert_eq!(teams.len(), 3);
    assert_eq!(teams[0].version, 1);
    assert_eq!(teams[1].version, 3);
    assert_eq!(teams[2].version, 4);

    let objects = map.chunks_by_name("ObjectsList");
    assert_eq!(objects.len(), 1);
}

/// Looking up a nonexistent chunk returns `None`.
#[test]
fn chunk_not_found() {
    let data = build_sage_map(&[("WorldInfo", 1, &[0x00])]);
    let map = MapSageFile::parse(&data).unwrap();

    assert!(map.chunk("NonExistent").is_none());
    assert!(map.chunks_by_name("NonExistent").is_empty());
}

/// Magic-only file (no chunks) is valid and yields zero chunks.
#[test]
fn parse_magic_only() {
    let data = b"CkMp";
    let map = MapSageFile::parse(data).unwrap();

    assert_eq!(map.chunk_count(), 0);
    assert!(map.chunks().is_empty());
}

// ── Error-path tests ────────────────────────────────────────────────────────

/// Wrong first 4 bytes are rejected with `InvalidMagic`.
#[test]
fn reject_invalid_magic() {
    let err = MapSageFile::parse(b"NOPE").unwrap_err();
    assert_eq!(
        err,
        Error::InvalidMagic {
            context: "SAGE map magic (expected 'CkMp' or 'EAR\\0')",
        }
    );
}

/// Truncated chunk header (name_length field cut short) is rejected.
#[test]
fn reject_truncated_chunk_header() {
    // Magic + only 2 bytes of what should be a 4-byte name_length field.
    let data = [b'C', b'k', b'M', b'p', 0x05, 0x00];
    let err = MapSageFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// Chunk whose data_size exceeds remaining bytes is rejected.
#[test]
fn reject_chunk_data_overflow() {
    let mut data = Vec::new();
    data.extend_from_slice(b"CkMp");
    // name_length = 4
    data.extend_from_slice(&4u32.to_le_bytes());
    data.extend_from_slice(b"Test");
    // version = 1
    data.extend_from_slice(&1u32.to_le_bytes());
    // data_size = 1000 (way more than remaining bytes)
    data.extend_from_slice(&1000u32.to_le_bytes());
    // Only 2 bytes of actual data
    data.extend_from_slice(&[0xAA, 0xBB]);

    let err = MapSageFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidOffset { .. }));
}

/// Chunk name length exceeding MAX_CHUNK_NAME is rejected.
#[test]
fn reject_chunk_name_too_long() {
    let mut data = Vec::new();
    data.extend_from_slice(b"CkMp");
    // name_length = 300 (exceeds MAX_CHUNK_NAME of 256)
    data.extend_from_slice(&300u32.to_le_bytes());
    // Pad with enough bytes to avoid UnexpectedEof before the size check.
    data.extend_from_slice(&vec![0x41; 300]);
    data.extend_from_slice(&1u32.to_le_bytes()); // version
    data.extend_from_slice(&0u32.to_le_bytes()); // data_size

    let err = MapSageFile::parse(&data).unwrap_err();
    assert_eq!(
        err,
        Error::InvalidSize {
            value: 300,
            limit: MAX_CHUNK_NAME,
            context: "SAGE map chunk name",
        }
    );
}

// ── Adversarial tests ───────────────────────────────────────────────────────

/// All-0xFF input must not panic (returns an error).
#[test]
fn adversarial_all_ff() {
    let data = [0xFF; 256];
    // Must not panic; the exact error variant does not matter.
    let _ = MapSageFile::parse(&data);
}

/// All-zero input must not panic (returns an error because magic is wrong).
#[test]
fn adversarial_all_zero() {
    let data = [0x00; 256];
    let err = MapSageFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidMagic {
            context: "SAGE map magic (expected 'CkMp' or 'EAR\\0')",
        }
    ));
}
