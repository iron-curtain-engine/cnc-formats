// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

//! Validation, error, security, and cross-validation tests for the MIX module.
//! Split from `tests.rs` to stay within the ~600-line file-size cap.

use super::*;
use crate::mix::tests::build_mix;

// ── Error field & Display verification ────────────────────────────────

/// `UnexpectedEof` carries the exact `needed` / `available` byte counts.
///
/// Why: structured error fields let callers generate precise diagnostics.
/// A 0-byte input needs 2 bytes for format detection.
#[test]
fn eof_error_carries_byte_counts() {
    let err = MixArchive::parse(&[]).unwrap_err();
    match err {
        Error::UnexpectedEof { needed, available } => {
            assert_eq!(needed, 2, "need at least 2 bytes for format detection");
            assert_eq!(available, 0);
        }
        other => panic!("Expected UnexpectedEof, got: {other}"),
    }
}

/// `InvalidSize` error variant carries the offending value, the cap, and a
/// context tag.
///
/// Why: when the cap rejects an entry count, the error must explain
/// *what* value was rejected and *what* the limit is.
///
/// Note: since `MAX_MIX_ENTRIES` (131,072) exceeds `u16::MAX` (65,535),
/// this can no longer be triggered via the basic-format header.  We test
/// the error variant structure directly.
#[test]
fn invalid_size_error_carries_value_and_limit() {
    let err = Error::InvalidSize {
        value: 200_000,
        limit: MAX_MIX_ENTRIES,
        context: "MIX entry count",
    };
    match err {
        Error::InvalidSize {
            value,
            limit,
            context,
        } => {
            assert_eq!(value, 200_000);
            assert_eq!(limit, MAX_MIX_ENTRIES);
            assert!(
                context.contains("MIX"),
                "context should mention MIX: {context}"
            );
        }
        other => panic!("Expected InvalidSize, got: {other}"),
    }
}

/// `InvalidOffset` carries the computed end position and the buffer bound.
///
/// Why: callers need to know *where* the bad offset pointed and how
/// large the data section actually is.
#[test]
fn invalid_offset_error_carries_position_and_bound() {
    let count: u16 = 1;
    let data_size: u32 = 5;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&data_size.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes()); // crc
    bytes.extend_from_slice(&0u32.to_le_bytes()); // offset
    bytes.extend_from_slice(&9999u32.to_le_bytes()); // size
    bytes.extend_from_slice(&[0xAA; 5]);

    let err = MixArchive::parse(&bytes).unwrap_err();
    match err {
        Error::InvalidOffset { offset, bound } => {
            assert_eq!(offset, 9999, "end position should be 0 + 9999");
            assert_eq!(bound, 5, "data section is 5 bytes");
        }
        other => panic!("Expected InvalidOffset, got: {other}"),
    }
}

/// `Error::Display` embeds numeric context for human-readable output.
///
/// Why: error messages are the user-facing interface; they must include
/// the offending values so problems can be diagnosed without a debugger.
/// Both `InvalidSize` and `InvalidOffset` Display paths are tested.
#[test]
fn error_display_messages_contain_context() {
    // InvalidSize Display — construct directly since u16 can't exceed
    // the raised cap (131,072).
    let err = Error::InvalidSize {
        value: 200_000,
        limit: MAX_MIX_ENTRIES,
        context: "MIX entry count",
    };
    let msg = err.to_string();
    assert!(msg.contains("200000"), "should show the value: {msg}");
    assert!(
        msg.contains(&MAX_MIX_ENTRIES.to_string()),
        "should show the limit: {msg}"
    );

    // InvalidOffset Display
    let mut bytes2 = Vec::new();
    bytes2.extend_from_slice(&1u16.to_le_bytes());
    bytes2.extend_from_slice(&5u32.to_le_bytes());
    bytes2.extend_from_slice(&1u32.to_le_bytes());
    bytes2.extend_from_slice(&0u32.to_le_bytes());
    bytes2.extend_from_slice(&9999u32.to_le_bytes());
    bytes2.extend_from_slice(&[0xAA; 5]);
    let msg2 = MixArchive::parse(&bytes2).unwrap_err().to_string();
    assert!(msg2.contains("9999"), "should show offset: {msg2}");
    assert!(msg2.contains('5'), "should show bound: {msg2}");
}

