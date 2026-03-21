// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

pub(crate) fn build_mix(files: &[(&str, &[u8])]) -> Vec<u8> {
    // Compute CRCs and sort by signed i32 comparison, matching how
    // Westwood's tools write SubBlock arrays on disk.  The parser
    // re-sorts by unsigned u32 after reading, so this exercises the
    // real-world code path.
    let mut entries: Vec<(MixCrc, &[u8])> = files
        .iter()
        .map(|(name, data)| (crc(name), *data))
        .collect();
    entries.sort_by_key(|(c, _)| c.to_raw() as i32);

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

/// Streaming reader opens the archive without a backing `&[u8]` parser result.
#[test]
fn test_stream_reader_reads_single_file() {
    let content = b"hello, world";
    let bytes = build_mix(&[("TEST.TXT", content)]);
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = MixArchiveReader::open(cursor).unwrap();

    assert_eq!(archive.file_count(), 1);
    let got = archive.read("TEST.TXT").unwrap().expect("file not found");
    assert_eq!(got, content);
}

/// Streaming reader can copy an entry directly into a writer.
#[test]
fn test_stream_reader_copies_entry() {
    let bytes = build_mix(&[("A.BIN", b"alpha"), ("B.BIN", b"beta")]);
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = MixArchiveReader::open(cursor).unwrap();
    let mut out = Vec::new();

    assert!(archive.copy_by_index(1, &mut out).unwrap());
    assert_eq!(out, b"beta");
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

/// Entries sorted by signed `i32` on disk are found via unsigned lookup.
///
/// Why: Westwood tools sort SubBlock entries by signed `i32` CRC.  Under
/// signed ordering, values `0x8000_0000..=0xFFFF_FFFF` sort BEFORE
/// `0x0000_0000..=0x7FFF_FFFF`.  The parser re-sorts by unsigned `u32`
/// so that `binary_search_by_key` (which uses unsigned `Ord` on `MixCrc`)
/// can find all entries.
///
/// How: manually build an archive with two entries whose CRCs straddle
/// the signed boundary, arranged in signed order on disk.
#[test]
fn test_signed_crc_order_lookup() {
    let crc_neg = MixCrc::from_raw(0xA000_0000); // negative under i32
    let crc_pos = MixCrc::from_raw(0x1000_0000); // positive under i32
    let data_neg = b"NEG";
    let data_pos = b"POS";

    let count: u16 = 2;
    let neg_size = data_neg.len() as u32;
    let pos_size = data_pos.len() as u32;
    // Data layout: data_neg at offset 0, data_pos at offset 3.
    let data_size: u32 = neg_size + pos_size;

    let mut bytes = Vec::new();
    // FileHeader
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&data_size.to_le_bytes());
    // SubBlocks in signed order: 0xA0000000 (-1610612736) < 0x10000000 (+268435456)
    bytes.extend_from_slice(&crc_neg.to_raw().to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes()); // offset=0
    bytes.extend_from_slice(&neg_size.to_le_bytes());
    bytes.extend_from_slice(&crc_pos.to_raw().to_le_bytes());
    bytes.extend_from_slice(&neg_size.to_le_bytes()); // offset=3
    bytes.extend_from_slice(&pos_size.to_le_bytes());
    // Data section
    bytes.extend_from_slice(data_neg);
    bytes.extend_from_slice(data_pos);

    let archive = MixArchive::parse(&bytes).unwrap();
    // Both entries must be found despite signed-order on disk.
    assert_eq!(archive.get_by_crc(crc_neg), Some(data_neg.as_slice()));
    assert_eq!(archive.get_by_crc(crc_pos), Some(data_pos.as_slice()));
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

/// `entries()` returns SubBlocks sorted by unsigned CRC.
///
/// Why: Westwood tools sort entries by signed `i32` CRC on disk.
/// The parser re-sorts by unsigned `u32` for binary search compatibility.
/// We verify that regardless of input file order, entries emerge unsigned-sorted.
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

/// V38: MIX count as u16 (max 65535) is always within the cap.
///
/// Why: the cap (131,072) is larger than any u16 value.  A basic-format
/// header with u16::MAX entries should fail with UnexpectedEof (not enough
/// SubBlock data), not InvalidSize.  This verifies the cap doesn't reject
/// legitimate large archives like RA1's MAIN.MIX (~64,000 entries).
#[test]
fn test_parse_large_count_not_rejected_by_cap() {
    // u16::MAX = 65535 entries; cap is 131,072 so this should pass the cap
    // check but fail with UnexpectedEof (not enough SubBlock data).
    let count: u16 = u16::MAX;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes()); // data_size
    bytes.extend_from_slice(&[0u8; 256]);

    let result = MixArchive::parse(&bytes);
    // Should be EOF (not enough SubBlocks), not InvalidSize.
    assert!(
        matches!(result, Err(Error::UnexpectedEof { .. })),
        "expected UnexpectedEof for large count with insufficient data, got: {result:?}"
    );
}

/// Extended format with SHA-1 flag (`flags = 0x0001`) is parsed correctly.
///
/// Why: the SHA-1 digest is stored at the END of the file (after the
/// data section), not between the SubBlock array and the data.
/// The parser must ignore it and still resolve file lookups correctly.
///
/// How: a complete extended archive is built manually with a 20-byte
/// digest appended after the file data.
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
    // File data (immediately after SubBlock index)
    bytes.extend_from_slice(file_data);
    // SHA-1 digest at end of file (20 bytes, dummy)
    bytes.extend_from_slice(&[0xAB; 20]);

    let archive = MixArchive::parse(&bytes).unwrap();
    assert_eq!(archive.file_count(), 1);
    assert_eq!(archive.get(filename).unwrap(), file_data);
}

