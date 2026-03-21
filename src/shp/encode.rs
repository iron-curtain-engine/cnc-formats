// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

/// Encodes palette-indexed pixel frames into a complete SHP file.
///
/// Each frame in `frames` must be exactly `width × height` bytes of
/// palette-indexed pixel data.  All frames are encoded as LCW keyframes
/// (`ShpFrameFormat::Lcw`, format code 0x80).
///
/// Returns the complete SHP file as `Vec<u8>` that [`ShpFile::parse`] can
/// round-trip.
///
/// # Errors
///
/// Returns [`Error::InvalidSize`] if any frame has the wrong number of pixels.
pub fn encode_frames(frames: &[&[u8]], width: u16, height: u16) -> Result<Vec<u8>, Error> {
    let pixel_count = (width as usize).saturating_mul(height as usize);
    for (i, frame) in frames.iter().enumerate() {
        if frame.len() != pixel_count {
            return Err(Error::InvalidSize {
                value: frame.len(),
                limit: pixel_count,
                context: "SHP frame pixel count mismatch",
            });
        }
        let _ = i;
    }

    let frame_count = frames.len() as u16;
    let total_entries = (frame_count as usize).saturating_add(EXTRA_OFFSET_ENTRIES);
    let offset_table_size = total_entries.saturating_mul(OFFSET_ENTRY_SIZE);
    let header_size = 14usize;

    let compressed: Vec<Vec<u8>> = frames.iter().map(|f| lcw::compress(f)).collect();

    let largest = compressed.iter().map(|c| c.len()).max().unwrap_or(0);
    let data_start = header_size.saturating_add(offset_table_size);

    let total_data: usize = compressed.iter().map(|c| c.len()).sum();
    let mut out = Vec::with_capacity(data_start.saturating_add(total_data));

    out.extend_from_slice(&frame_count.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&(largest as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());

    let mut file_offset = data_start as u32;
    for c in &compressed {
        let raw = ((ShpFrameFormat::Lcw as u32) << 24) | (file_offset & OFFSET_MASK);
        out.extend_from_slice(&raw.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        file_offset = file_offset.saturating_add(c.len() as u32);
    }

    let eof_raw = file_offset & OFFSET_MASK;
    out.extend_from_slice(&eof_raw.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());

    for c in &compressed {
        out.extend_from_slice(c);
    }

    Ok(out)
}

/// Builds a minimal SHP binary for cross-module testing.
///
/// Creates a `width × height` SHP with one LCW keyframe that fills all
/// pixels with `fill_value`.
#[cfg(all(test, feature = "convert"))]
pub(crate) fn build_test_shp_helper(width: u16, height: u16, fill_value: u8) -> Vec<u8> {
    let pixel_count = (width as usize) * (height as usize);
    let lcw = [
        0xFEu8,
        pixel_count as u8,
        (pixel_count >> 8) as u8,
        fill_value,
        0x80,
    ];
    let frame_count: u16 = 1;
    let total_entries = frame_count as usize + EXTRA_OFFSET_ENTRIES;
    let offset_table_size = total_entries * OFFSET_ENTRY_SIZE;
    let data_start = (14 + offset_table_size) as u32;

    let mut out = Vec::new();
    let push_u16 = |v: u16, buf: &mut Vec<u8>| buf.extend_from_slice(&v.to_le_bytes());
    push_u16(frame_count, &mut out);
    push_u16(0, &mut out);
    push_u16(0, &mut out);
    push_u16(width, &mut out);
    push_u16(height, &mut out);
    push_u16(lcw.len() as u16, &mut out);
    push_u16(0, &mut out);

    let raw = ((ShpFrameFormat::Lcw as u32) << 24) | (data_start & OFFSET_MASK);
    out.extend_from_slice(&raw.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    let eof = (data_start + lcw.len() as u32) & OFFSET_MASK;
    out.extend_from_slice(&eof.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());

    out.extend_from_slice(&lcw);
    out
}
