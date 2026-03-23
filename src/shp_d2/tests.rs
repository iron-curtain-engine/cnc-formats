// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;
use crate::error::Error;

// ── Test Helpers ─────────────────────────────────────────────────────────────

/// Builds a minimal valid Dune II SHP file with uncompressed frames.
///
/// Each entry in `frames` is `(width, height, flags_extra, pixel_data)`.
/// The `FLAG_UNCOMPRESSED` bit is always set; `flags_extra` allows adding
/// additional flag bits (e.g. `FLAG_HAS_REMAP`).
fn build_shp_d2_uncompressed(frames: &[(u16, u8, u16, &[u8])]) -> Vec<u8> {
    build_shp_d2_uncompressed_with_remap(frames, &[])
}

/// Like `build_shp_d2_uncompressed` but allows attaching remap tables.
///
/// `remaps` is indexed by frame index; if present for a frame, 16 bytes
/// are inserted between the frame header and the pixel data.
fn build_shp_d2_uncompressed_with_remap(
    frames: &[(u16, u8, u16, &[u8])],
    remaps: &[Option<[u8; 16]>],
) -> Vec<u8> {
    let num_frames = frames.len();
    let header_size = 2 + (num_frames + 2) * 4; // num_frames:2 + offsets

    let mut out = Vec::new();
    out.extend_from_slice(&(num_frames as u16).to_le_bytes());

    // Calculate offsets for each frame.
    let mut offset = header_size;
    let mut offsets = Vec::new();
    for (i, (w, h, flags_extra, _pixels)) in frames.iter().enumerate() {
        offsets.push(offset as u32);
        let has_remap = (*flags_extra & FLAG_HAS_REMAP != 0)
            || remaps.get(i).and_then(|r| r.as_ref()).is_some();
        let data_size = *w as usize * *h as usize;
        let frame_size =
            FRAME_HEADER_SIZE + if has_remap { REMAP_TABLE_SIZE } else { 0 } + data_size;
        offset += frame_size;
    }
    offsets.push(offset as u32); // sentinel 1
    offsets.push(offset as u32); // sentinel 2

    // Write offset table.
    for o in &offsets {
        out.extend_from_slice(&o.to_le_bytes());
    }

    // Write frame data.
    for (i, (w, h, flags_extra, pixels)) in frames.iter().enumerate() {
        let remap = remaps.get(i).and_then(|r| *r);
        let has_remap = (*flags_extra & FLAG_HAS_REMAP != 0) || remap.is_some();
        let flags: u16 = FLAG_UNCOMPRESSED | if has_remap { FLAG_HAS_REMAP } else { 0 };
        let data_size = *w as usize * *h as usize;
        let file_size =
            FRAME_HEADER_SIZE + if has_remap { REMAP_TABLE_SIZE } else { 0 } + data_size;

        // Frame header (10 bytes).
        out.extend_from_slice(&flags.to_le_bytes()); // flags
        out.push(*h); // slices (== height)
        out.extend_from_slice(&w.to_le_bytes()); // width
        out.push(*h); // height
        out.extend_from_slice(&(file_size as u16).to_le_bytes()); // file_size
        out.extend_from_slice(&(data_size as u16).to_le_bytes()); // data_size

        // Optional remap table.
        if has_remap {
            if let Some(table) = remap {
                out.extend_from_slice(&table);
            } else {
                out.extend_from_slice(&[0u8; REMAP_TABLE_SIZE]);
            }
        }

        // Pixel data.
        out.extend_from_slice(&pixels[..data_size]);
    }

    out
}

// ── Basic Functionality ──────────────────────────────────────────────────────

/// Parse a valid single-frame uncompressed Dune II SHP file.
#[test]
fn parse_valid_uncompressed() {
    let pixels = [0xAAu8; 4 * 3]; // 4 wide, 3 tall
    let data = build_shp_d2_uncompressed(&[(4, 3, 0, &pixels)]);
    let shp = ShpD2File::parse(&data).unwrap();

    assert_eq!(shp.frame_count(), 1);
    let frame = shp.frame(0).unwrap();
    assert_eq!(frame.width, 4);
    assert_eq!(frame.height, 3);
    assert_eq!(frame.pixels.len(), 12);
    assert!(frame.pixels.iter().all(|&p| p == 0xAA));
    assert!(frame.remap.is_none());
}

/// Parse multiple frames with different sizes.
#[test]
fn parse_multiple_frames() {
    let px1 = [0x11u8; 2 * 2];
    let px2 = [0x22u8; 3 * 4];
    let px3 = [0x33u8; 5];
    let data = build_shp_d2_uncompressed(&[(2, 2, 0, &px1), (3, 4, 0, &px2), (5, 1, 0, &px3)]);
    let shp = ShpD2File::parse(&data).unwrap();

    assert_eq!(shp.frame_count(), 3);

    let f0 = shp.frame(0).unwrap();
    assert_eq!(f0.width, 2);
    assert_eq!(f0.height, 2);
    assert!(f0.pixels.iter().all(|&p| p == 0x11));

    let f1 = shp.frame(1).unwrap();
    assert_eq!(f1.width, 3);
    assert_eq!(f1.height, 4);
    assert!(f1.pixels.iter().all(|&p| p == 0x22));

    let f2 = shp.frame(2).unwrap();
    assert_eq!(f2.width, 5);
    assert_eq!(f2.height, 1);
    assert!(f2.pixels.iter().all(|&p| p == 0x33));
}

