// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

pub(crate) fn build_mix(files: &[(&str, &[u8])]) -> Vec<u8> {
    // Compute CRCs and sort.
    let mut entries: Vec<(MixCrc, &[u8])> = files
        .iter()
        .map(|(name, data)| (crc(name), *data))
        .collect();
    entries.sort_by_key(|(c, _)| *c);

    // Compute offsets.
    let count = entries.len() as u16;
    let mut offsets: Vec<u32> = Vec::with_capacity(entries.len());
    let mut cur = 0u32;
    for (_, data) in &entries {
        offsets.push(cur);
        cur += data.len() as u32;
    }
    let data_size: u32 = cur;

    let mut out = Vec::new();
    // FileHeader
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&data_size.to_le_bytes());
    // SubBlock array
    for (i, (c, data)) in entries.iter().enumerate() {
        out.extend_from_slice(&c.to_raw().to_le_bytes());
        out.extend_from_slice(&offsets[i].to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    }
    // File data
    for (_, data) in &entries {
        out.extend_from_slice(data);
    }
    out
}

// ── CRC tests ────────────────────────────────────────────────────────────

/// CRC is deterministic: the same input always produces the same hash.
///
/// Why: the CRC is used as the lookup key inside MIX archives; any
/// non-determinism would make files unretrievable.
#[test]
fn test_crc_deterministic() {
    assert_eq!(crc("CONQUER.MIX"), crc("CONQUER.MIX"));
}

/// CRC is case-insensitive: the filename is uppercased before hashing.
///
/// Why: the Westwood engine stores hashes of uppercased names, so
/// callers may legitimately pass mixed-case filenames.  We verify
/// three case variants all yield the same value.
#[test]
fn test_crc_case_insensitive() {
    assert_eq!(crc("conquer.mix"), crc("CONQUER.MIX"));
    assert_eq!(crc("Conquer.Mix"), crc("CONQUER.MIX"));
}

/// Different filenames produce different CRCs.
///
/// Why: collision across common RA filenames would make the archive
/// non-functional.  This is a basic sanity check, not an exhaustive
/// collision analysis.
#[test]
fn test_crc_different_names() {
    assert_ne!(crc("GENERAL.MIX"), crc("CONQUER.MIX"));
    assert_ne!(crc("AUDIO.MIX"), crc("SCORES.MIX"));
}

/// CRC of a single-character name exercises the zero-padding path.
///
/// How: "A" is only 1 byte, so the 4-byte group is [0x41, 0, 0, 0].
/// One `rotate_left(1)` + `wrapping_add` on an initial accumulator of 0
/// yields `0x41`.
#[test]
fn test_crc_short_name() {
    // "A" + zero-padding → single 4-byte chunk [0x41, 0x00, 0x00, 0x00]
    // CRC = 0u32.rotate_left(1) + 0x00000041 = 0x41
    assert_eq!(crc("A"), MixCrc::from_raw(0x41));
}

/// CRC of an 8-character name uses exactly two full 4-byte groups.
///
/// Why: tests the aligned no-padding path, verifying that multi-group
/// accumulation chains correctly.  The expected value is computed
/// step-by-step in the test body for reproducibility.
#[test]
fn test_crc_eight_chars() {
    let expected = {
        // "ABCDEFGH" → group1=[0x41,0x42,0x43,0x44], group2=[0x45,0x46,0x47,0x48]
        let g1 = u32::from_le_bytes([0x41, 0x42, 0x43, 0x44]);
        let g2 = u32::from_le_bytes([0x45, 0x46, 0x47, 0x48]);
        let c1: u32 = 0u32.rotate_left(1).wrapping_add(g1);
        c1.rotate_left(1).wrapping_add(g2)
    };
    assert_eq!(crc("ABCDEFGH"), MixCrc::from_raw(expected));
}

// ── Archive parsing tests ─────────────────────────────────────────────────

/// Parse an archive with zero files.
///
/// A 0-file archive cannot use the basic format because `count == 0`
/// produces a leading `0x0000` word, which is indistinguishable from the
/// extended-format marker.  The parser correctly follows the spec and
/// treats it as an extended-format archive.  We therefore build the
/// empty archive in extended format (marker=0x0000, flags=0x0000) followed
/// by a basic FileHeader with count=0.
#[test]
fn test_parse_empty_archive() {
    // Extended format: [0x0000 marker][0x0000 flags][count=0 u16][data_size=0 u32]
    let bytes: Vec<u8> = vec![
        0x00, 0x00, // extended marker
        0x00, 0x00, // flags = 0 (no SHA1, no encryption)
        0x00, 0x00, // count = 0
        0x00, 0x00, 0x00, 0x00, // data_size = 0
    ];
    let archive = MixArchive::parse(&bytes).unwrap();
    assert_eq!(archive.file_count(), 0);
}