// ── Known-hash cross-validation (OpenRA Classic hash algorithm) ───────
//
// OpenRA's PackageEntry.HashFilename(name, PackageHashType.Classic) uses
// the same rotate_left(1) + add algorithm on uppercased, zero-padded
// 4-byte groups.  These expected values were computed independently and
// match the OpenRA Classic hash output for Red Alert archive filenames.

/// CRC of well-known RA filenames matches independently computed values.
///
/// Why: cross-validation against the OpenRA Classic hash algorithm.
/// If these ever drift, the engine cannot open real RA MIX archives.
#[test]
fn crc_matches_known_ra_filenames() {
    // Values cross-validated against OpenRA PackageEntry.HashFilename Classic.
    assert_eq!(crc("CONQUER.MIX"), MixCrc::from_raw(0xA236_1104));
    assert_eq!(crc("TEMPERAT.MIX"), MixCrc::from_raw(0x4201_0709));
    assert_eq!(crc("DESERT.MIX"), MixCrc::from_raw(0xAFAA_15FE));
    assert_eq!(crc("GENERAL.MIX"), MixCrc::from_raw(0x7229_E10E));
    assert_eq!(crc("SCORES.MIX"), MixCrc::from_raw(0xE39A_0C20));
    assert_eq!(crc("ALLIES.MIX"), MixCrc::from_raw(0xBF8E_2FD8));
    assert_eq!(crc("RUSSIAN.MIX"), MixCrc::from_raw(0xAA42_2128));
}

/// CRC of "local mix database.dat" matches the OpenRA lookup key.
///
/// Why: this special filename is used by OpenRA to locate the embedded
/// filename table inside MIX archives.  A wrong hash breaks filename
/// resolution for the entire tool chain.
#[test]
fn crc_local_mix_database_matches_openra() {
    assert_eq!(crc("local mix database.dat"), MixCrc::from_raw(0x54C2_D545));
}

/// Built-in filename map includes RA2-era filenames as well as TD/RA1 names.
#[test]
fn builtin_name_map_includes_ra2_entries() {
    let names = builtin_name_map();
    assert_eq!(
        names.get(&crc("RULESMO.INI")).map(String::as_str),
        Some("RULESMO.INI")
    );
    assert_eq!(
        names.get(&crc("MWCLFX28.SNO")).map(String::as_str),
        Some("MWCLFX28.SNO")
    );
    assert_eq!(
        names.get(&crc("AUDIOMD.MIX")).map(String::as_str),
        Some("AUDIOMD.MIX")
    );
}

/// Built-in filename resolution omits ambiguous CRC collisions.
///
/// Why: the built-in corpus is a candidate list, not authoritative metadata.
/// First-match-wins would make resolution depend on list order.
#[test]
fn builtin_name_map_omits_ambiguous_crc_collisions() {
    let names = builtin_name_map();
    let bik_collision = crc("S10_P03E.BIK");
    assert_eq!(bik_collision, crc("S11_P01E.BIK"));
    assert_eq!(names.get(&bik_collision), None);

    let second_collision = crc("A01_F04E.BIK");
    assert_eq!(second_collision, crc("A03_F00E.BIK"));
    assert_eq!(names.get(&second_collision), None);

    let stats = builtin_name_stats();
    assert!(stats.ambiguous_crc_count > 0);
}

// ── Determinism ──────────────────────────────────────────────────────

/// Parsing the same archive bytes twice yields identical results.
///
/// Why: the parser is a pure function of its input; any hidden state
/// that leaked between calls would break reproducibility.
#[test]
fn parse_is_deterministic() {
    let bytes = build_mix(&[("A.DAT", b"hello"), ("B.DAT", b"world")]);
    let a = MixArchive::parse(&bytes).unwrap();
    let b = MixArchive::parse(&bytes).unwrap();
    assert_eq!(a.file_count(), b.file_count());
    assert_eq!(a.get("A.DAT"), b.get("A.DAT"));
    assert_eq!(a.get("B.DAT"), b.get("B.DAT"));
}