/// Parse a frame with FLAG_HAS_REMAP and verify the remap table.
#[test]
fn parse_with_remap() {
    let pixels = [0xBBu8; 3 * 2];
    let mut remap_table = [0u8; 16];
    for (i, b) in remap_table.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(17); // 0x00, 0x11, 0x22, ...
    }
    let data = build_shp_d2_uncompressed_with_remap(
        &[(3, 2, FLAG_HAS_REMAP, &pixels)],
        &[Some(remap_table)],
    );
    let shp = ShpD2File::parse(&data).unwrap();
    assert_eq!(shp.frame_count(), 1);

    let frame = shp.frame(0).unwrap();
    assert_eq!(frame.width, 3);
    assert_eq!(frame.height, 2);
    assert!(frame.pixels.iter().all(|&p| p == 0xBB));
    assert_eq!(frame.remap, Some(remap_table));
    assert!(frame.flags & FLAG_HAS_REMAP != 0);
}

/// Verify `frame()` and `frame_count()` accessors.
#[test]
fn frame_access() {
    let px = [0x42u8; 2 * 2];
    let data = build_shp_d2_uncompressed(&[(2, 2, 0, &px), (2, 2, 0, &px)]);
    let shp = ShpD2File::parse(&data).unwrap();

    assert_eq!(shp.frame_count(), 2);
    assert!(shp.frame(0).is_some());
    assert!(shp.frame(1).is_some());
    assert!(shp.frame(2).is_none());

    let all = shp.frames();
    assert_eq!(all.len(), 2);
}

// ── Error Paths ──────────────────────────────────────────────────────────────

/// Input shorter than 2 bytes (cannot read num_frames) is rejected.
#[test]
fn reject_truncated_header() {
    let err = ShpD2File::parse(&[0u8; 1]).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// An offset pointing past the end of data is rejected.
#[test]
fn reject_offset_out_of_bounds() {
    // Build a valid 1-frame file, then corrupt the first offset to point
    // way past the end.
    let pixels = [0xAAu8; 2 * 2];
    let mut data = build_shp_d2_uncompressed(&[(2, 2, 0, &pixels)]);
    // Offset table starts at byte 2; first frame offset is at bytes 2..6.
    let bad_offset = (data.len() as u32) + 1000;
    data[2..6].copy_from_slice(&bad_offset.to_le_bytes());
    let err = ShpD2File::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidOffset { .. }));
}

/// Frame count exceeding MAX_FRAMES is rejected.
#[test]
fn reject_too_many_frames() {
    let mut data = vec![0u8; 2 + (MAX_FRAMES + 3) * 4 + 10];
    let bad_count = (MAX_FRAMES as u16) + 1;
    data[0..2].copy_from_slice(&bad_count.to_le_bytes());
    let err = ShpD2File::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "SHP_D2 header",
            ..
        }
    ));
}

/// Frame with width=0 is rejected.
#[test]
fn reject_zero_width() {
    let pixels = [0u8; 0]; // won't be used
    let mut data = build_shp_d2_uncompressed(&[(1, 1, 0, &[0x42])]);
    // Find the frame header: offset table starts at 2, has 3 entries (4 bytes each).
    let frame_offset_pos = 2usize; // first offset entry
    let frame_offset = u32::from_le_bytes([
        data[frame_offset_pos],
        data[frame_offset_pos + 1],
        data[frame_offset_pos + 2],
        data[frame_offset_pos + 3],
    ]) as usize;
    // Width is at frame_offset + 3 (u16 LE).
    data[frame_offset + 3] = 0;
    data[frame_offset + 4] = 0;
    let err = ShpD2File::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidMagic {
            context: "SHP_D2 frame"
        }
    ));
    let _ = pixels;
}

/// Frame with height=0 is rejected.
#[test]
fn reject_zero_height() {
    let mut data = build_shp_d2_uncompressed(&[(1, 1, 0, &[0x42])]);
    let frame_offset_pos = 2usize;
    let frame_offset = u32::from_le_bytes([
        data[frame_offset_pos],
        data[frame_offset_pos + 1],
        data[frame_offset_pos + 2],
        data[frame_offset_pos + 3],
    ]) as usize;
    // Height is at frame_offset + 5 (u8).
    data[frame_offset + 5] = 0;
    let err = ShpD2File::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidMagic {
            context: "SHP_D2 frame"
        }
    ));
}

// ── Security Edge Cases (V38) ────────────────────────────────────────────────

/// `ShpD2File::parse` on 256 bytes of `0xFF` must not panic.
#[test]
fn adversarial_all_ff() {
    let data = vec![0xFFu8; 256];
    let _ = ShpD2File::parse(&data);
}

/// `ShpD2File::parse` on 256 bytes of `0x00` must not panic.
#[test]
fn adversarial_all_zero() {
    let data = vec![0x00u8; 256];
    let _ = ShpD2File::parse(&data);
}