/// Extended SHA-1 archive with no trailing digest still parses.
///
/// Why: the SHA-1 flag only affects cache-time integrity verification
/// (not implemented here).  The parser must not require or skip the
/// digest — it is at the end of the file, after the data section.
#[test]
fn test_parse_extended_sha1_no_trailing_digest() {
    let mut bytes = Vec::new();
    // Extended marker
    bytes.extend_from_slice(&0u16.to_le_bytes());
    // Flags: has_sha1 = 0x0001
    bytes.extend_from_slice(&1u16.to_le_bytes());
    // FileHeader: count=0, data_size=0
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    // No SHA-1 digest appended — still valid.

    let archive = MixArchive::parse(&bytes).unwrap();
    assert_eq!(archive.file_count(), 0);
}

/// End-to-end encrypted MIX with SHA-1 flag set (flags = 0x0003).
///
/// Why: when both encryption and SHA-1 flags are set, the SHA-1 digest
/// is stored at the END of the file (after the data section), not
/// between the encrypted header and the data.  The parser must locate
/// the data section immediately after the encrypted blocks.
///
/// How: same construction as the basic encrypted test, but with
/// `flags = 0x0003` and a 20-byte dummy SHA-1 digest appended after
/// the file data.
#[cfg(feature = "encrypted-mix")]
#[test]
fn parse_encrypted_mix_with_sha1_end_to_end() {
    use blowfish::cipher::generic_array::GenericArray;
    use blowfish::cipher::{BlockEncrypt, KeyInit};
    type BlowfishBE = blowfish::Blowfish;

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

    let cipher = BlowfishBE::new_from_slice(&bf_key).unwrap();
    let mut encrypted_header = plaintext.clone();
    for chunk in encrypted_header.chunks_exact_mut(8) {
        cipher.encrypt_block(GenericArray::from_mut_slice(chunk));
    }

    let mut archive_bytes = Vec::new();
    archive_bytes.extend_from_slice(&0u16.to_le_bytes()); // extended marker
    archive_bytes.extend_from_slice(&0x0003u16.to_le_bytes()); // flags: encrypted + sha1
    archive_bytes.extend_from_slice(&key_source);
    archive_bytes.extend_from_slice(&encrypted_header);
    // Data section immediately follows encrypted blocks.
    archive_bytes.extend_from_slice(file_data);
    // SHA-1 digest at end of file (20 bytes, dummy).
    archive_bytes.extend_from_slice(&[0xAA; 20]);

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