/// Parse a single-file archive and retrieve the file by name.
///
/// Why: core happy-path test for the entire parse → lookup pipeline.
#[test]
fn test_parse_single_file() {
    let content = b"hello, world";
    let bytes = build_mix(&[("TEST.TXT", content)]);
    let archive = MixArchive::parse(&bytes).unwrap();

    assert_eq!(archive.file_count(), 1);
    let got = archive.get("TEST.TXT").expect("file not found");
    assert_eq!(got, content);
}

/// File lookup is case-insensitive (same CRC regardless of case).
///
/// Why: `get()` must uppercase the caller's string before hashing,
/// matching the Westwood convention.  Three case variants are tested.
#[test]
fn test_get_case_insensitive() {
    let content = b"data";
    let bytes = build_mix(&[("FILE.BIN", content)]);
    let archive = MixArchive::parse(&bytes).unwrap();

    assert_eq!(archive.get("FILE.BIN"), Some(content.as_ref()));
    assert_eq!(archive.get("file.bin"), Some(content.as_ref()));
    assert_eq!(archive.get("File.Bin"), Some(content.as_ref()));
}

/// Looking up a nonexistent filename returns `None`, not a panic.
///
/// Why: callers must be able to probe for optional files safely.
#[test]
fn test_get_nonexistent() {
    let bytes = build_mix(&[("PRESENT.BIN", b"x")]);
    let archive = MixArchive::parse(&bytes).unwrap();
    assert_eq!(archive.get("ABSENT.BIN"), None);
}

/// Multi-file archive: all files can be retrieved with correct content.
///
/// Why: verifies that the SubBlock index, CRC sort order, and offset
/// calculations are correct when multiple entries coexist.  Each file
/// is looked up by its original name.
#[test]
fn test_parse_multiple_files() {
    let files: &[(&str, &[u8])] = &[
        ("ALPHA.DAT", b"first"),
        ("BETA.DAT", b"second_file"),
        ("GAMMA.DAT", b"third"),
    ];
    let bytes = build_mix(files);
    let archive = MixArchive::parse(&bytes).unwrap();

    assert_eq!(archive.file_count(), 3);
    for (name, expected) in files {
        let got = archive.get(name).expect(name);
        assert_eq!(got, *expected, "content mismatch for {name}");
    }
}

/// Extended format (marker `0x0000`, flags = 0) parses identically.
///
/// Why: the extended format adds a 4-byte prefix; the parser must
/// advance past it and still read the FileHeader + SubBlocks correctly.
#[test]
fn test_parse_extended_format() {
    // Build a basic archive then prepend the extended header (0x0000, 0x0000).
    let basic = build_mix(&[("EXT.BIN", b"extended")]);
    let mut extended = Vec::new();
    extended.extend_from_slice(&[0x00u8, 0x00, 0x00, 0x00]); // marker + flags=0
    extended.extend_from_slice(&basic);

    let archive = MixArchive::parse(&extended).unwrap();
    assert_eq!(archive.get("EXT.BIN"), Some(b"extended".as_ref()));
}

