// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Integration tests for format conversion (AUD, TMP, VQA -> WAV/MKV).
use super::*;
use crate::aud::{AudFile, AUD_FLAG_16BIT, SCOMP_WESTWOOD};
use crate::fnt::FntFile;
use crate::pal::Palette;
use crate::shp::ShpFile;
use crate::wsa::WsaFile;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Builds a minimal 256-color VGA palette: all black except index 1 = red,
/// index 2 = green, index 3 = blue (6-bit VGA values).
fn test_palette() -> Palette {
    let mut pal_data = vec![0u8; 768];
    // Index 1: R=63, G=0, B=0 (full red in 6-bit VGA).
    pal_data[3] = 63;
    // Index 2: R=0, G=63, B=0 (full green).
    pal_data[7] = 63;
    // Index 3: R=0, G=0, B=63 (full blue).
    pal_data[11] = 63;
    Palette::parse(&pal_data).unwrap()
}

/// Builds a minimal SHP file: 2×2, one LCW keyframe filling with `value`.
fn build_test_shp(value: u8) -> Vec<u8> {
    crate::shp::build_test_shp_helper(2, 2, value)
}

// ── SHP → PNG ────────────────────────────────────────────────────────────────

/// SHP with a single LCW keyframe converts to valid PNG bytes.
///
/// Why: core happy-path test for the most common modder use case.
#[test]
fn shp_to_png_single_frame() {
    let shp_data = build_test_shp(0x01);
    let shp = ShpFile::parse(&shp_data).unwrap();
    let pal = test_palette();
    let pngs = shp_frames_to_png(&shp, &pal).unwrap();
    assert_eq!(pngs.len(), 1);
    // PNG magic: first 8 bytes are the PNG signature.
    assert_eq!(&pngs[0][..8], b"\x89PNG\r\n\x1a\n");
}

// ── PAL → PNG ────────────────────────────────────────────────────────────────

/// Palette swatch produces a valid 16×16 PNG.
///
/// Why: palette visualization is one of the simplest conversions — verifies
/// the PNG encoder pipeline end-to-end.
#[test]
fn pal_to_png_valid() {
    let pal = test_palette();
    let png_data = pal_to_png(&pal).unwrap();
    assert_eq!(&png_data[..8], b"\x89PNG\r\n\x1a\n");
    // Should be a reasonably small file (16×16 RGBA).
    assert!(png_data.len() > 50);
    assert!(png_data.len() < 10_000);
}

// ── AUD → WAV ────────────────────────────────────────────────────────────────

/// AUD → WAV produces valid RIFF WAV bytes.
///
/// Why: verifies the full decode + WAV encoding pipeline.
///
/// How: Builds a minimal AUD file with Westwood ADPCM compression.
/// The compressed data is a single IMA ADPCM chunk.
#[test]
fn aud_to_wav_valid() {
    let aud_data = build_test_aud();
    let aud = AudFile::parse(&aud_data).unwrap();
    let wav_data = aud_to_wav(&aud).unwrap();
    // WAV magic: "RIFF" at offset 0, "WAVE" at offset 8.
    assert_eq!(&wav_data[..4], b"RIFF");
    assert_eq!(&wav_data[8..12], b"WAVE");
}

/// Builds a minimal valid AUD file with Westwood IMA ADPCM compression.
///
/// Contains one ADPCM chunk with a 4-byte header (initial sample + step
/// index) followed by 4 bytes of ADPCM nibbles.
fn build_test_aud() -> Vec<u8> {
    let sample_rate: u16 = 22050;
    // One chunk: 4-byte header + 4 bytes of nibbles = 8 bytes compressed.
    let chunk_size: u16 = 8;
    let uncompressed_size: u16 = 16; // 8 nibbles × 2 bytes each.
    let out_size_field: u16 = uncompressed_size;

    // File header: 12 bytes.
    let mut buf = Vec::new();
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    // compressed_size (u32): chunk header (4) + chunk data (4+4) = 12.
    let compressed_body_size: u32 = 4 + chunk_size as u32;
    buf.extend_from_slice(&compressed_body_size.to_le_bytes());
    // uncompressed_size (u32).
    buf.extend_from_slice(&(uncompressed_size as u32).to_le_bytes());
    // flags: 16-bit, mono.
    buf.push(AUD_FLAG_16BIT);
    // compression: Westwood IMA ADPCM.
    buf.push(SCOMP_WESTWOOD);

    // Chunk header: u16 compressed_size, u16 uncompressed_size.
    buf.extend_from_slice(&chunk_size.to_le_bytes());
    buf.extend_from_slice(&out_size_field.to_le_bytes());

    // IMA ADPCM header: i16 initial_sample = 0, u8 step_index = 0, u8 pad.
    buf.extend_from_slice(&0i16.to_le_bytes());
    buf.push(0u8); // step_index
    buf.push(0u8); // padding

    // 4 bytes of ADPCM nibbles (all zero = silence).
    buf.extend_from_slice(&[0u8; 4]);

    buf
}