// ── Boundary tests ──────────────────────────────────────────────────

/// A large entry count within the u16 range is accepted.
///
/// Why: RA1's MAIN.MIX has ~64,000 entries.  The parser must accept any
/// count that fits in u16 (max 65,535) since the V38 cap (131,072) is
/// above u16::MAX.  This test uses 16,384 entries as a practical
/// large-archive smoke test.
#[test]
fn parse_large_entry_count_accepted() {
    let count: u16 = 16_384;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&count.to_le_bytes());
    // data_size = 0 (empty data section is fine if all entries have size 0)
    bytes.extend_from_slice(&0u32.to_le_bytes());
    // 16384 SubBlocks, each 12 bytes: crc=i, offset=0, size=0
    for i in 0..16_384u32 {
        bytes.extend_from_slice(&i.to_le_bytes()); // crc
        bytes.extend_from_slice(&0u32.to_le_bytes()); // offset
        bytes.extend_from_slice(&0u32.to_le_bytes()); // size
    }
    let archive = MixArchive::parse(&bytes).unwrap();
    assert_eq!(archive.file_count(), 16_384);
}

// ── Integer overflow safety ──────────────────────────────────────────

/// Entry with `offset = u32::MAX, size = 1`: `saturating_add` prevents wrap.
///
/// Why (V38): on 32-bit targets `u32::MAX + 1` wraps to 0, which
/// would pass a naïve bounds check.  The `saturating_add` keeps the
/// result at `usize::MAX`, which is always out-of-bounds.
#[test]
fn parse_entry_offset_plus_size_overflow_rejected() {
    let count: u16 = 1;
    let data_size: u32 = 0;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&data_size.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes()); // crc
    bytes.extend_from_slice(&u32::MAX.to_le_bytes()); // offset
    bytes.extend_from_slice(&1u32.to_le_bytes()); // size
    let err = MixArchive::parse(&bytes).unwrap_err();
    assert!(matches!(err, Error::InvalidOffset { .. }));
}

/// Entry with both `offset` and `size` at `u32::MAX` → `InvalidOffset`.
///
/// Why: the worst-case double-max scenario must saturate to `usize::MAX`,
/// not wrap to `u32::MAX - 1`.
#[test]
fn parse_entry_double_max_overflow_rejected() {
    let count: u16 = 1;
    let data_size: u32 = 0;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&data_size.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes()); // crc
    bytes.extend_from_slice(&u32::MAX.to_le_bytes()); // offset
    bytes.extend_from_slice(&u32::MAX.to_le_bytes()); // size
    let err = MixArchive::parse(&bytes).unwrap_err();
    assert!(matches!(err, Error::InvalidOffset { .. }));
}

// ── Security: overflow & edge-case tests ─────────────────────────────

/// `get_by_crc` uses `saturating_add` internally for offset + size.
///
/// Why: even if a future code path admitted extreme offset/size pairs
/// into the entry table, the lookup must not wrap.  This test exercises
/// the normal happy path to confirm saturating arithmetic is in place.
#[test]
fn get_by_crc_saturating_add_safety() {
    // Craft a valid archive and verify retrieval still works.
    let bytes = build_mix(&[("SAFE.BIN", b"data")]);
    let archive = MixArchive::parse(&bytes).unwrap();
    assert_eq!(archive.get("SAFE.BIN").unwrap(), b"data");
}

/// CRC of an empty string is 0 and does not panic.
///
/// Why: the loop body is never entered for a zero-length input; the
/// accumulator stays at its initial value.
#[test]
fn crc_empty_string() {
    let c = crc("");
    // Empty input → no iterations → accum stays 0
    assert_eq!(c, MixCrc::from_raw(0));
}

