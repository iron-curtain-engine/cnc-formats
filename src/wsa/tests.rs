// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

// ─── Test helpers ────────────────────────────────────────────────────────────

/// Builds a minimal valid WSA file with the given number of frames.
///
/// Each frame's compressed data is a short byte sequence `[0x80, frame_idx]`
/// (arbitrary — we only test parsing here, not LCW decompression).
/// If `has_loop` is true, the last offset entry points to a dummy loop delta.
fn build_wsa(num_frames: u16, width: u16, height: u16, has_loop: bool) -> Vec<u8> {
    let frame_payload_size = 2usize; // 2 bytes per frame as dummy data
    let num_offsets = (num_frames as usize) + 2;
    let offsets_size = num_offsets * 4;
    let header_and_offsets = 14 + offsets_size;

    // Total frame data: num_frames frames of frame_payload_size each.
    // If has_loop, add one more for the loop delta.
    let data_payload =
        (num_frames as usize) * frame_payload_size + if has_loop { frame_payload_size } else { 0 };
    let total = header_and_offsets + data_payload;
    let mut buf = vec![0u8; total];

    // ── Header (14 bytes) ──
    buf[0..2].copy_from_slice(&num_frames.to_le_bytes());
    buf[2..4].copy_from_slice(&0u16.to_le_bytes()); // x
    buf[4..6].copy_from_slice(&0u16.to_le_bytes()); // y
    buf[6..8].copy_from_slice(&width.to_le_bytes());
    buf[8..10].copy_from_slice(&height.to_le_bytes());
    let buf_size = (width as u32) * (height as u32);
    buf[10..14].copy_from_slice(&buf_size.to_le_bytes());

    // ── Offset table ──
    // Offsets are relative to the data area (after header + offset table).
    let offset_table_start = 14;
    for i in 0..=num_frames as usize {
        let rel_offset = i * frame_payload_size;
        let pos = offset_table_start + i * 4;
        buf[pos..pos + 4].copy_from_slice(&(rel_offset as u32).to_le_bytes());
    }

    // Loop delta offset.
    let loop_entry_pos = offset_table_start + (num_frames as usize + 1) * 4;
    if has_loop {
        let loop_rel = (num_frames as usize) * frame_payload_size;
        buf[loop_entry_pos..loop_entry_pos + 4].copy_from_slice(&(loop_rel as u32).to_le_bytes());
    } else {
        buf[loop_entry_pos..loop_entry_pos + 4].copy_from_slice(&0u32.to_le_bytes());
    }

    // ── Frame data ──
    for i in 0..num_frames as usize {
        let abs = header_and_offsets + i * frame_payload_size;
        buf[abs] = 0x80; // dummy
        buf[abs + 1] = i as u8;
    }
    if has_loop {
        let abs = header_and_offsets + (num_frames as usize) * frame_payload_size;
        buf[abs] = 0x80;
        buf[abs + 1] = 0xFF; // loop marker
    }

    buf
}

// ─── Basic functionality ─────────────────────────────────────────────────────

/// Parses a well-formed WSA with 3 frames and no loop.
#[test]
fn parse_basic() {
    let data = build_wsa(3, 320, 200, false);
    let wsa = WsaFile::parse(&data).unwrap();
    assert_eq!(wsa.header.num_frames, 3);
    assert_eq!(wsa.header.width, 320);
    assert_eq!(wsa.header.height, 200);
    assert_eq!(wsa.header.buffer_size, 64000);
    assert_eq!(wsa.frames.len(), 3);
    assert!(!wsa.has_loop_frame);
    // Each frame has 2 bytes of dummy data.
    assert_eq!(wsa.frames[0].data.len(), 2);
    assert_eq!(wsa.frames[0].data[0], 0x80);
    assert_eq!(wsa.frames[1].data[1], 1);
}

/// Parses a WSA with a loop frame.
#[test]
fn parse_with_loop() {
    let data = build_wsa(2, 64, 64, true);
    let wsa = WsaFile::parse(&data).unwrap();
    assert!(wsa.has_loop_frame);
    assert_eq!(wsa.frames.len(), 2);
}

/// Zero-frame WSA is accepted (valid but degenerate).
#[test]
fn parse_zero_frames() {
    let data = build_wsa(0, 320, 200, false);
    let wsa = WsaFile::parse(&data).unwrap();
    assert_eq!(wsa.header.num_frames, 0);
    assert_eq!(wsa.frames.len(), 0);
    assert!(!wsa.has_loop_frame);
}

/// Frame index is correctly assigned.
#[test]
fn frame_indices() {
    let data = build_wsa(4, 16, 16, false);
    let wsa = WsaFile::parse(&data).unwrap();
    for (i, f) in wsa.frames.iter().enumerate() {
        assert_eq!(f.index, i);
    }
}

