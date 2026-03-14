// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Integration tests — cross-module workflows that exercise the full parsing
//! pipeline, not just individual format parsers in isolation.

#[cfg(feature = "adl")]
use cnc_formats::adl::AdlFile;
use cnc_formats::aud::{self, AudFile, AUD_FLAG_16BIT, SCOMP_WESTWOOD};
use cnc_formats::fnt::FntFile;
use cnc_formats::ini::IniFile;
use cnc_formats::lcw;
#[cfg(feature = "midi")]
use cnc_formats::mid::MidFile;
#[cfg(feature = "miniyaml")]
use cnc_formats::miniyaml::MiniYamlDoc;
use cnc_formats::mix::{self, MixArchive};
use cnc_formats::pal::{Palette, PALETTE_BYTES};
use cnc_formats::shp::ShpFile;
use cnc_formats::tmp;
use cnc_formats::vqa::VqaFile;
use cnc_formats::wsa::WsaFile;
#[cfg(feature = "xmi")]
use cnc_formats::xmi::XmiFile;
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

/// Build a minimal 1-frame LCW SHP file using the canonical 8-byte offset
/// table format (frame_count + 2 entries × 8 bytes each).
fn build_shp_bytes(width: u16, height: u16, lcw_data: &[u8]) -> Vec<u8> {
    let frame_count: u16 = 1;
    let largest = lcw_data.len() as u16;
    let flags: u16 = 0;
    let total_entries = frame_count as usize + 2; // frame + EOF + zero-padding
    let offset_table_size = total_entries * 8;
    let data_start = (14 + offset_table_size) as u32;

    let mut out = Vec::new();
    // Header: 7 × u16
    out.extend_from_slice(&frame_count.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // x
    out.extend_from_slice(&0u16.to_le_bytes()); // y
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&largest.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    // Offset table: 3 × 8-byte entries (frame 0, EOF, zero-padding).
    // Frame 0: LCW format (0x80) in high byte, file offset in low 24 bits.
    let raw0 = (0x80u32 << 24) | (data_start & 0x00FF_FFFF);
    out.extend_from_slice(&raw0.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // ref_offset
    out.extend_from_slice(&0u16.to_le_bytes()); // ref_format
                                                // EOF sentinel: file offset = end of frame data.
    let raw_eof = (data_start + lcw_data.len() as u32) & 0x00FF_FFFF;
    out.extend_from_slice(&raw_eof.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    // Zero-padding entry.
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    // Frame data.
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
    // LCW fill: 16 bytes of 0xAA + end marker.
    let lcw_data: Vec<u8> = vec![0xFE, 0x10, 0x00, 0xAA, 0x80];
    let shp_bytes = build_shp_bytes(4, 4, &lcw_data);
    let mix_bytes = build_mix_bytes(&[("UNIT.SHP", &shp_bytes)]);

    let archive = MixArchive::parse(&mix_bytes).unwrap();
    let extracted = archive
        .get("UNIT.SHP")
        .expect("UNIT.SHP should exist in archive");
    let shp = ShpFile::parse(extracted).unwrap();

    assert_eq!(shp.frame_count(), 1);
    assert_eq!(shp.header.width, 4);
    assert_eq!(shp.header.height, 4);
    assert_eq!(shp.frames[0].data, lcw_data.as_slice());
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
    let shp_bytes = build_shp_bytes(4, 4, &lcw_data);
    let shp = ShpFile::parse(&shp_bytes).unwrap();

    assert_eq!(shp.frame_count(), 1);

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
    let lcw_data: Vec<u8> = vec![0xFE, 0x04, 0x00, 0x00, 0x80]; // fill 4 bytes of 0x00
    let shp_bytes = build_shp_bytes(2, 2, &lcw_data);
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
/// Why: callers can surface `CrcMismatch` directly in diagnostics, so its
/// Display impl must render the expected/found values in `0x{:08X}` format.
/// Testing it here prevents silent formatting regressions.
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
    let _header_plus_offsets = 14 + offsets_size;

    let mut data = Vec::new();
    data.extend_from_slice(&num_frames.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes()); // x
    data.extend_from_slice(&0u16.to_le_bytes()); // y
    data.extend_from_slice(&width.to_le_bytes());
    data.extend_from_slice(&height.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes()); // largest_frame_size
    data.extend_from_slice(&0u16.to_le_bytes()); // flags (no palette)
                                                 // Offsets: both 0 (sentinel pattern for 0 frames).
                                                 // Offsets are relative to data area, not file start.
    for _ in 0..num_offsets {
        data.extend_from_slice(&0u32.to_le_bytes());
    }

    let wsa = WsaFile::parse(&data).unwrap();
    assert_eq!(wsa.header.num_frames, 0);
    assert_eq!(wsa.header.width, 64);
    assert_eq!(wsa.header.height, 48);
}

/// FNT: parse a well-formed minimal FNT file and verify glyph count.
///
/// Why: integration-level confidence that `FntFile::parse` works
/// end-to-end with the canonical 20-byte block-offset header format.
#[test]
fn fnt_parse_minimal_valid() {
    // Minimal FNT: max_height=8, num_chars=256, only glyph 0 has width=2.
    let num_chars: u16 = 256;
    let nc = num_chars as usize;
    let glyph_w: u8 = 2;
    let data_rows: u8 = 8;
    // 4bpp: ceil(2/2) = 1 byte per row, 8 rows = 8 bytes.
    let glyph_size = data_rows as usize;

    // Block layout after 20-byte header.
    let offset_table_start = 20usize;
    let width_table_start = offset_table_start + nc * 2;
    let height_table_start = width_table_start + nc;
    let data_area_start = height_table_start + nc * 2;
    let total = data_area_start + glyph_size;

    let mut data = vec![0u8; total];
    // Header.
    data[0..2].copy_from_slice(&(total as u16).to_le_bytes()); // FontLength
    data[2] = 0; // compress
    data[3] = 5; // data_blocks
    data[4..6].copy_from_slice(&0x0010u16.to_le_bytes()); // InfoBlockOffset
    data[6..8].copy_from_slice(&(offset_table_start as u16).to_le_bytes());
    data[8..10].copy_from_slice(&(width_table_start as u16).to_le_bytes());
    data[10..12].copy_from_slice(&(data_area_start as u16).to_le_bytes());
    data[12..14].copy_from_slice(&(height_table_start as u16).to_le_bytes());
    data[14..16].copy_from_slice(&0x1012u16.to_le_bytes()); // UnknownConst
    data[16] = 0; // pad
    data[17] = (num_chars - 1) as u8; // CharCount
    data[18] = 8; // MaxHeight
    data[19] = glyph_w; // MaxWidth

    // Width table: glyph 0 = 2, rest = 0.
    data[width_table_start] = glyph_w;

    // Height table: glyph 0 → y_offset=0, data_rows=8.
    data[height_table_start..height_table_start + 2]
        .copy_from_slice(&((data_rows as u16) << 8).to_le_bytes());

    // Offset table: glyph 0 → data_area_start.
    data[offset_table_start..offset_table_start + 2]
        .copy_from_slice(&(data_area_start as u16).to_le_bytes());

    // Glyph data: fill with 0xAA.
    for b in data[data_area_start..].iter_mut() {
        *b = 0xAA;
    }

    let fnt = FntFile::parse(&data).unwrap();
    assert_eq!(fnt.glyphs.len(), 256);
    assert_eq!(fnt.header.max_height, 8);
    assert_eq!(fnt.header.num_chars, 256);
}

/// TMP (TD): parse a well-formed minimal terrain tile file.
///
/// Why: integration-level confidence that `TdTmpFile::parse` works
/// end-to-end with a 1×1 grid, single 24×24 tile.
#[test]
fn tmp_td_parse_minimal_valid() {
    // TD TMP: IControl_Type header (32 bytes) + map data + tile data.
    // width(u16), height(u16), tile_count(u16), allocated(u16),
    // tile_w(u16), tile_h(u16), file_size(u32), image_start(u32),
    // + 4 more u32 fields (palettes, remaps, trans_flag, color_map)
    //
    // width/height = grid dimensions (1×1), tile_w/tile_h = 24×24
    // Single 24×24 tile with map_offset and icons_offset pointing to
    // inline data right after the header.
    let icon_w: u16 = 24;
    let icon_h: u16 = 24;
    let count: u16 = 1;
    let tile_area = (icon_w as usize) * (icon_h as usize); // 576 bytes
    let map_start: u32 = 32; // right after header
    let icons_start: u32 = 33; // right after 1-byte map
    let total = icons_start as usize + tile_area; // 609

    let mut data = vec![0u8; total];
    // IControl_Type header (32 bytes).
    data[0..2].copy_from_slice(&icon_w.to_le_bytes()); // icon_width
    data[2..4].copy_from_slice(&icon_h.to_le_bytes()); // icon_height
    data[4..6].copy_from_slice(&count.to_le_bytes()); // count
    data[6..8].copy_from_slice(&count.to_le_bytes()); // allocated
    data[8..12].copy_from_slice(&(total as u32).to_le_bytes()); // size
    data[12..16].copy_from_slice(&icons_start.to_le_bytes()); // icons_offset
    data[28..32].copy_from_slice(&map_start.to_le_bytes()); // map_offset

    // Map data at offset 32: single byte = 0 (tile index).
    // Tile pixel data starting at offset 33 (already zeroed).

    let td = tmp::TdTmpFile::parse(&data).unwrap();
    assert_eq!(td.tiles.len(), 1);
    assert_eq!(td.header.icon_width, 24);
}

/// Adversarial: every public parser on all-`0xFF` input must not panic.
///
/// Why (V38): each parser must handle worst-case inputs (maximised header
/// fields, offset overflow, huge declared sizes) without panic or OOM.
/// This is the integration-level counterpart of per-module adversarial
/// tests — it confirms the **entire** public API surface is safe in a
/// single test function.
#[test]
fn all_parsers_adversarial_all_ff() {
    let data = vec![0xFFu8; 512];
    // Binary format parsers
    let _ = MixArchive::parse(&data);
    let _ = ShpFile::parse(&data);
    let _ = Palette::parse(&data);
    let _ = AudFile::parse(&data);
    let _ = lcw::decompress(&data, 512);
    let _ = VqaFile::parse(&data);
    let _ = WsaFile::parse(&data);
    let _ = FntFile::parse(&data);
    let _ = tmp::TdTmpFile::parse(&data);
    let _ = tmp::RaTmpFile::parse(&data);
    // Text format parsers
    let _ = IniFile::parse(&data);
    #[cfg(feature = "miniyaml")]
    let _ = MiniYamlDoc::parse(&data);
    // Feature-gated music format parsers
    #[cfg(feature = "adl")]
    let _ = AdlFile::parse(&data);
    #[cfg(feature = "midi")]
    let _ = MidFile::parse(&data);
    #[cfg(feature = "xmi")]
    let _ = XmiFile::parse(&data);
}

/// Adversarial: every public parser on all-`0x00` input must not panic.
///
/// Why (V38): zero-filled input exercises zero-dimension paths, zero-count
/// loops, division-by-zero guards, and degenerate empty-payload handling.
/// This is the all-zero counterpart of `all_parsers_adversarial_all_ff`.
#[test]
fn all_parsers_adversarial_all_zero() {
    let data = vec![0x00u8; 512];
    // Binary format parsers
    let _ = MixArchive::parse(&data);
    let _ = ShpFile::parse(&data);
    let _ = Palette::parse(&data);
    let _ = AudFile::parse(&data);
    let _ = lcw::decompress(&data, 512);
    let _ = VqaFile::parse(&data);
    let _ = WsaFile::parse(&data);
    let _ = FntFile::parse(&data);
    let _ = tmp::TdTmpFile::parse(&data);
    let _ = tmp::RaTmpFile::parse(&data);
    // Text format parsers
    let _ = IniFile::parse(&data);
    #[cfg(feature = "miniyaml")]
    let _ = MiniYamlDoc::parse(&data);
    // Feature-gated music format parsers
    #[cfg(feature = "adl")]
    let _ = AdlFile::parse(&data);
    #[cfg(feature = "midi")]
    let _ = MidFile::parse(&data);
    #[cfg(feature = "xmi")]
    let _ = XmiFile::parse(&data);
}

// ── Text format integration tests ────────────────────────────────────────────

/// INI: parse a realistic C&C rules fragment with multiple sections.
///
/// Why: integration-level confidence that `IniFile::parse` works
/// end-to-end with a typical multi-section rules file.
#[test]
fn ini_parse_realistic_rules() {
    let input = b"\
[General]
Name=Red Alert
Version=3.03

[E1]
Name=Rifle Infantry
Strength=50
Speed=4
";
    let ini = IniFile::parse(input).unwrap();
    assert_eq!(ini.section_count(), 2);
    assert_eq!(ini.get("General", "Name"), Some("Red Alert"));
    assert_eq!(ini.get("E1", "Strength"), Some("50"));
}

/// INI: adversarial all-`0xFF` bytes (invalid UTF-8) must not panic.
#[test]
fn ini_adversarial_all_ff() {
    let data = vec![0xFFu8; 512];
    let _ = IniFile::parse(&data);
}

/// MiniYAML: parse a realistic OpenRA unit definition.
///
/// Why: integration-level confidence that `MiniYamlDoc::parse` works
/// end-to-end with a typical OpenRA mod file.
#[cfg(feature = "miniyaml")]
#[test]
fn miniyaml_parse_realistic_unit() {
    let input = b"\
Inherits: @infantry
Name: Rifle Infantry
Health:
\tHP: 100
Mobile:
\tSpeed: 56
";
    let doc = MiniYamlDoc::parse(input).unwrap();
    assert_eq!(doc.nodes().len(), 4);
    assert_eq!(doc.node("Inherits").unwrap().value(), Some("@infantry"));
    let health = doc.node("Health").unwrap();
    assert_eq!(health.child("HP").unwrap().value(), Some("100"));
}

/// MiniYAML: adversarial all-`0xFF` bytes (invalid UTF-8) must not panic.
#[cfg(feature = "miniyaml")]
#[test]
fn miniyaml_adversarial_all_ff() {
    let data = vec![0xFFu8; 512];
    let _ = MiniYamlDoc::parse(&data);
}