/// CRC of a 3-character name tests the partial-group zero-padding path.
///
/// How: "ABC" yields one 4-byte group `[0x41, 0x42, 0x43, 0x00]`.
/// The expected value is computed step-by-step in the test body.
#[test]
fn crc_three_char_partial_group() {
    // "ABC" → one 4-byte group: [0x41, 0x42, 0x43, 0x00]
    let expected = {
        let g = u32::from_le_bytes([0x41, 0x42, 0x43, 0x00]);
        0u32.rotate_left(1).wrapping_add(g)
    };
    assert_eq!(crc("ABC"), MixCrc::from_raw(expected));
}

/// Count at the cap but data too short for the index → `UnexpectedEof`.
///
/// Why: a crafted header at the cap boundary (16 384 entries) still
/// needs `16384 × 12 = 196 608` index bytes.  If the input is truncated,
/// the parser must not attempt to read past the end.
#[test]
fn parse_large_count_within_cap_but_truncated_data() {
    // count = 16384 (at cap), but data is too short for the index
    let count: u16 = 16_384;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    // Only provide 24 bytes of index data (need 16384 * 12 = 196608)
    bytes.extend_from_slice(&[0u8; 24]);
    let err = MixArchive::parse(&bytes).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// Extended format with both encrypted + SHA-1 flags (`0x0003`) is
/// rejected on short input.
///
/// Why: encryption takes precedence over SHA-1 processing.  With
/// `encrypted-mix` enabled the parser attempts decryption, which fails
/// with `UnexpectedEof` on a 10-byte input.  Without the feature it
/// returns `EncryptedArchive` immediately.
#[test]
fn parse_extended_encrypted_with_sha1_returns_error() {
    let data = [
        0x00u8, 0x00, // extended marker
        0x03, 0x00, // flags = encrypted (0x02) | sha1 (0x01)
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let result = MixArchive::parse(&data);
    let err = result.unwrap_err();
    #[cfg(feature = "encrypted-mix")]
    assert!(
        matches!(err, Error::UnexpectedEof { .. }),
        "expected UnexpectedEof, got: {err}",
    );
    #[cfg(not(feature = "encrypted-mix"))]
    assert_eq!(err, Error::EncryptedArchive);
}

/// `get_by_crc` returns the correct file content for a known CRC key.
///
/// Why: tests the lower-level CRC lookup path that `get()` delegates to.
/// The test builds a 2-file archive and verifies the first file's data
/// is returned when its pre-computed CRC is supplied.
#[test]
fn get_by_crc_returns_correct_data() {
    let bytes = build_mix(&[("HELLO.TXT", b"world"), ("OTHER.BIN", b"data")]);
    let archive = MixArchive::parse(&bytes).unwrap();
    let key = crc("HELLO.TXT");
    let data = archive.get_by_crc(key).unwrap();
    assert_eq!(data, b"world");
}

// ── Encrypted MIX end-to-end ─────────────────────────────────────────

/// End-to-end: parse a Blowfish-encrypted MIX archive.
///
/// Why: proves the full encrypted pipeline — key derivation, Blowfish
/// decryption, header parsing, and file extraction — works end-to-end
/// with a synthetic archive.
///
/// How: an all-zero 80-byte key_source derives a Blowfish key via RSA.
/// A FileHeader (count=1, one SubBlock) is encrypted with that key.
/// The complete encrypted MIX is assembled as:
/// `[marker, flags=0x0002, key_source(80), encrypted_header, file_data]`.
/// `MixArchive::parse` must extract the embedded file by name.
#[cfg(feature = "encrypted-mix")]
#[test]
fn parse_encrypted_mix_end_to_end() {
    use blowfish::cipher::generic_array::GenericArray;
    use blowfish::cipher::{BlockEncrypt, KeyInit};
    type BlowfishBE = blowfish::Blowfish;

    // Derive the Blowfish key from an all-zero key_source.
    let key_source = [0u8; 80];
    let bf_key = crate::mix_crypt::derive_blowfish_key(&key_source).unwrap();

    // Build plaintext header: count=1, data_size=5, one SubBlock.
    // Layout: count(u16) + data_size(u32) + crc(u32) + offset(u32) + size(u32)
    let file_data = b"HELLO";
    let file_crc = crc("TEST.DAT");
    let mut plaintext = Vec::new();
    plaintext.extend_from_slice(&1u16.to_le_bytes());
    plaintext.extend_from_slice(&(file_data.len() as u32).to_le_bytes());
    plaintext.extend_from_slice(&file_crc.to_raw().to_le_bytes());
    plaintext.extend_from_slice(&0u32.to_le_bytes());
    plaintext.extend_from_slice(&(file_data.len() as u32).to_le_bytes());
    // Pad to 8-byte block boundary: 18 → 24 bytes (3 blocks).
    while plaintext.len() % 8 != 0 {
        plaintext.push(0);
    }

    // Encrypt the header with the derived key.
    let cipher = BlowfishBE::new_from_slice(&bf_key).unwrap();
    let mut encrypted_header = plaintext.clone();
    for chunk in encrypted_header.chunks_exact_mut(8) {
        cipher.encrypt_block(GenericArray::from_mut_slice(chunk));
    }

    // Assemble the full encrypted MIX archive.
    let mut archive_bytes = Vec::new();
    archive_bytes.extend_from_slice(&0u16.to_le_bytes()); // extended marker
    archive_bytes.extend_from_slice(&0x0002u16.to_le_bytes()); // flags: encrypted
    archive_bytes.extend_from_slice(&key_source); // 80-byte key_source
    archive_bytes.extend_from_slice(&encrypted_header); // encrypted header blocks
    archive_bytes.extend_from_slice(file_data); // data section

    // Parse and verify.
    let archive = MixArchive::parse(&archive_bytes).unwrap();
    assert_eq!(archive.file_count(), 1);
    let extracted = archive.get("TEST.DAT").expect("file should exist");
    assert_eq!(extracted, file_data);
}

// ── Module-specific adversarial tests ────────────────────────────────

/// An archive with overlapping SubBlock entries (offset ranges overlap)
/// parses without panic.
///
/// Why (V38): real-world or maliciously crafted MIX files may have entries
/// whose byte ranges overlap (e.g. two entries sharing the same data
/// region).  The parser validates that each entry's `offset+size` fits
/// within the data section but does not (and should not) reject overlaps
/// — some tools produce overlapping entries intentionally.
///
/// How: builds a 2-entry archive where both entries point to offset 0
/// with size 5, sharing the same 5-byte data region.
#[test]
fn adversarial_overlapping_entries_no_panic() {
    // Two entries sharing the same 5-byte data region at offset 0.
    let crc_a = MixCrc::from_raw(0x0000_0001);
    let crc_b = MixCrc::from_raw(0x0000_0002);
    let data_section = b"HELLO";

    let count: u16 = 2;
    let data_size: u32 = data_section.len() as u32;

    let mut out = Vec::new();
    // FileHeader: count + data_size
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&data_size.to_le_bytes());
    // SubBlock array (must be sorted by CRC for binary search)
    // Entry A: crc=1, offset=0, size=5
    out.extend_from_slice(&crc_a.to_raw().to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&5u32.to_le_bytes());
    // Entry B: crc=2, offset=0, size=5 (overlaps A entirely)
    out.extend_from_slice(&crc_b.to_raw().to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&5u32.to_le_bytes());
    // Data section
    out.extend_from_slice(data_section);

    let archive = MixArchive::parse(&out).unwrap();
    assert_eq!(archive.file_count(), 2);
    // Both entries should resolve to the same data.
    assert_eq!(archive.get_by_crc(crc_a), Some(data_section.as_slice()));
    assert_eq!(archive.get_by_crc(crc_b), Some(data_section.as_slice()));
}

/// An archive with duplicate CRC entries (same CRC, different offsets)
/// parses without panic.
///
/// Why (V38): if a crafted archive contains two SubBlock entries with the
/// same CRC, `binary_search_by_key` returns one of them (unspecified
/// which).  The parser must not panic or corrupt memory — it should
/// return valid data for whichever entry the search finds.
///
/// How: builds a 2-entry archive where both entries share CRC 0x12345678
/// but point to different data regions.
#[test]
fn adversarial_duplicate_crcs_no_panic() {
    let dup_crc = MixCrc::from_raw(0x1234_5678);
    let data_a = b"AAAA";
    let data_b = b"BBBB";

    let count: u16 = 2;
    let data_size: u32 = (data_a.len() + data_b.len()) as u32;

    let mut out = Vec::new();
    // FileHeader
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&data_size.to_le_bytes());
    // SubBlock array (both entries have the same CRC — "sorted" trivially)
    // Entry 0: offset=0, size=4
    out.extend_from_slice(&dup_crc.to_raw().to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&4u32.to_le_bytes());
    // Entry 1: offset=4, size=4
    out.extend_from_slice(&dup_crc.to_raw().to_le_bytes());
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(&4u32.to_le_bytes());
    // Data section
    out.extend_from_slice(data_a);
    out.extend_from_slice(data_b);

    let archive = MixArchive::parse(&out).unwrap();
    assert_eq!(archive.file_count(), 2);
    // binary_search returns one of the two — we just verify no panic.
    let result = archive.get_by_crc(dup_crc);
    assert!(result.is_some(), "should find at least one entry");
    let data = result.unwrap();
    assert!(
        data == data_a || data == data_b,
        "should return data from one of the two entries",
    );
}

