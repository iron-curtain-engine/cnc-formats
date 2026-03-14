// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

//! Validation, adversarial, and boundary tests for the MEG parser.

use super::*;
use crate::error::Error;

// ── Display ──────────────────────────────────────────────────────────────

/// MegEntry Debug output contains all fields.
///
/// Why: diagnostics and error messages depend on Debug formatting.
#[test]
fn entry_debug_format() {
    let entry = MegEntry {
        name: "test.txt".to_string(),
        offset: 100,
        size: 50,
    };
    let dbg = format!("{entry:?}");
    assert!(dbg.contains("test.txt"));
    assert!(dbg.contains("100"));
    assert!(dbg.contains("50"));
}

/// MegEntry Clone produces an independent copy.
#[test]
fn entry_clone_independent() {
    let entry = MegEntry {
        name: "foo.bin".to_string(),
        offset: 42,
        size: 7,
    };
    let cloned = entry.clone();
    assert_eq!(entry, cloned);
}

// ── Adversarial security tests ──────────────────────────────────────────

/// `MegArchive::parse` on 256 bytes of `0xFF` must not panic.
///
/// Why (V38): an all-ones buffer sets `num_filenames = 0xFFFFFFFF` which
/// exceeds the 65,536 cap.  The parser must reject cleanly without
/// overflow or OOM.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 256];
    let _ = MegArchive::parse(&data);
}

/// `MegArchive::parse` on 256 zero bytes must not panic.
///
/// Why: an all-zero header has `num_filenames = 0` and `num_files = 0`,
/// which is a valid empty archive.  The parser must handle it cleanly.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = vec![0u8; 256];
    let result = MegArchive::parse(&data);
    // All zeros = empty archive (num_filenames=0, num_files=0).
    assert!(result.is_ok());
    assert_eq!(result.unwrap().file_count(), 0);
}

/// `MegArchive::parse` on a single byte must not panic.
///
/// Why: minimal input that cannot possibly contain a valid header.
#[test]
fn adversarial_single_byte() {
    let _ = MegArchive::parse(&[0x42]);
}

/// Large claimed filename count with insufficient data → error, not OOM.
///
/// Why (V38): if the parser trusted `num_filenames` without bounds
/// checking, it would attempt to allocate a huge Vec and loop through
/// more filenames than exist in the buffer.
#[test]
fn adversarial_huge_filename_count_within_cap() {
    // num_filenames = 1000 (within cap but no actual filename data)
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1000u32.to_le_bytes());
    bytes.extend_from_slice(&1000u32.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 32]); // minimal padding

    let result = MegArchive::parse(&bytes);
    assert!(result.is_err());
}

/// Filename claiming more bytes than remain → `UnexpectedEof`.
///
/// Why: a crafted filename length can point past the end of the buffer.
#[test]
fn adversarial_filename_overflows_buffer() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u32.to_le_bytes()); // num_filenames = 1
    bytes.extend_from_slice(&1u32.to_le_bytes()); // num_files = 1
                                                  // Filename length = 200, but only 10 bytes follow
    bytes.extend_from_slice(&200u16.to_le_bytes());
    bytes.extend_from_slice(&[0x41u8; 10]);

    let result = MegArchive::parse(&bytes);
    assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
}

/// File record table truncated → `UnexpectedEof`.
///
/// Why: after parsing the filename table, the parser expects
/// `num_files * 18` bytes for file records.  Truncation must be caught.
#[test]
fn adversarial_truncated_record_table() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u32.to_le_bytes()); // num_filenames = 1
    bytes.extend_from_slice(&1u32.to_le_bytes()); // num_files = 1
                                                  // One filename: "A"
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.push(b'A');
    // Only 10 bytes of record data (need 18)
    bytes.extend_from_slice(&[0u8; 10]);

    let result = MegArchive::parse(&bytes);
    assert!(matches!(result, Err(Error::UnexpectedEof { .. })));
}

// ── Boundary tests ──────────────────────────────────────────────────────

/// Exactly at the MAX_MEG_ENTRIES cap → accepted (not rejected).
///
/// Why: the cap check is `>`, not `>=`, so exactly MAX_MEG_ENTRIES
/// should be accepted if the data is sufficient.  We only test the
/// header acceptance (data will be insufficient, producing a different
/// error).
#[test]
fn boundary_exact_cap_accepted_in_header() {
    let count = MAX_MEG_ENTRIES as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&count.to_le_bytes());
    // Not enough data for the filename table, but the cap check passes.
    bytes.extend_from_slice(&[0u8; 64]);

    let result = MegArchive::parse(&bytes);
    // Should fail with UnexpectedEof (not enough filename data),
    // NOT with InvalidSize (cap exceeded).
    assert!(
        matches!(result, Err(Error::UnexpectedEof { .. })),
        "expected UnexpectedEof at exact cap, got: {result:?}"
    );
}

/// One past MAX_MEG_ENTRIES → `InvalidSize`.
#[test]
fn boundary_one_past_cap_rejected() {
    let count = (MAX_MEG_ENTRIES + 1) as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 64]);

    let result = MegArchive::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidSize { .. })));
}

/// Filename of exactly MAX_FILENAME_LEN bytes → accepted (if data exists).
#[test]
fn boundary_max_filename_len_accepted() {
    let name = "A".repeat(MAX_FILENAME_LEN);
    let content = b"data";
    let bytes = tests::build_meg(&[(&name, content)]);
    let archive = MegArchive::parse(&bytes).unwrap();
    assert_eq!(archive.file_count(), 1);
    assert_eq!(archive.entries()[0].name, name);
}

/// Filename of MAX_FILENAME_LEN + 1 bytes → `InvalidSize`.
#[test]
fn boundary_filename_one_past_cap_rejected() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    let bad_len = (MAX_FILENAME_LEN + 1) as u16;
    bytes.extend_from_slice(&bad_len.to_le_bytes());
    bytes.extend_from_slice(&vec![b'A'; MAX_FILENAME_LEN + 1]);

    let result = MegArchive::parse(&bytes);
    assert!(matches!(result, Err(Error::InvalidSize { .. })));
}

// ── Determinism ─────────────────────────────────────────────────────────

/// Same input always produces the same parse result.
///
/// Why: determinism is a core invariant — non-deterministic parsing would
/// break file verification and caching.
#[test]
fn deterministic_parse() {
    let bytes = tests::build_meg(&[("DET.BIN", b"deterministic")]);
    let a = MegArchive::parse(&bytes).unwrap();
    let b = MegArchive::parse(&bytes).unwrap();

    assert_eq!(a.file_count(), b.file_count());
    assert_eq!(a.entries(), b.entries());
    assert_eq!(a.get("DET.BIN"), b.get("DET.BIN"));
}
