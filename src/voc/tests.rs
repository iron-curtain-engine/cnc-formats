// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;
use crate::error::Error;

/// Build a valid VOC file with the given blocks.
///
/// Each entry in `blocks` is `(block_type, payload)`.  A terminator block
/// (type 0) is automatically appended.
fn build_voc(blocks: &[(u8, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    // Magic (20 bytes).
    out.extend_from_slice(b"Creative Voice File\x1a");
    // data_offset (u16 LE) = 26.
    out.extend_from_slice(&26u16.to_le_bytes());
    // version (u16 LE) = 0x010A (version 1.10).
    out.extend_from_slice(&0x010Au16.to_le_bytes());
    // version_check (u16 LE) = !version + 0x1234.
    let check = (!0x010Au16).wrapping_add(0x1234);
    out.extend_from_slice(&check.to_le_bytes());

    for (block_type, payload) in blocks {
        out.push(*block_type);
        if *block_type != BLOCK_TERMINATOR {
            let size = payload.len();
            out.push((size & 0xFF) as u8);
            out.push(((size >> 8) & 0xFF) as u8);
            out.push(((size >> 16) & 0xFF) as u8);
            out.extend_from_slice(payload);
        }
    }
    // Terminator.
    out.push(0);
    out
}

// ─── Happy path ──────────────────────────────────────────────────────────────

/// Parsing a VOC file with a single Sound Data block (type 1) succeeds.
#[test]
fn parse_valid_voc() {
    // Sound data payload: freq_divisor=156 (sr=10000), codec=0, 4 samples.
    let payload: &[u8] = &[156, 0, 0x80, 0x81, 0x82, 0x83];
    let data = build_voc(&[(BLOCK_SOUND_DATA, payload)]);
    let voc = VocFile::parse(&data).unwrap();

    assert_eq!(voc.version(), (1, 10));
    assert_eq!(voc.header().data_offset, 26);
    assert_eq!(voc.blocks().len(), 1);

    let block = &voc.blocks()[0];
    assert_eq!(block.block_type, BLOCK_SOUND_DATA);
    assert_eq!(block.size, 6);

    let block_bytes = voc.block_data(block).unwrap();
    assert_eq!(block_bytes, payload);

    // Sample rate: 1_000_000 / (256 - 156) = 10_000.
    assert_eq!(voc.sound_data_sample_rate(block), Some(10_000));
}

/// A VOC file with only a terminator (no data blocks) parses correctly.
#[test]
fn parse_empty_blocks() {
    let data = build_voc(&[]);
    let voc = VocFile::parse(&data).unwrap();

    assert_eq!(voc.blocks().len(), 0);
    assert_eq!(voc.version(), (1, 10));
}

/// Multiple block types can coexist in a single VOC file.
#[test]
fn parse_multiple_block_types() {
    // Sound data (type 1): freq_divisor=128, codec=0, 2 sample bytes.
    let sound_payload: &[u8] = &[128, 0, 0xAA, 0xBB];
    // Silence (type 3): duration=1000 (u16 LE), freq_divisor=128.
    let silence_payload: &[u8] = &[0xE8, 0x03, 128];
    // Marker (type 4): marker_id=42 (u16 LE).
    let marker_payload: &[u8] = &[42, 0];
    // Text (type 5): null-terminated ASCII.
    let text_payload: &[u8] = b"Hello\0";
    // Sound continue (type 2): raw sample bytes.
    let continue_payload: &[u8] = &[0xCC, 0xDD];

    let data = build_voc(&[
        (BLOCK_SOUND_DATA, sound_payload),
        (BLOCK_SILENCE, silence_payload),
        (BLOCK_MARKER, marker_payload),
        (BLOCK_TEXT, text_payload),
        (BLOCK_SOUND_CONTINUE, continue_payload),
    ]);
    let voc = VocFile::parse(&data).unwrap();

    assert_eq!(voc.blocks().len(), 5);
    assert_eq!(voc.blocks()[0].block_type, BLOCK_SOUND_DATA);
    assert_eq!(voc.blocks()[1].block_type, BLOCK_SILENCE);
    assert_eq!(voc.blocks()[2].block_type, BLOCK_MARKER);
    assert_eq!(voc.blocks()[3].block_type, BLOCK_TEXT);
    assert_eq!(voc.blocks()[4].block_type, BLOCK_SOUND_CONTINUE);

    // Verify payload of the text block.
    let text_data = voc.block_data(&voc.blocks()[3]).unwrap();
    assert_eq!(text_data, b"Hello\0");

    // Verify payload of the continue block.
    let continue_data = voc.block_data(&voc.blocks()[4]).unwrap();
    assert_eq!(continue_data, &[0xCC, 0xDD]);
}

/// Repeat start (type 6), repeat end (type 7), extended (type 8), and
/// new sound data (type 9) blocks are parsed correctly.
#[test]
fn parse_extended_block_types() {
    // Repeat start (type 6): count=3 (u16 LE).
    let repeat_start_payload: &[u8] = &[3, 0];
    // Repeat end (type 7): empty payload.
    let repeat_end_payload: &[u8] = &[];
    // Extended (type 8): freq=0xD25C (u16 LE), codec=0, channels_minus_one=0.
    let extended_payload: &[u8] = &[0x5C, 0xD2, 0, 0];
    // New sound data (type 9): sample_rate=22050 (u32 LE), bits=8, channels=1,
    // codec=0 (u16 LE), reserved=4 bytes, 2 sample bytes.
    let mut new_sound = Vec::new();
    new_sound.extend_from_slice(&22050u32.to_le_bytes());
    new_sound.push(8); // bits_per_sample
    new_sound.push(1); // channels
    new_sound.extend_from_slice(&0u16.to_le_bytes()); // codec
    new_sound.extend_from_slice(&[0u8; 4]); // reserved
    new_sound.extend_from_slice(&[0xEE, 0xFF]); // samples

    let data = build_voc(&[
        (BLOCK_REPEAT_START, repeat_start_payload),
        (BLOCK_REPEAT_END, repeat_end_payload),
        (BLOCK_EXTENDED, extended_payload),
        (BLOCK_NEW_SOUND_DATA, &new_sound),
    ]);
    let voc = VocFile::parse(&data).unwrap();

    assert_eq!(voc.blocks().len(), 4);
    assert_eq!(voc.blocks()[0].block_type, BLOCK_REPEAT_START);
    assert_eq!(voc.blocks()[0].size, 2);
    assert_eq!(voc.blocks()[1].block_type, BLOCK_REPEAT_END);
    assert_eq!(voc.blocks()[1].size, 0);
    assert_eq!(voc.blocks()[2].block_type, BLOCK_EXTENDED);
    assert_eq!(voc.blocks()[2].size, 4);
    assert_eq!(voc.blocks()[3].block_type, BLOCK_NEW_SOUND_DATA);
    assert_eq!(voc.blocks()[3].size, 14);

    // Verify new sound data payload.
    let ns_data = voc.block_data(&voc.blocks()[3]).unwrap();
    assert_eq!(ns_data.len(), 14);
    // Last two bytes are sample data.
    assert_eq!(&ns_data[12..], &[0xEE, 0xFF]);
}

/// `sound_data_sample_rate` returns `None` for non-type-1 blocks.
#[test]
fn sample_rate_wrong_block_type() {
    let data = build_voc(&[(BLOCK_SOUND_CONTINUE, &[0xAA, 0xBB])]);
    let voc = VocFile::parse(&data).unwrap();
    assert_eq!(voc.sound_data_sample_rate(&voc.blocks()[0]), None);
}

// ─── Error cases ─────────────────────────────────────────────────────────────

/// Corrupt magic bytes are rejected with `InvalidMagic`.
#[test]
fn reject_invalid_magic() {
    let mut data = build_voc(&[]);
    data[0] = b'X'; // corrupt first byte of magic
    let err = VocFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidMagic {
            context: "VOC header"
        }
    ));
}