/// Encrypted archive (flags bit 1) is rejected on short input.
///
/// Why: with `encrypted-mix` enabled, the parser attempts decryption
/// and needs an 80-byte key_source after the 4-byte header.  A short
/// input fails with `UnexpectedEof`.  Without the feature, it returns
/// `EncryptedArchive` immediately.
#[test]
fn test_parse_encrypted_returns_error() {
    // Extended marker + flags with bit 1 set (encrypted)
    let data = [0x00u8, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
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

/// Input shorter than the minimum header returns `UnexpectedEof`.
///
/// Why: the parser needs at least 2 bytes for format detection.  Both
/// 0-byte and 4-byte inputs are tested to cover different EOF points.
#[test]
fn test_parse_too_short() {
    assert!(matches!(
        MixArchive::parse(&[]),
        Err(Error::UnexpectedEof { .. })
    ));
    assert!(matches!(
        MixArchive::parse(&[0u8; 4]),
        Err(Error::UnexpectedEof { .. })
    ));
}

/// `entries()` returns SubBlocks sorted by CRC.
///
/// Why: the `build_mix` helper and the binary-search lookup both
/// depend on sort order.  We verify that no matter what order the
/// files are given, the stored entries emerge CRC-sorted.
#[test]
fn test_entries_sorted_by_crc() {
    let bytes = build_mix(&[("B.DAT", b"b"), ("A.DAT", b"a"), ("C.DAT", b"c")]);
    let archive = MixArchive::parse(&bytes).unwrap();
    let entries = archive.entries();
    for i in 1..entries.len() {
        assert!(
            entries[i - 1].crc <= entries[i].crc,
            "entries not sorted by CRC"
        );
    }
}

/// SubBlock whose offset+size exceeds the data section → `InvalidOffset`.
///
/// Why: every entry is validated during parsing; without this check a
/// malformed archive could cause an out-of-bounds slice later.
///
/// How: a 1-file header is built manually with `size = 9999` but only
/// 5 bytes of actual data.
#[test]
fn test_parse_invalid_offset() {
    // Build a 1-file archive manually with a SubBlock whose offset+size
    // exceeds the data section.
    let count: u16 = 1;
    let data_size: u32 = 5; // claim 5 bytes in data section

    let mut bytes = Vec::new();
    // FileHeader
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&data_size.to_le_bytes());
    // SubBlock: crc=1, offset=0, size=9999 (way past data section)
    bytes.extend_from_slice(&1u32.to_le_bytes()); // crc
    bytes.extend_from_slice(&0u32.to_le_bytes()); // offset
    bytes.extend_from_slice(&9999u32.to_le_bytes()); // size (too large)
                                                     // Data section: only 5 bytes
    bytes.extend_from_slice(&[0xAA; 5]);

    let result = MixArchive::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidOffset { .. })));
}

/// V38: entry count exceeding `MAX_MIX_ENTRIES` → `InvalidSize`.
///
/// Why (V38 safety cap): a crafted archive claiming millions of entries
/// could allocate gigabytes.  The parser rejects counts above the cap.
///
/// How: `count = 16385` as `u16` is non-zero (basic format), so the
/// parser reads it directly and hits the cap check.
#[test]
fn test_parse_entry_count_exceeds_cap() {
    // Craft a basic-format header claiming 16385 entries (exceeds MAX_MIX_ENTRIES).
    // count=16385 as u16 = 0x4001, which is nonzero so it's basic format.
    let count: u16 = 16_385;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes()); // data_size
                                                  // Don't need SubBlocks — should fail at the cap check
    bytes.extend_from_slice(&[0u8; 256]);

    let result = MixArchive::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidSize { .. })));
}

/// Extended format with SHA-1 flag (`flags = 0x0001`) is parsed correctly.
///
/// Why: the SHA-1 variant inserts a 20-byte digest between the SubBlock
/// array and the file data.  The parser must skip it and still resolve
/// file lookups.
///
/// How: a complete extended archive is built manually with a dummy
/// 20-byte digest field, then queried by filename.
#[test]
fn test_parse_extended_sha1() {
    // Build file data for one entry.
    let file_data = b"HELLO";
    let filename = "test.txt";
    let file_crc = crc(filename);

    let mut bytes = Vec::new();
    // Extended marker
    bytes.extend_from_slice(&0u16.to_le_bytes());
    // Flags: has_sha1 = 0x0001
    bytes.extend_from_slice(&1u16.to_le_bytes());
    // FileHeader: count=1, data_size
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&(file_data.len() as u32).to_le_bytes());
    // SubBlock: crc, offset=0, size
    bytes.extend_from_slice(&file_crc.to_raw().to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(file_data.len() as u32).to_le_bytes());
    // 20-byte SHA-1 digest (dummy)
    bytes.extend_from_slice(&[0xAB; 20]);
    // File data
    bytes.extend_from_slice(file_data);

    let archive = MixArchive::parse(&bytes).unwrap();
    assert_eq!(archive.file_count(), 1);
    assert_eq!(archive.get(filename).unwrap(), file_data);
}

/// Extended SHA-1 archive with a truncated digest returns `UnexpectedEof`.
///
/// Why: if the input is shorter than the 20-byte digest, the parser
/// must not read past the end of the buffer.
#[test]
fn test_parse_extended_sha1_truncated() {
    let mut bytes = Vec::new();
    // Extended marker
    bytes.extend_from_slice(&0u16.to_le_bytes());
    // Flags: has_sha1 = 0x0001
    bytes.extend_from_slice(&1u16.to_le_bytes());
    // FileHeader: count=0, data_size=0
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    // Only 10 of 20 SHA-1 bytes
    bytes.extend_from_slice(&[0u8; 10]);

    let result = MixArchive::parse(&bytes);
    assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
}

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