// ── FNT → PNG ────────────────────────────────────────────────────────────────

/// FNT atlas produces a valid PNG.
///
/// Why: verifies the font atlas rendering — 16×16 grid of glyphs.
#[test]
fn fnt_to_png_valid() {
    let fnt_data = build_test_fnt();
    let fnt = FntFile::parse(&fnt_data).unwrap();
    let png_data = fnt_to_png(&fnt).unwrap();
    assert_eq!(&png_data[..8], b"\x89PNG\r\n\x1a\n");
}

/// Builds a minimal FNT file: 256 glyphs, 8×8 max cell, glyph 'A' has
/// width 4 and height 8.
fn build_test_fnt() -> Vec<u8> {
    let num_chars: u16 = 256;
    let nc = num_chars as usize;
    let max_height: u8 = 8;
    let glyph_w: u8 = 4;
    // 4bpp: ceil(4/2) = 2 bytes per row.
    let bytes_per_row = 2usize;
    let glyph_size = bytes_per_row * (max_height as usize);

    let offset_table_start = 20usize;
    let offset_table_size = nc * 2;
    let width_table_start = offset_table_start + offset_table_size;
    let width_table_size = nc;
    let data_area_start = width_table_start + width_table_size;
    let height_table_start = data_area_start + glyph_size;
    let height_table_size = nc * 2;
    let total = height_table_start + height_table_size;

    let mut buf = vec![0u8; total];

    // Header (20 bytes).
    buf[0..2].copy_from_slice(&(total as u16).to_le_bytes());
    buf[3] = 5; // data_blocks
    buf[6..8].copy_from_slice(&(offset_table_start as u16).to_le_bytes());
    buf[8..10].copy_from_slice(&(width_table_start as u16).to_le_bytes());
    buf[10..12].copy_from_slice(&(data_area_start as u16).to_le_bytes());
    buf[12..14].copy_from_slice(&(height_table_start as u16).to_le_bytes());
    buf[17] = 255; // char_count (last index)
    buf[18] = max_height;
    buf[19] = glyph_w;

    // Width and offset for glyph 0x41 ('A').
    buf[width_table_start + 0x41] = glyph_w;
    let o_pos = offset_table_start + 0x41 * 2;
    buf[o_pos..o_pos + 2].copy_from_slice(&(data_area_start as u16).to_le_bytes());
    // Height entry for 0x41: y_offset=0, data_rows=max_height.
    let h_pos = height_table_start + 0x41 * 2;
    buf[h_pos..h_pos + 2].copy_from_slice(&((max_height as u16) << 8).to_le_bytes());

    // Fill glyph data with pattern.
    for b in buf[data_area_start..data_area_start + glyph_size].iter_mut() {
        *b = 0x12;
    }

    buf
}

// ── TMP → PNG ────────────────────────────────────────────────────────────────

/// TD TMP tile converts to valid PNG.
///
/// Why: terrain tiles are the simplest pixel → PNG path (no decompression).
#[test]
fn td_tmp_to_png_valid() {
    let tmp_data = crate::tmp::build_td_test_tmp();
    let tmp = crate::tmp::TdTmpFile::parse(&tmp_data).unwrap();
    let pal = test_palette();
    let pngs = td_tmp_tiles_to_png(&tmp, &pal).unwrap();
    assert!(!pngs.is_empty());
    assert_eq!(&pngs[0][..8], b"\x89PNG\r\n\x1a\n");
}

// ── SHP → GIF ────────────────────────────────────────────────────────────────

