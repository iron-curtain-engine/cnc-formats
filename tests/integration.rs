// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Integration tests — cross-module workflows that exercise the full parsing
//! pipeline, not just individual format parsers in isolation.

extern crate alloc;

use cnc_formats::aud::{self, AudFile, AUD_FLAG_16BIT, SCOMP_WESTWOOD};
use cnc_formats::fnt::FntFile;
use cnc_formats::lcw;
use cnc_formats::mix::{self, MixArchive};
use cnc_formats::pal::{Palette, PALETTE_BYTES};
use cnc_formats::shp::ShpFile;
use cnc_formats::tmp;
use cnc_formats::vqa::VqaFile;
use cnc_formats::wsa::WsaFile;
use cnc_formats::Error;

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Build a minimal AUD file (header + compressed payload).
fn build_aud_bytes(compressed: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&22050u16.to_le_bytes()); // sample_rate
    v.extend_from_slice(&(compressed.len() as u32).to_le_bytes()); // compressed_size
    v.extend_from_slice(&0u32.to_le_bytes()); // uncompressed_size (unused for parsing)
    v.push(AUD_FLAG_16BIT); // flags
    v.push(SCOMP_WESTWOOD); // compression
    v.extend_from_slice(compressed);
    v
}

/// Build a minimal 1-frame uncompressed SHP file.
fn build_shp_bytes(width: u16, height: u16, pixels: &[u8]) -> Vec<u8> {
    let frame_count: u16 = 1;
    let largest = pixels.len() as u16;
    let flags: u16 = 0;

    let offset_table_size = (frame_count as usize + 1) * 4;
    let data_start = 14 + offset_table_size;

    let mut out = Vec::new();
    // Header: 7 × u16
    out.extend_from_slice(&frame_count.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // x
    out.extend_from_slice(&0u16.to_le_bytes()); // y
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&largest.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    // Offset table: 2 entries with OFFSET_UNCOMPRESSED_FLAG (0x8000_0000)
    let off0 = data_start as u32 | 0x8000_0000;
    let off1 = (data_start + pixels.len()) as u32 | 0x8000_0000;
    out.extend_from_slice(&off0.to_le_bytes());
    out.extend_from_slice(&off1.to_le_bytes());
    // Frame data
    out.extend_from_slice(pixels);
    out
}

/// Build a 1-frame SHP with an LCW-compressed frame (no uncompressed flag).
fn build_shp_compressed_bytes(width: u16, height: u16, lcw_data: &[u8]) -> Vec<u8> {
    let frame_count: u16 = 1;
    let largest = lcw_data.len() as u16;
    let flags: u16 = 0;

    let offset_table_size = (frame_count as usize + 1) * 4;
    let data_start = 14 + offset_table_size;

    let mut out = Vec::new();
    out.extend_from_slice(&frame_count.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&largest.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    // Offsets WITHOUT uncompressed flag → LCW-compressed
    let off0 = data_start as u32;
    let off1 = (data_start + lcw_data.len()) as u32;
    out.extend_from_slice(&off0.to_le_bytes());
    out.extend_from_slice(&off1.to_le_bytes());
    out.extend_from_slice(lcw_data);
    out
}

/// Build a basic MIX archive from (filename, data) pairs.
fn build_mix_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut entries: Vec<(mix::MixCrc, &[u8])> = files
        .iter()
        .map(|(name, data)| (mix::crc(name), *data))
        .collect();
    entries.sort_by_key(|(c, _)| *c);

    let count = entries.len() as u16;
    let mut offsets = Vec::with_capacity(entries.len());
    let mut cur = 0u32;
    for (_, data) in &entries {
        offsets.push(cur);
        cur += data.len() as u32;
    }
    let data_size = cur;

    let mut out = Vec::new();
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&data_size.to_le_bytes());
    for (i, (c, data)) in entries.iter().enumerate() {
        out.extend_from_slice(&c.to_raw().to_le_bytes());
        out.extend_from_slice(&offsets[i].to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    }
    for (_, data) in &entries {
        out.extend_from_slice(data);
    }
    out
}

// ─── Cross-module integration tests ──────────────────────────────────────────

/// MIX archive → extract SHP → parse sprite frames.
///
/// Why: the primary use-case of this crate is extracting assets from a MIX
/// and parsing them.  This test proves the SHP bytes survive the MIX
/// round-trip and produce the same frame data.
#[test]
fn mix_extract_then_parse_shp() {
    let pixels: Vec<u8> = (0u8..16).collect();
    let shp_bytes = build_shp_bytes(4, 4, &pixels);
    let mix_bytes = build_mix_bytes(&[("UNIT.SHP", &shp_bytes)]);

    let archive = MixArchive::parse(&mix_bytes).unwrap();
    let extracted = archive
        .get("UNIT.SHP")
        .expect("UNIT.SHP should exist in archive");
    let shp = ShpFile::parse(extracted).unwrap();

    assert_eq!(shp.frame_count(), 1);
    assert_eq!(shp.header.width, 4);
    assert_eq!(shp.header.height, 4);
    assert_eq!(shp.frames[0].data, pixels);
}

/// MIX archive → extract PAL → parse palette and convert to 8-bit RGB.
///
/// Why: palettes are stored inside MIX archives in actual games.  This
/// exercises the full extraction → parse → `to_rgb8_array` pipeline.
#[test]
fn mix_extract_then_parse_pal() {
    let mut pal_bytes = vec![0u8; PALETTE_BYTES];
    pal_bytes[0] = 63; // color 0 red = max VGA
    let mix_bytes = build_mix_bytes(&[("TEMPERAT.PAL", &pal_bytes)]);

    let archive = MixArchive::parse(&mix_bytes).unwrap();
    let extracted = archive.get("TEMPERAT.PAL").unwrap();
    let palette = Palette::parse(extracted).unwrap();

    assert_eq!(palette.colors[0].r, 63);
    assert_eq!(palette.to_rgb8_array()[0], [252, 0, 0]);
}

/// MIX archive → extract AUD → parse header → decode Westwood ADPCM.
///
/// Why: exercises the deepest integration path — three modules in sequence.
/// The AUD header's `compressed_data` slice must correctly index into the
/// MIX-extracted payload, and the ADPCM decoder must produce the expected
/// number of samples.
#[test]
fn mix_extract_then_parse_and_decode_aud() {
    let compressed = vec![0x77u8; 10]; // 10 bytes → 20 samples
    let aud_bytes = build_aud_bytes(&compressed);
    let mix_bytes = build_mix_bytes(&[("SPEECH.AUD", &aud_bytes)]);

    let archive = MixArchive::parse(&mix_bytes).unwrap();
    let extracted = archive.get("SPEECH.AUD").unwrap();
    let aud = AudFile::parse(extracted).unwrap();

    assert_eq!(aud.header.sample_rate, 22050);
    assert_eq!(aud.header.compression, SCOMP_WESTWOOD);

    let samples = aud::decode_adpcm(aud.compressed_data, aud.header.is_stereo(), 0);
    assert_eq!(samples.len(), 20);
}

/// SHP with LCW-compressed frame → parse → `pixels()` decompresses.
///
/// Why: LCW decompression inside SHP is the default (uncompressed is the
/// exception).  This test verifies that the `ShpFrame::pixels()` bridge
/// to `lcw::decompress` produces the correct pixel buffer.
///
/// How: the LCW stream is a single long-fill command producing 16 bytes
/// of `0xAB`, matching the 4×4 pixel area.
#[test]
fn shp_lcw_decompress_pipeline() {
    // LCW stream: long fill 16 bytes of 0xAB, then end marker
    let lcw_data: Vec<u8> = vec![0xFE, 0x10, 0x00, 0xAB, 0x80];
    let shp_bytes = build_shp_compressed_bytes(4, 4, &lcw_data);
    let shp = ShpFile::parse(&shp_bytes).unwrap();

    assert_eq!(shp.frame_count(), 1);
    assert!(!shp.frames[0].is_uncompressed);

    let pixels = shp.frames[0].pixels(16).unwrap();
    assert_eq!(pixels, vec![0xABu8; 16]);
}

/// Standalone LCW: a known multi-command stream decompresses correctly.
///
/// Why: verifies that command chaining (medium-literal then long-fill)
/// produces the expected concatenated output, independent of any SHP
/// container.  This exercises the public `lcw::decompress` API directly.
#[test]
fn lcw_known_stream_decompresses() {
    // Medium literal (3 bytes) + long fill (4 × 0xFF) + end marker
    let stream: Vec<u8> = vec![
        0x83, 0x01, 0x02, 0x03, // medium literal: 3 bytes
        0xFE, 0x04, 0x00, 0xFF, // long fill: 4 × 0xFF
        0x80, // end
    ];
    let output = lcw::decompress(&stream, 7).unwrap();
    assert_eq!(output, vec![0x01, 0x02, 0x03, 0xFF, 0xFF, 0xFF, 0xFF]);
}

/// Full AUD workflow: parse header → decode ADPCM → verify determinism.
///
/// Why: the ADPCM decoder carries internal state (`sample`, `step_index`);
/// decoding the same payload twice must yield bit-identical PCM output.
/// This catches any accidental state leaking between calls.
#[test]
fn aud_parse_decode_deterministic() {
    let compressed = vec![0x11u8; 20];
    let aud_bytes = build_aud_bytes(&compressed);

    let aud = AudFile::parse(&aud_bytes).unwrap();
    let samples_a = aud::decode_adpcm(aud.compressed_data, aud.header.is_stereo(), 0);
    let samples_b = aud::decode_adpcm(aud.compressed_data, aud.header.is_stereo(), 0);

    assert_eq!(samples_a.len(), 40); // 20 bytes × 2 nibbles
    assert_eq!(samples_a, samples_b);
}

/// MIX archive with SHP + PAL + AUD — all three parse after extraction.
///
/// Why: real MIX archives contain mixed file types.  This test proves that
/// three different parsers can independently consume their data slices
/// from the same archive without cross-contamination.
#[test]
fn mix_multi_file_type_archive() {
    let pixels: Vec<u8> = vec![0u8; 4];
    let shp_bytes = build_shp_bytes(2, 2, &pixels);
    let pal_bytes = vec![0u8; PALETTE_BYTES];
    let compressed = vec![0x00u8; 5];
    let aud_bytes = build_aud_bytes(&compressed);

    let mix_bytes = build_mix_bytes(&[
        ("UNIT.SHP", &shp_bytes),
        ("TEMPERAT.PAL", &pal_bytes),
        ("SPEECH.AUD", &aud_bytes),
    ]);

    let archive = MixArchive::parse(&mix_bytes).unwrap();
    assert_eq!(archive.file_count(), 3);

    // Each file is independently parseable after extraction.
    ShpFile::parse(archive.get("UNIT.SHP").unwrap()).unwrap();
    Palette::parse(archive.get("TEMPERAT.PAL").unwrap()).unwrap();
    AudFile::parse(archive.get("SPEECH.AUD").unwrap()).unwrap();
}

// ── Error Display verification ───────────────────────────────────────────────

/// `Error::CrcMismatch` Display output contains hex-formatted CRC values.
///
/// Why: the `CrcMismatch` variant is defined in `error.rs` for future use
/// (e.g. MIX checksum validation).  Its Display impl must render the
/// expected/found values in `0x{:08X}` format so callers can diagnose
/// which CRC was wrong.  Testing it now prevents silent regressions.
#[test]
fn crc_mismatch_display_contains_hex_values() {
    let err = Error::CrcMismatch {
        expected: 0xDEAD_BEEF,
        found: 0xCAFE_BABE,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("DEADBEEF"),
        "should contain expected CRC: {msg}"
    );
    assert!(msg.contains("CAFEBABE"), "should contain found CRC: {msg}");
}

/// `Error::CrcMismatch` structured fields carry the exact values.
///
/// Why: callers match on the structured fields for programmatic error
/// handling.  This test ensures the fields are not swapped or truncated.
#[test]
fn crc_mismatch_fields_are_correct() {
    let err = Error::CrcMismatch {
        expected: 0x1234_5678,
        found: 0x9ABC_DEF0,
    };
    match err {
        Error::CrcMismatch { expected, found } => {
            assert_eq!(expected, 0x1234_5678);
            assert_eq!(found, 0x9ABC_DEF0);
        }
        other => panic!("Expected CrcMismatch, got: {other}"),
    }
}

// ── New-module standalone parse tests ────────────────────────────────────────

/// VQA: parse a well-formed minimal VQA file and verify header fields.
///
/// Why: integration-level confidence that `VqaFile::parse` works
/// end-to-end with a complete FORM/WVQA envelope.
#[test]
fn vqa_parse_minimal_valid() {
    // Manually build a minimal valid VQA: FORM + WVQA + VQHD chunk
    let mut vqhd = [0u8; 42];
    // version = 2
    vqhd[0] = 2;
    vqhd[1] = 0;
    // num_frames = 5
    vqhd[4] = 5;
    vqhd[5] = 0;
    // width = 320 at offset 6
    vqhd[6] = 0x40;
    vqhd[7] = 0x01;
    // height = 200 at offset 8
    vqhd[8] = 0xC8;
    vqhd[9] = 0x00;

    let form_data_size = 4 + 8 + vqhd.len(); // "WVQA" + VQHD chunk
    let mut data = Vec::new();
    data.extend_from_slice(b"FORM");
    data.extend_from_slice(&(form_data_size as u32).to_be_bytes());
    data.extend_from_slice(b"WVQA");
    data.extend_from_slice(b"VQHD");
    data.extend_from_slice(&(vqhd.len() as u32).to_be_bytes());
    data.extend_from_slice(&vqhd);

    let vqa = VqaFile::parse(&data).unwrap();
    assert_eq!(vqa.header.num_frames, 5);
    assert_eq!(vqa.header.width, 320);
    assert_eq!(vqa.header.height, 200);
}

/// WSA: parse a well-formed minimal WSA file and verify header fields.
///
/// Why: integration-level confidence that `WsaFile::parse` works
/// end-to-end with a zero-frame animation (degenerate but valid).
#[test]
fn wsa_parse_minimal_valid() {
    // Minimal WSA: 0 frames, 64x48 dimensions, no-loop
    // Header: 14 bytes + 2 offsets (0 frames + 2 always) = 14 + 8 = 22 bytes
    let num_frames: u16 = 0;
    let width: u16 = 64;
    let height: u16 = 48;
    let num_offsets = (num_frames as usize) + 2;
    let offsets_size = num_offsets * 4;
    let header_plus_offsets = 14 + offsets_size;

    let mut data = Vec::new();
    data.extend_from_slice(&num_frames.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes()); // x
    data.extend_from_slice(&0u16.to_le_bytes()); // y
    data.extend_from_slice(&width.to_le_bytes());
    data.extend_from_slice(&height.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes()); // delta_buffer_size
                                                 // Offsets: both point to end (sentinel pattern for 0 frames)
    for _ in 0..num_offsets {
        data.extend_from_slice(&(header_plus_offsets as u32).to_le_bytes());
    }

    let wsa = WsaFile::parse(&data).unwrap();
    assert_eq!(wsa.header.num_frames, 0);
    assert_eq!(wsa.header.width, 64);
    assert_eq!(wsa.header.height, 48);
}

/// FNT: parse a well-formed minimal FNT file and verify glyph count.
///
/// Why: integration-level confidence that `FntFile::parse` works
/// end-to-end with a 256-glyph font.
#[test]
fn fnt_parse_minimal_valid() {
    // Minimal FNT: height=8, all glyphs width=1 (1 byte each)
    let height: u8 = 8;
    let glyph_w: u16 = 1;
    let bytes_per_col = 1usize; // ceil(8/8) = 1
    let glyph_size = (glyph_w as usize) * bytes_per_col;

    let mut data = Vec::new();
    // Header: 6 bytes — data_size(u16), height(u8), max_width(u8), unknown(u16)
    let data_size = (256 * glyph_size) as u16;
    data.extend_from_slice(&data_size.to_le_bytes()); // data_size
    data.push(height); // height (u8 at offset 2)
    data.push(glyph_w as u8); // max_width (u8 at offset 3)
    data.extend_from_slice(&0u16.to_le_bytes()); // unknown

    // Width table: 256 × u16, all = glyph_w
    for _ in 0..256 {
        data.extend_from_slice(&glyph_w.to_le_bytes());
    }
    // Offset table: 256 × u16, sequential offsets
    for i in 0..256u16 {
        let off = (i as usize) * glyph_size;
        data.extend_from_slice(&(off as u16).to_le_bytes());
    }
    // Glyph data: 256 × 1 byte each
    data.extend_from_slice(&vec![0xAAu8; 256 * glyph_size]);

    let fnt = FntFile::parse(&data).unwrap();
    assert_eq!(fnt.glyphs.len(), 256);
    assert_eq!(fnt.header.height, 8);
}

/// TMP (TD): parse a well-formed minimal terrain tile file.
///
/// Why: integration-level confidence that `TdTmpFile::parse` works
/// end-to-end with a 1×1 grid, single 24×24 tile.
#[test]
fn tmp_td_parse_minimal_valid() {
    // TD TMP header layout (20 bytes):
    // width(u16), height(u16), tile_count(u16), allocated(u16),
    // tile_w(u16), tile_h(u16), file_size(u32), image_start(u32)
    //
    // width/height = grid dimensions (1×1), tile_w/tile_h = 24×24
    let grid_w: u16 = 1;
    let grid_h: u16 = 1;
    let tile_w: u16 = 24;
    let tile_h: u16 = 24;
    let tile_count: u16 = 1;
    let grid_size = (grid_w as usize) * (grid_h as usize); // 1 byte icon map
    let tile_area = (tile_w as usize) * (tile_h as usize); // 576 bytes
    let map_end = 20 + grid_size; // 21
    let total = map_end + tile_area; // 597

    let mut data = vec![0u8; total];
    // Header fields
    data[0] = grid_w as u8;
    data[1] = (grid_w >> 8) as u8;
    data[2] = grid_h as u8;
    data[3] = (grid_h >> 8) as u8;
    data[4] = tile_count as u8;
    data[5] = (tile_count >> 8) as u8;
    data[6] = 0; // allocated
    data[7] = 0;
    data[8] = tile_w as u8;
    data[9] = (tile_w >> 8) as u8;
    data[10] = tile_h as u8;
    data[11] = (tile_h >> 8) as u8;
    let fs = total as u32;
    data[12] = (fs & 0xFF) as u8;
    data[13] = ((fs >> 8) & 0xFF) as u8;
    data[14] = ((fs >> 16) & 0xFF) as u8;
    data[15] = ((fs >> 24) & 0xFF) as u8;
    // image_start = 0 → tiles follow icon map immediately
    // Icon map at offset 20: single byte = 0 (tile index)

    let td = tmp::TdTmpFile::parse(&data).unwrap();
    assert_eq!(td.tiles.len(), 1);
    assert_eq!(td.header.tile_w, 24);
}

/// Adversarial: all new-module parsers on all-`0xFF` input must not panic.
///
/// Why (V38): each parser must handle worst-case inputs (maximised header
/// fields, offset overflow, huge declared sizes) without panic or OOM.
/// This is the integration-level counterpart of per-module adversarial
/// tests — it confirms the public API surface is safe.
#[test]
fn all_new_parsers_adversarial_all_ff() {
    let data = vec![0xFFu8; 512];
    let _ = VqaFile::parse(&data);
    let _ = WsaFile::parse(&data);
    let _ = FntFile::parse(&data);
    let _ = tmp::TdTmpFile::parse(&data);
    let _ = tmp::RaTmpFile::parse(&data);
}
