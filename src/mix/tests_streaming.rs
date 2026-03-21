// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::*;

use std::io::{Cursor, Read, Seek, SeekFrom};

fn build_mix(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut entries: Vec<(MixCrc, &[u8])> = files
        .iter()
        .map(|(name, data)| (crc(name), *data))
        .collect();
    entries.sort_by_key(|(entry_crc, _)| *entry_crc);

    let count = entries.len() as u16;
    let mut offsets = Vec::with_capacity(entries.len());
    let mut cur = 0u32;
    for (_, data) in &entries {
        offsets.push(cur);
        cur = cur.saturating_add(data.len() as u32);
    }

    let mut out = Vec::new();
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cur.to_le_bytes());
    for (index, (entry_crc, data)) in entries.iter().enumerate() {
        out.extend_from_slice(&entry_crc.to_raw().to_le_bytes());
        out.extend_from_slice(&offsets[index].to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    }
    for (_, data) in &entries {
        out.extend_from_slice(data);
    }
    out
}

#[test]
fn mix_entry_reader_reads_exact_entry_bounds() {
    let bytes = build_mix(&[("A.BIN", b"abc"), ("B.BIN", b"wxyz")]);
    let mut archive = MixArchiveReader::open(Cursor::new(bytes)).unwrap();
    let mut reader = archive.open_entry("B.BIN").unwrap().unwrap();
    let mut out = Vec::new();

    reader.read_to_end(&mut out).unwrap();

    assert_eq!(out, b"wxyz");
    assert_eq!(reader.remaining_len(), 0);
}

#[test]
fn mix_entry_reader_seek_stays_within_entry() {
    let bytes = build_mix(&[("A.BIN", b"abcdef")]);
    let mut archive = MixArchiveReader::open(Cursor::new(bytes)).unwrap();
    let mut reader = archive.open_entry("A.BIN").unwrap().unwrap();
    let mut out = [0u8; 2];

    assert_eq!(reader.seek(SeekFrom::Start(2)).unwrap(), 2);
    assert_eq!(reader.read(&mut out).unwrap(), 2);
    assert_eq!(&out, b"cd");
    assert_eq!(reader.seek(SeekFrom::End(-1)).unwrap(), 5);
    assert_eq!(reader.read(out.get_mut(..1).unwrap_or(&mut [])).unwrap(), 1);
    assert_eq!(out.first().copied(), Some(b'f'));
    assert!(reader.seek(SeekFrom::Start(7)).is_err());
}

#[test]
fn mix_overlay_index_uses_latest_mount_wins() {
    let base_bytes = build_mix(&[("RULES.INI", b"base"), ("A.BIN", b"a")]);
    let patch_bytes = build_mix(&[("RULES.INI", b"patch"), ("B.BIN", b"b")]);
    let base = MixArchive::parse(&base_bytes).unwrap();
    let patch = MixArchive::parse(&patch_bytes).unwrap();
    let mut overlay = MixOverlayIndex::new();

    overlay.mount_archive("base", base.entries());
    overlay.mount_archive("patch", patch.entries());

    let rules = overlay.resolve_name("RULES.INI").unwrap();
    let a_bin = overlay.resolve_name("A.BIN").unwrap();
    let b_bin = overlay.resolve_name("B.BIN").unwrap();

    assert_eq!(rules.source, "patch");
    assert_eq!(a_bin.source, "base");
    assert_eq!(b_bin.source, "patch");
    assert_eq!(overlay.len(), 3);
}

#[test]
fn mix_overlay_index_resolves_crc_directly() {
    let archive_bytes = build_mix(&[("SPEECH.AUD", b"aud")]);
    let archive = MixArchive::parse(&archive_bytes).unwrap();
    let mut overlay = MixOverlayIndex::new();

    overlay.mount_archive(42usize, archive.entries());

    let resolved = overlay.resolve_crc(crc("SPEECH.AUD")).unwrap();
    assert_eq!(resolved.source, 42);
    assert_eq!(resolved.size, 3);
}