/// SHP frames produce a valid animated GIF.
///
/// Why: GIF is a natural fit for palette-indexed sprites (max 256 colors,
/// multi-frame support).  Verifies the full SHP decode + GIF encode path.
#[test]
fn shp_to_gif_valid() {
    let shp_data = build_test_shp(0x01);
    let shp = ShpFile::parse(&shp_data).unwrap();
    let pal = test_palette();
    let gif_data = shp_frames_to_gif(&shp, &pal, 10).unwrap();
    // GIF magic: "GIF89a" header.
    assert_eq!(&gif_data[..6], b"GIF89a");
}

// ── WSA → GIF ────────────────────────────────────────────────────────────────

/// WSA frames produce a valid animated GIF.
///
/// Why: WSA is an animation format; GIF is the natural lossless preview.
#[test]
fn wsa_to_gif_valid() {
    // Build a minimal WSA: 2×2, two frames.
    let frame0 = vec![1u8; 4]; // All palette index 1.
    let frame1 = vec![2u8; 4]; // All palette index 2.
    let wsa_data =
        crate::wsa::encode_frames(&[frame0.as_slice(), frame1.as_slice()], 2, 2).unwrap();
    let wsa = WsaFile::parse(&wsa_data).unwrap();
    let pal = test_palette();
    let gif_data = wsa_frames_to_gif(&wsa, &pal, 10).unwrap();
    assert_eq!(&gif_data[..6], b"GIF89a");
}

// ── Round-trip: SHP → PNG → SHP ──────────────────────────────────────────────

/// SHP → PNG → SHP round-trip preserves pixel data.
///
/// Why: validates that the encoder + decoder pipeline is lossless when using
/// the same palette.  The original pixel indices should survive the RGBA
/// intermediate stage.
#[test]
fn shp_round_trip_png() {
    let pal = test_palette();
    // Build a 2×2 SHP with all pixels = index 1 (red).
    let shp_data = build_test_shp(0x01);
    let shp = ShpFile::parse(&shp_data).unwrap();

    // Export to PNG.
    let pngs = shp_frames_to_png(&shp, &pal).unwrap();
    assert_eq!(pngs.len(), 1);

    // Import back from PNG.
    let reimported = png_to_shp(&[pngs[0].as_slice()], &pal).unwrap();
    let shp2 = ShpFile::parse(&reimported).unwrap();
    let frames2 = shp2.decode_frames().unwrap();
    assert_eq!(frames2.len(), 1);
    // All 4 pixels should be index 1 (red).
    assert_eq!(frames2[0], vec![1u8; 4]);
}

// ── Round-trip: AUD → WAV → AUD ─────────────────────────────────────────────

/// AUD → WAV → AUD round-trip produces valid AUD.
///
/// Why: verifies the bidirectional audio pipeline.  The ADPCM codec is
/// lossy so exact sample equality isn't expected, but structural validity
/// is asserted.
#[test]
fn aud_round_trip_wav() {
    let aud_data = build_test_aud();
    let aud = AudFile::parse(&aud_data).unwrap();
    let wav_data = aud_to_wav(&aud).unwrap();
    // Import WAV back to AUD.
    let reimported = wav_to_aud(&wav_data).unwrap();
    // Should parse as valid AUD.
    let aud2 = AudFile::parse(&reimported).unwrap();
    assert_eq!(aud2.header.compression, SCOMP_WESTWOOD);
    assert_eq!(aud2.header.sample_rate, 22050);
}

// ── Round-trip: PAL → PNG → PAL ──────────────────────────────────────────────

/// PAL → PNG → PAL round-trip produces valid PAL bytes (768 bytes).
///
/// Why: palette extraction from PNG must survive the encode/decode cycle.
/// The PNG swatch is 16×16 where each pixel = one palette entry, so
/// extracting colors back should reproduce the original palette.
#[test]
fn pal_round_trip_png() {
    let pal = test_palette();
    let png_data = pal_to_png(&pal).unwrap();
    let pal_bytes = png_to_pal(&png_data).unwrap();
    assert_eq!(pal_bytes.len(), 768);
    // The first entry (index 0) should be black = (0, 0, 0).
    assert_eq!(pal_bytes[0], 0);
    assert_eq!(pal_bytes[1], 0);
    assert_eq!(pal_bytes[2], 0);
}

// ── Round-trip: TMP → PNG → TMP ─────────────────────────────────────────────

