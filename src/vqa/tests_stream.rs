// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

fn write_u32_be(buf: &mut [u8], offset: usize, value: u32) {
    if let Some(dst) = buf.get_mut(offset..offset.saturating_add(4)) {
        dst.copy_from_slice(&value.to_be_bytes());
    }
}

fn write_u16_le(buf: &mut [u8], offset: usize, value: u16) {
    if let Some(dst) = buf.get_mut(offset..offset.saturating_add(2)) {
        dst.copy_from_slice(&value.to_le_bytes());
    }
}

fn build_vqhd(num_frames: u16) -> [u8; 42] {
    let mut hd = [0u8; 42];
    write_u16_le(&mut hd, 0, 2);
    write_u16_le(&mut hd, 4, num_frames);
    write_u16_le(&mut hd, 6, 320);
    write_u16_le(&mut hd, 8, 200);
    if let Some(slot) = hd.get_mut(10) {
        *slot = 4;
    }
    if let Some(slot) = hd.get_mut(11) {
        *slot = 2;
    }
    if let Some(slot) = hd.get_mut(12) {
        *slot = 15;
    }
    hd
}

fn build_vqa_with_finf(num_frames: u16) -> Vec<u8> {
    let vqhd = build_vqhd(num_frames);
    let finf_size = (num_frames as usize) * 4;
    let form_data_size = 4 + (8 + vqhd.len()) + (8 + finf_size);
    let total = 8 + form_data_size;
    let mut buf = vec![0u8; total];

    if let Some(dst) = buf.get_mut(0..4) {
        dst.copy_from_slice(b"FORM");
    }
    write_u32_be(&mut buf, 4, form_data_size as u32);
    if let Some(dst) = buf.get_mut(8..12) {
        dst.copy_from_slice(b"WVQA");
    }

    let mut pos = 12usize;
    if let Some(dst) = buf.get_mut(pos..pos.saturating_add(4)) {
        dst.copy_from_slice(b"VQHD");
    }
    write_u32_be(&mut buf, pos + 4, vqhd.len() as u32);
    if let Some(dst) = buf.get_mut(pos + 8..pos + 8 + vqhd.len()) {
        dst.copy_from_slice(&vqhd);
    }
    pos = pos.saturating_add(8 + vqhd.len());

    if let Some(dst) = buf.get_mut(pos..pos.saturating_add(4)) {
        dst.copy_from_slice(b"FINF");
    }
    write_u32_be(&mut buf, pos + 4, finf_size as u32);
    let data_start = pos + 8;
    for i in 0..num_frames as usize {
        let offset = data_start + i * 4;
        if let Some(dst) = buf.get_mut(offset..offset.saturating_add(4)) {
            dst.copy_from_slice(&((i as u32) * 100).to_le_bytes());
        }
    }

    buf
}

#[test]
fn stream_reads_header_and_finf() {
    let data = build_vqa_with_finf(5);
    let cursor = std::io::Cursor::new(data);
    let mut stream = VqaStream::open(cursor).unwrap();

    let first = stream.next_chunk().unwrap().unwrap();
    assert_eq!(&first.fourcc, b"VQHD");
    assert_eq!(stream.header().map(|h| h.num_frames), Some(5));

    let second = stream.next_chunk().unwrap().unwrap();
    assert_eq!(&second.fourcc, b"FINF");
    assert_eq!(stream.frame_index().map(|f| f.len()), Some(5));
    assert_eq!(
        stream.frame_index().and_then(|f| f.get(4)).copied(),
        Some(400)
    );
    assert!(stream.next_chunk().unwrap().is_none());
}

#[test]
fn stream_next_chunk_owned_detaches_from_reused_buffer() {
    let data = build_vqa_with_finf(2);
    let cursor = std::io::Cursor::new(data);
    let mut stream = VqaStream::open(cursor).unwrap();

    let first = stream.next_chunk_owned().unwrap().unwrap();
    let second = stream.next_chunk().unwrap().unwrap();

    assert_eq!(&first.fourcc, b"VQHD");
    assert_eq!(first.data.len(), 42);
    assert_eq!(&second.fourcc, b"FINF");
    assert_eq!(second.data.len(), 8);
    assert_eq!(first.data.get(4..6), Some(&2u16.to_le_bytes()[..]));
}

#[test]
fn try_resync_finds_next_valid_chunk_header() {
    // Build a VQA with VQHD, then inject garbage, then a valid JUNK chunk.
    let vqhd = build_vqhd(1);
    let junk_payload = [0xAA; 8];
    let garbage = [0xFF; 10];

    // Total form body: WVQA(4) + VQHD chunk(8+42) + garbage(10) + JUNK chunk(8+8)
    let form_body_size = 4 + (8 + vqhd.len()) + garbage.len() + (8 + junk_payload.len());

    let mut buf = Vec::new();
    buf.extend_from_slice(b"FORM");
    buf.extend_from_slice(&(form_body_size as u32).to_be_bytes());
    buf.extend_from_slice(b"WVQA");
    // VQHD chunk
    buf.extend_from_slice(b"VQHD");
    buf.extend_from_slice(&(vqhd.len() as u32).to_be_bytes());
    buf.extend_from_slice(&vqhd);
    // Garbage (simulates corruption replacing chunk header)
    buf.extend_from_slice(&garbage);
    // Valid JUNK chunk
    buf.extend_from_slice(b"JUNK");
    buf.extend_from_slice(&(junk_payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(&junk_payload);

    let cursor = std::io::Cursor::new(buf);
    let mut stream = VqaStream::open(cursor).unwrap();

    // Read VQHD successfully.
    let first = stream.next_chunk().unwrap().unwrap();
    assert_eq!(&first.fourcc, b"VQHD");

    // Next read would fail on the garbage. Use resync instead.
    let found = stream.try_resync().unwrap();
    assert!(found, "try_resync should find the JUNK chunk header");
}

#[test]
fn try_resync_returns_false_at_end_of_stream() {
    let data = build_vqa_with_finf(1);
    let cursor = std::io::Cursor::new(data);
    let mut stream = VqaStream::open(cursor).unwrap();

    // Consume all chunks.
    while stream.next_chunk().unwrap().is_some() {}

    // At end of stream, resync should return false.
    let found = stream.try_resync().unwrap();
    assert!(!found, "try_resync should return false at end of stream");
}