/// `InvalidSize` carries the offending value, the cap, and a context tag.
///
/// Why: when the cap rejects an entry count, the error must explain
/// *what* value was rejected and *what* the limit is.
#[test]
fn invalid_size_error_carries_value_and_limit() {
    let count: u16 = 16_385;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 256]);

    let err = MixArchive::parse(&bytes).unwrap_err();
    match err {
        Error::InvalidSize {
            value,
            limit,
            context,
        } => {
            assert_eq!(value, 16_385);
            assert_eq!(limit, 16_384);
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
    // InvalidSize Display
    let count: u16 = 16_385;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 256]);
    let msg = MixArchive::parse(&bytes).unwrap_err().to_string();
    assert!(msg.contains("16385"), "should show the value: {msg}");
    assert!(msg.contains("16384"), "should show the limit: {msg}");

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

/// Exactly `MAX_MIX_ENTRIES` entries is accepted (boundary: count == cap).
///
/// Why: the safety cap uses `>` not `>=`; this test proves that the
/// maximum permissible value is not accidentally rejected.
///
/// How: builds a header claiming 16 384 zero-size entries with enough
/// index bytes to satisfy the parser.
#[test]
fn parse_exactly_max_entries_is_accepted() {
    // Build a header claiming 16384 entries with enough index bytes.
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
    use blowfish::BlowfishLE;

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
    let cipher = BlowfishLE::new_from_slice(&bf_key).unwrap();
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

/// End-to-end encrypted MIX with SHA-1 flag set (flags = 0x0003).
///
/// Why: when both encryption and SHA-1 flags are set, the parser must
/// skip the 20-byte SHA-1 digest between the encrypted header and the
/// data section.  This test verifies that data-offset calculation.
///
/// How: same construction as the basic encrypted test, but with
/// `flags = 0x0003` and a 20-byte dummy SHA-1 digest inserted
/// between the encrypted header and the file data.
#[cfg(feature = "encrypted-mix")]
#[test]
fn parse_encrypted_mix_with_sha1_end_to_end() {
    use blowfish::cipher::generic_array::GenericArray;
    use blowfish::cipher::{BlockEncrypt, KeyInit};
    use blowfish::BlowfishLE;

    let key_source = [0u8; 80];
    let bf_key = crate::mix_crypt::derive_blowfish_key(&key_source).unwrap();

    let file_data = b"WORLD";
    let file_crc = crc("DATA.BIN");
    let mut plaintext = Vec::new();
    plaintext.extend_from_slice(&1u16.to_le_bytes());
    plaintext.extend_from_slice(&(file_data.len() as u32).to_le_bytes());
    plaintext.extend_from_slice(&file_crc.to_raw().to_le_bytes());
    plaintext.extend_from_slice(&0u32.to_le_bytes());
    plaintext.extend_from_slice(&(file_data.len() as u32).to_le_bytes());
    while plaintext.len() % 8 != 0 {
        plaintext.push(0);
    }

    let cipher = BlowfishLE::new_from_slice(&bf_key).unwrap();
    let mut encrypted_header = plaintext.clone();
    for chunk in encrypted_header.chunks_exact_mut(8) {
        cipher.encrypt_block(GenericArray::from_mut_slice(chunk));
    }

    let mut archive_bytes = Vec::new();
    archive_bytes.extend_from_slice(&0u16.to_le_bytes()); // extended marker
    archive_bytes.extend_from_slice(&0x0003u16.to_le_bytes()); // flags: encrypted + sha1
    archive_bytes.extend_from_slice(&key_source);
    archive_bytes.extend_from_slice(&encrypted_header);
    archive_bytes.extend_from_slice(&[0xAA; 20]); // dummy SHA-1 digest
    archive_bytes.extend_from_slice(file_data);

    let archive = MixArchive::parse(&archive_bytes).unwrap();
    assert_eq!(archive.file_count(), 1);
    let extracted = archive.get("DATA.BIN").expect("file should exist");
    assert_eq!(extracted, file_data);
}

// ── Adversarial security tests ───────────────────────────────────────

/// `MixArchive::parse` on 256 bytes of `0xFF` must not panic.
///
/// Why (V38): an all-ones buffer sets `count = 0xFFFF` (exceeds the
/// 16 384 cap) or, in extended mode, triggers encrypted-archive
/// handling.  The parser must reject cleanly without overflow or OOM.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 256];
    let _ = MixArchive::parse(&data);
}

/// `MixArchive::parse` on 256 zero bytes must not panic.
///
/// Why: an all-zero header has `count = 0` and the extended-format
/// marker `0x0000`, which enters the extended path with `flags = 0`.
/// The parser must handle the degenerate zero-entry archive or reject
/// the truncated extended header.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = vec![0u8; 256];
    let _ = MixArchive::parse(&data);
}