// ─── Error paths ─────────────────────────────────────────────────────────────

/// Truncated header returns UnexpectedEof.
#[test]
fn truncated_header() {
    let data = [0u8; 13];
    let err = WsaFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::UnexpectedEof {
            needed: 14,
            available: 13
        }
    ));
}

/// Truncated offset table returns UnexpectedEof.
#[test]
fn truncated_offset_table() {
    let mut data = build_wsa(5, 320, 200, false);
    // Need 14 + (5+2)*4 = 42 bytes for header+offsets; truncate to 30.
    data.truncate(30);
    let err = WsaFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// Frame data truncated returns UnexpectedEof.
#[test]
fn truncated_frame_data() {
    let mut data = build_wsa(3, 320, 200, false);
    data.truncate(data.len() - 1);
    let err = WsaFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// Frame count over V38 cap returns InvalidSize.
#[test]
fn over_max_frame_count() {
    // Can't construct a real file with 8193 frames — just test the header.
    let mut data = vec![0u8; 14];
    data[0..2].copy_from_slice(&8193u16.to_le_bytes());
    data[6..8].copy_from_slice(&1u16.to_le_bytes()); // width=1
    data[8..10].copy_from_slice(&1u16.to_le_bytes()); // height=1
    let err = WsaFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            value: 8193,
            limit: 8192,
            ..
        }
    ));
}

/// Frame area over V38 cap returns InvalidSize.
#[test]
fn over_max_frame_area() {
    let mut data = vec![0u8; 14];
    data[0..2].copy_from_slice(&1u16.to_le_bytes()); // num_frames=1
    data[6..8].copy_from_slice(&0xFFFFu16.to_le_bytes()); // width=65535
    data[8..10].copy_from_slice(&0xFFFFu16.to_le_bytes()); // height=65535
    let err = WsaFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidSize { .. }));
}

// ─── Determinism ─────────────────────────────────────────────────────────────

/// Parsing the same input twice yields identical results.
#[test]
fn deterministic() {
    let data = build_wsa(5, 320, 200, true);
    let a = WsaFile::parse(&data).unwrap();
    let b = WsaFile::parse(&data).unwrap();
    assert_eq!(a, b);
}

// ─── Display messages ────────────────────────────────────────────────────────

/// Error Display output includes numeric context.
#[test]
fn error_display_includes_values() {
    let data = [0u8; 10];
    let err = WsaFile::parse(&data).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("14"));
    assert!(msg.contains("10"));
}

// ─── Integer overflow safety ─────────────────────────────────────────────────

/// Offset table size that would overflow uses saturating arithmetic and
/// returns an error instead of panicking.
#[test]
fn offsets_overflow_no_panic() {
    // Header claiming 0xFFFF frames needs (65535+2)*4 = 262148 bytes of
    // offsets — way more than 14 bytes of available data.
    let mut data = vec![0u8; 14];
    data[0..2].copy_from_slice(&0xFFFFu16.to_le_bytes());
    data[6..8].copy_from_slice(&1u16.to_le_bytes());
    data[8..10].copy_from_slice(&1u16.to_le_bytes());
    // Should be rejected either by MAX_FRAME_COUNT (8192) or by EOF check.
    let err = WsaFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize { .. } | Error::UnexpectedEof { .. }
    ));
}

// ─── Security adversarial tests ──────────────────────────────────────────────

/// All-0xFF input must not panic — exercises maximum field values.
///
/// Why: every parsed u16/u32 field becomes 0xFFFF/0xFFFFFFFF, the worst-case
/// for overflow.  The parser must reject gracefully.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = [0xFF; 256];
    let _ = WsaFile::parse(&data);
}

/// All-zero input must not panic.
///
/// Why: zero frame count, zero dimensions — must not trigger division by
/// zero or empty-slice panics.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = [0u8; 256];
    let _ = WsaFile::parse(&data);
}

/// Offset table with self-referencing offsets must not cause infinite loops.
///
/// Why: a crafted file where offset[0] == offset[1] creates a zero-length
/// frame, which is valid.  But offset[n] pointing backward could confuse
/// naÃ¯ve parsers.
#[test]
fn adversarial_self_referencing_offsets() {
    let mut data = build_wsa(2, 8, 8, false);
    // Set all offsets to the same value — zero-length frames.
    let offset_val = (WSA_HEADER_SIZE + (2 + 2) * 4) as u32;
    for i in 0..4 {
        let pos = WSA_HEADER_SIZE + i * 4;
        data[pos..pos + 4].copy_from_slice(&offset_val.to_le_bytes());
    }
    let _ = WsaFile::parse(&data);
}