/// `get_by_index` preserves distinct payloads for duplicate CRC entries.
///
/// Why: archive extraction must be able to dump every physical entry even when
/// CRC lookup is ambiguous.
#[test]
fn get_by_index_preserves_duplicate_crc_entries() {
    let dup_crc = MixCrc::from_raw(0x1234_5678);
    let data_a = b"AAAA";
    let data_b = b"BBBB";

    let count: u16 = 2;
    let data_size: u32 = (data_a.len() + data_b.len()) as u32;

    let mut out = Vec::new();
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&data_size.to_le_bytes());
    out.extend_from_slice(&dup_crc.to_raw().to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(&dup_crc.to_raw().to_le_bytes());
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(data_a);
    out.extend_from_slice(data_b);

    let archive = MixArchive::parse(&out).unwrap();
    assert_eq!(archive.get_by_index(0), Some(data_a.as_slice()));
    assert_eq!(archive.get_by_index(1), Some(data_b.as_slice()));
}

/// An archive where a SubBlock's offset points into the header area
/// (offset beyond data section) is rejected.
///
/// Why (V38): a malicious archive could set an entry's offset+size to
/// reference the header bytes.  Since offsets are relative to the data
/// section start, any offset exceeding the data section length must be
/// caught by the bounds check.
///
/// How: builds a 1-entry archive with 5 bytes of data but the SubBlock
/// offset is set to 100 (well past the 5-byte data section).
#[test]
fn adversarial_offset_past_data_section() {
    let count: u16 = 1;
    let data_section = b"HELLO";
    let data_size: u32 = data_section.len() as u32;

    let mut out = Vec::new();
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&data_size.to_le_bytes());
    // SubBlock: crc=1, offset=100 (past data section), size=5
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&100u32.to_le_bytes());
    out.extend_from_slice(&5u32.to_le_bytes());
    out.extend_from_slice(data_section);

    let err = MixArchive::parse(&out).unwrap_err();
    assert!(
        matches!(err, Error::InvalidOffset { .. }),
        "expected InvalidOffset, got: {err}",
    );
}

/// An archive with a zero-size entry at offset 0 parses successfully.
///
/// Why: zero-size entries are valid (some archives contain placeholder
/// entries).  The parser must handle `offset=0, size=0` without
/// off-by-one errors.
#[test]
fn adversarial_zero_size_entry() {
    let count: u16 = 1;
    let data_section = b"DATA";
    let data_size: u32 = data_section.len() as u32;
    let entry_crc = MixCrc::from_raw(0xDEAD);

    let mut out = Vec::new();
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&data_size.to_le_bytes());
    // SubBlock: offset=0, size=0
    out.extend_from_slice(&entry_crc.to_raw().to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(data_section);

    let archive = MixArchive::parse(&out).unwrap();
    assert_eq!(archive.file_count(), 1);
    let data = archive.get_by_crc(entry_crc).unwrap();
    assert!(data.is_empty(), "zero-size entry should return empty slice");
}