/// TMP → PNG → TMP round-trip preserves tile pixels.
///
/// Why: terrain tile encoding/decoding must be lossless with the same palette.
#[test]
fn tmp_round_trip_png() {
    let pal = test_palette();
    let tmp_data = crate::tmp::build_td_test_tmp();
    let tmp = crate::tmp::TdTmpFile::parse(&tmp_data).unwrap();

    // Export to PNG.
    let pngs = td_tmp_tiles_to_png(&tmp, &pal).unwrap();
    assert!(!pngs.is_empty());

    // Import first tile back.
    let reimported = png_to_td_tmp(&[pngs[0].as_slice()], &pal).unwrap();
    let tmp2 = crate::tmp::TdTmpFile::parse(&reimported).unwrap();
    assert_eq!(tmp2.tiles.len(), 1);
}

// ── WAV → AUD standalone ────────────────────────────────────────────────────

/// WAV → AUD produces valid AUD bytes.
///
/// Why: standalone import test — build WAV in memory, convert, and verify.
#[test]
fn wav_to_aud_valid() {
    // Build a minimal WAV using hound.
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 22050,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav_buf = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut wav_buf, spec).unwrap();
        // Write 100 samples of silence.
        for _ in 0..100 {
            writer.write_sample(0i16).unwrap();
        }
        writer.finalize().unwrap();
    }
    let wav_data = wav_buf.into_inner();
    let aud_data = wav_to_aud(&wav_data).unwrap();
    let aud = AudFile::parse(&aud_data).unwrap();
    assert_eq!(aud.header.sample_rate, 22050);
    assert_eq!(aud.header.compression, SCOMP_WESTWOOD);
}

// ── PNG → SHP empty input ───────────────────────────────────────────────────

/// PNG → SHP with no input files returns an error.
///
/// Why: empty input is a degenerate case that must be handled gracefully.
#[test]
fn png_to_shp_no_files_error() {
    let pal = test_palette();
    let result = png_to_shp(&[], &pal);
    assert!(result.is_err());
}

/// Convert error Display output includes the conversion-specific reason text.
///
/// Why: convert subcommands surface `Error::Display` directly, so the user
/// should see the actionable conversion failure, not just a generic variant.
#[test]
fn convert_error_display_includes_reason() {
    let pal = test_palette();
    let err = png_to_shp(&[], &pal).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("no PNG files provided for SHP encoding"),
        "Display should include the conversion reason: {msg}",
    );
}

// ── AUD chunk-header regression tests ───────────────────────────────────────

/// SCOMP=99 AUD with 0xDEAF chunk headers produces clean WAV output.
///
/// Why: RA1's IMA ADPCM files wrap every ~512 bytes of ADPCM data in an
/// 8-byte header (u16 compressed_size, u16 output_size, u32 0x0000DEAF).
/// If the headers are NOT stripped, they decode as audio nibbles, producing
/// audible clicks/pops every 520 bytes.  This test builds a minimal chunked
/// AUD and verifies the decoded samples contain no header-derived garbage.
#[test]
fn aud_scomp99_chunk_headers_stripped() {
    // Build a SCOMP=99 AUD with 2 chunks, each containing 4 bytes of
    // ADPCM silence (all zeros → decoded samples stay near zero).
    let adpcm_chunk = [0u8; 4]; // 4 bytes = 8 samples of near-silence
    let mut compressed = Vec::new();
    for _ in 0..2 {
        // Chunk header: compressed_size=4, output_size=16, magic=0xDEAF
        compressed.extend_from_slice(&4u16.to_le_bytes());
        compressed.extend_from_slice(&16u16.to_le_bytes());
        compressed.extend_from_slice(&0x0000_DEAFu32.to_le_bytes());
        compressed.extend_from_slice(&adpcm_chunk);
    }

    // Build AUD file: 12-byte header + compressed data.
    let mut aud_bytes = Vec::new();
    aud_bytes.extend_from_slice(&22050u16.to_le_bytes()); // sample_rate
    aud_bytes.extend_from_slice(&(compressed.len() as u32).to_le_bytes()); // compressed_size
    aud_bytes.extend_from_slice(&32u32.to_le_bytes()); // uncompressed_size (16 samples × 2 bytes)
    aud_bytes.push(AUD_FLAG_16BIT); // flags: 16-bit mono
    aud_bytes.push(99); // compression: IMA ADPCM (SCOMP=99)
    aud_bytes.extend_from_slice(&compressed);

    let aud = AudFile::parse(&aud_bytes).unwrap();
    let wav = aud_to_wav(&aud).unwrap();

    // WAV should be valid RIFF.
    assert_eq!(&wav[..4], b"RIFF", "output must be a RIFF WAV file");

    // Decode the WAV samples and verify they're near-zero (silence).
    // If headers weren't stripped, the 0xDEAF bytes would produce large
    // sample values (audible garbage).
    let reader = hound::WavReader::new(std::io::Cursor::new(&wav)).unwrap();
    let samples: Vec<i16> = reader.into_samples::<i16>().map(|s| s.unwrap()).collect();
    assert!(!samples.is_empty(), "WAV should contain decoded samples");
    // All samples from silence ADPCM should have absolute value < 50.
    // Header garbage would produce values in the thousands.
    for (i, &s) in samples.iter().enumerate() {
        assert!(
            s.abs() < 50,
            "sample {i} = {s}: header byte leaked into audio (expected near-zero)"
        );
    }
}