/// Input shorter than 26 bytes is rejected with `UnexpectedEof`.
#[test]
fn reject_truncated_header() {
    let data = b"Creative Voice File\x1a\x1a\x00\x0a\x01";
    assert!(data.len() < HEADER_SIZE);
    let err = VocFile::parse(data.as_slice()).unwrap_err();
    assert!(matches!(
        err,
        Error::UnexpectedEof {
            needed: HEADER_SIZE,
            ..
        }
    ));
}

/// A wrong version check word is rejected.
#[test]
fn reject_invalid_version_check() {
    let mut data = build_voc(&[]);
    // Overwrite version_check (bytes 24..26) with a bad value.
    data[24] = 0xFF;
    data[25] = 0xFF;
    let err = VocFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidMagic {
            context: "VOC version check"
        }
    ));
}

/// A block whose declared size exceeds the remaining data is rejected.
#[test]
fn reject_truncated_block() {
    let mut data = build_voc(&[(BLOCK_SOUND_DATA, &[128, 0, 0xAA])]);
    // Inflate the block size in the u24 field so it exceeds available data.
    // Block size is at offset 27..30 (after header byte at 26).
    data[27] = 0xFF;
    data[28] = 0xFF;
    data[29] = 0x0F; // ~1 MB, way beyond the buffer
    let err = VocFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// A data_offset that points beyond the buffer is rejected.
#[test]
fn reject_data_offset_beyond_buffer() {
    let mut data = build_voc(&[]);
    // Set data_offset to a huge value.
    data[20] = 0xFF;
    data[21] = 0xFF;
    let err = VocFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidOffset { .. }));
}

// ─── Adversarial inputs ──────────────────────────────────────────────────────

/// All-0xFF input must not panic (should return an error).
#[test]
fn adversarial_all_ff() {
    let data = vec![0xFF; 256];
    let result = VocFile::parse(&data);
    assert!(result.is_err());
}

/// All-0x00 input must not panic (should return an error — the magic is wrong).
#[test]
fn adversarial_all_zero() {
    let data = vec![0x00; 256];
    let result = VocFile::parse(&data);
    assert!(result.is_err());
}

/// A minimal valid VOC (just header + terminator at exactly 27 bytes)
/// parses without error.
#[test]
fn minimal_valid_voc() {
    let mut data = Vec::new();
    data.extend_from_slice(b"Creative Voice File\x1a");
    data.extend_from_slice(&26u16.to_le_bytes());
    data.extend_from_slice(&0x010Au16.to_le_bytes());
    let check = (!0x010Au16).wrapping_add(0x1234);
    data.extend_from_slice(&check.to_le_bytes());
    data.push(0); // terminator
    assert_eq!(data.len(), 27);

    let voc = VocFile::parse(&data).unwrap();
    assert_eq!(voc.blocks().len(), 0);
    assert_eq!(voc.version(), (1, 10));
}