#[test]
fn indices_by_offset_returns_offset_sorted_order() {
    // Create entries whose CRC order differs from offset order.
    // build_mix sorts by CRC, so data layout follows CRC order.
    // We verify that indices_by_offset returns the file-order traversal.
    let bytes = build_mix(&[
        ("AAA.BIN", b"first"),
        ("ZZZ.BIN", b"second"),
        ("MMM.BIN", b"third"),
    ]);
    let archive = MixArchiveReader::open(Cursor::new(bytes)).unwrap();
    let indices = archive.indices_by_offset();

    // Verify offsets are monotonically increasing in the returned order.
    let entries = archive.entries();
    let mut prev_offset = 0u32;
    for &i in &indices {
        let offset = entries.get(i).map_or(u32::MAX, |e| e.offset);
        assert!(offset >= prev_offset, "offset ordering violated");
        prev_offset = offset;
    }

    // All indices must be present.
    assert_eq!(indices.len(), entries.len());
}

// ─── Streaming-vs-batch correctness proofs ──────────────────────────────────

/// Proves streaming entry reads are byte-identical to batch for all access
/// methods (by name, by CRC, by index).
#[test]
fn streaming_reads_match_batch_for_all_entries() {
    let files: &[(&str, &[u8])] = &[
        ("RULES.INI", b"[General]\nGameSpeed=5"),
        ("SPEECH.AUD", &[0xAA; 64]),
        ("CONQUER.MIX", b"nested-archive-bytes"),
    ];
    let bytes = build_mix(files);

    let batch = MixArchive::parse(&bytes).unwrap();
    let mut stream = MixArchiveReader::open(Cursor::new(&bytes)).unwrap();

    assert_eq!(batch.file_count(), stream.file_count());

    // Compare by index — both use the same CRC-sorted order.
    for i in 0..batch.file_count() {
        let batch_data = batch.get_by_index(i).unwrap();
        let stream_data = stream.read_by_index(i).unwrap().unwrap();
        assert_eq!(batch_data, stream_data.as_slice(), "index {i} mismatch");
    }

    // Compare by filename (CRC hash lookup).
    for (name, _) in files {
        let batch_data = batch.get(name).unwrap();
        let stream_data = stream.read(name).unwrap().unwrap();
        assert_eq!(batch_data, stream_data.as_slice(), "{name} mismatch");
    }

    // Compare by CRC directly.
    for (name, _) in files {
        let key = crc(name);
        let batch_data = batch.get_by_crc(key).unwrap();
        let stream_data = stream.read_by_crc(key).unwrap().unwrap();
        assert_eq!(batch_data, stream_data.as_slice(), "CRC {key:?} mismatch");
    }
}

/// Proves `copy_by_index` writes the same bytes as batch `get_by_index`.
#[test]
fn streaming_copy_matches_batch_get() {
    let files: &[(&str, &[u8])] = &[
        ("MAP.BIN", &[0x01, 0x02, 0x03, 0x04, 0x05]),
        ("THEME.AUD", &[0xFF; 128]),
    ];
    let bytes = build_mix(files);

    let batch = MixArchive::parse(&bytes).unwrap();
    let mut stream = MixArchiveReader::open(Cursor::new(&bytes)).unwrap();

    for i in 0..batch.file_count() {
        let batch_data = batch.get_by_index(i).unwrap();
        let mut copied = Vec::new();
        let found = stream.copy_by_index(i, &mut copied).unwrap();
        assert!(found, "copy_by_index({i}) should find entry");
        assert_eq!(
            copied.as_slice(),
            batch_data,
            "copy_by_index({i}) content mismatch"
        );
    }
}

/// Proves entry metadata (CRC, offset, size) is identical between batch and
/// streaming parsers.
#[test]
fn streaming_entry_metadata_matches_batch() {
    let files: &[(&str, &[u8])] = &[
        ("A.TXT", b"alpha"),
        ("B.TXT", b"bravo-data"),
        ("C.TXT", b"c"),
    ];
    let bytes = build_mix(files);

    let batch = MixArchive::parse(&bytes).unwrap();
    let stream = MixArchiveReader::open(Cursor::new(&bytes)).unwrap();

    let batch_entries = batch.entries();
    let stream_entries = stream.entries();

    assert_eq!(batch_entries.len(), stream_entries.len());
    for (b, s) in batch_entries.iter().zip(stream_entries.iter()) {
        assert_eq!(b.crc, s.crc, "CRC mismatch");
        assert_eq!(b.offset, s.offset, "offset mismatch");
        assert_eq!(b.size, s.size, "size mismatch");
    }
}