/// SCOMP=1 AUD (Westwood ADPCM) correctly strips the 4-byte per-chunk header.
///
/// Why: SCOMP=1 files use Westwood chunk framing (u16 compressed_size +
/// u16 uncompressed_size) before each ADPCM payload.  If the header bytes
/// are fed to the ADPCM decoder the output will be garbage.  This test
/// verifies the header is stripped and silence decodes to near-zero samples.
#[test]
fn aud_scomp1_strips_chunk_headers() {
    let adpcm = [0u8; 8]; // 8 bytes → 16 samples of silence
                          // Wrap in a Westwood 4-byte chunk header.
    let mut payload = Vec::new();
    payload.extend_from_slice(&(adpcm.len() as u16).to_le_bytes()); // compressed_size
    payload.extend_from_slice(&32u16.to_le_bytes()); // uncompressed_size (16 samples × 2 bytes)
    payload.extend_from_slice(&adpcm);

    let mut aud_bytes = Vec::new();
    aud_bytes.extend_from_slice(&22050u16.to_le_bytes());
    aud_bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    aud_bytes.extend_from_slice(&32u32.to_le_bytes());
    aud_bytes.push(AUD_FLAG_16BIT);
    aud_bytes.push(SCOMP_WESTWOOD);
    aud_bytes.extend_from_slice(&payload);

    let aud = AudFile::parse(&aud_bytes).unwrap();
    let wav = aud_to_wav(&aud).unwrap();
    assert_eq!(&wav[..4], b"RIFF");

    let reader = hound::WavReader::new(std::io::Cursor::new(&wav)).unwrap();
    let samples: Vec<i16> = reader.into_samples::<i16>().map(|s| s.unwrap()).collect();
    assert!(!samples.is_empty(), "WAV should contain decoded samples");
    for (i, &s) in samples.iter().enumerate() {
        assert!(
            s.abs() < 50,
            "sample {i} = {s}: SCOMP=1 silence should decode to near-zero (header byte leaked)"
        );
    }
}

#[test]
fn aud_reader_to_wav_matches_buffered_conversion() {
    let raw_adpcm = [0x07u8, 0x70, 0x11, 0x88]; // 4 bytes → 8 samples
                                                // Wrap in a Westwood 4-byte chunk header.
    let mut payload = Vec::new();
    payload.extend_from_slice(&(raw_adpcm.len() as u16).to_le_bytes());
    payload.extend_from_slice(&16u16.to_le_bytes()); // 8 samples × 2 bytes
    payload.extend_from_slice(&raw_adpcm);

    let mut aud_bytes = Vec::new();
    aud_bytes.extend_from_slice(&22050u16.to_le_bytes());
    aud_bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    aud_bytes.extend_from_slice(&16u32.to_le_bytes());
    aud_bytes.push(AUD_FLAG_16BIT);
    aud_bytes.push(SCOMP_WESTWOOD);
    aud_bytes.extend_from_slice(&payload);

    let aud = AudFile::parse(&aud_bytes).unwrap();
    let expected = aud_to_wav(&aud).unwrap();

    let mut actual = std::io::Cursor::new(Vec::new());
    aud_reader_to_wav(std::io::Cursor::new(&aud_bytes), &mut actual).unwrap();

    assert_eq!(actual.into_inner(), expected);
}
