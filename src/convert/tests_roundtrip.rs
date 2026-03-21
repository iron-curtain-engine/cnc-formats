// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;
use crate::aud::{self, AudFile, AUD_FLAG_16BIT, SCOMP_WESTWOOD};
use crate::pal::Palette;
use crate::shp::ShpFile;
use crate::tmp::TdTmpFile;
use crate::wsa::WsaFile;

fn test_palette() -> Palette {
    let mut pal_data = vec![0u8; 768];
    pal_data[3] = 63;
    pal_data[7] = 63;
    pal_data[11] = 63;
    Palette::parse(&pal_data).unwrap()
}

fn build_test_shp(value: u8) -> Vec<u8> {
    crate::shp::build_test_shp_helper(2, 2, value)
}

fn build_test_aud() -> Vec<u8> {
    let sample_rate: u16 = 22050;
    let chunk_size: u16 = 8;
    let uncompressed_size: u16 = 16;
    let out_size_field: u16 = uncompressed_size;

    let mut buf = Vec::new();
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    let compressed_body_size: u32 = 4 + chunk_size as u32;
    buf.extend_from_slice(&compressed_body_size.to_le_bytes());
    buf.extend_from_slice(&(uncompressed_size as u32).to_le_bytes());
    buf.push(AUD_FLAG_16BIT);
    buf.push(SCOMP_WESTWOOD);
    buf.extend_from_slice(&chunk_size.to_le_bytes());
    buf.extend_from_slice(&out_size_field.to_le_bytes());
    buf.extend_from_slice(&0i16.to_le_bytes());
    buf.push(0u8);
    buf.push(0u8);
    buf.extend_from_slice(&[0u8; 4]);
    buf
}

#[test]
fn lcw_round_trip() {
    let input = vec![1, 2, 3, 4, 5, 5, 5, 5, 5, 5, 5, 5, 1, 2, 3, 4];
    let compressed = crate::lcw::compress(&input);
    let decompressed = crate::lcw::decompress(&compressed, input.len()).unwrap();
    assert_eq!(decompressed, input);
}

#[test]
fn lcw_round_trip_fill() {
    let input = vec![0xAA; 200];
    let compressed = crate::lcw::compress(&input);
    assert!(compressed.len() < input.len());
    let decompressed = crate::lcw::decompress(&compressed, input.len()).unwrap();
    assert_eq!(decompressed, input);
}

#[test]
fn adpcm_round_trip_approximate() {
    let samples: Vec<i16> = (0..64).map(|i| (i * 100) as i16).collect();
    let encoded = aud::encode_adpcm(&samples, false);
    let decoded = aud::decode_adpcm(&encoded, false, samples.len());
    assert_eq!(decoded.len(), samples.len());
    for (i, (&orig, &dec)) in samples.iter().zip(decoded.iter()).enumerate() {
        let diff = (orig as i32 - dec as i32).unsigned_abs();
        assert!(
            diff < 500,
            "sample {i}: orig={orig}, decoded={dec}, diff={diff}"
        );
    }
}

#[test]
fn lossless_shp_png_round_trip() {
    let pal = test_palette();
    let shp_data = build_test_shp(0x01);
    let shp = ShpFile::parse(&shp_data).unwrap();
    let original_pixels = shp.decode_frames().unwrap();

    let pngs = shp_frames_to_png(&shp, &pal).unwrap();
    let reimported = png_to_shp(&[pngs[0].as_slice()], &pal).unwrap();
    let shp2 = ShpFile::parse(&reimported).unwrap();
    let reimported_pixels = shp2.decode_frames().unwrap();

    assert_eq!(original_pixels, reimported_pixels);
}

#[test]
fn lossless_shp_gif_round_trip() {
    let pal = test_palette();
    let shp_data = build_test_shp(0x01);
    let shp = ShpFile::parse(&shp_data).unwrap();
    let original_pixels = shp.decode_frames().unwrap();

    let gif_data = shp_frames_to_gif(&shp, &pal, 10).unwrap();
    let reimported = gif_to_shp(&gif_data, &pal).unwrap();
    let shp2 = ShpFile::parse(&reimported).unwrap();
    let reimported_pixels = shp2.decode_frames().unwrap();

    assert_eq!(original_pixels, reimported_pixels);
}

#[test]
fn lossless_pal_png_round_trip() {
    let pal = test_palette();
    let original_bytes = pal.encode();

    let png_data = pal_to_png(&pal).unwrap();
    let reimported_bytes = png_to_pal(&png_data).unwrap();

    assert_eq!(original_bytes, reimported_bytes);
}

#[test]
fn lossless_tmp_png_round_trip() {
    let pal = test_palette();
    let tmp_data = crate::tmp::build_td_test_tmp();
    let tmp = crate::tmp::TdTmpFile::parse(&tmp_data).unwrap();
    let original_tile_pixels = tmp.tiles[0].pixels.to_vec();

    let pngs = td_tmp_tiles_to_png(&tmp, &pal).unwrap();
    let reimported = png_to_td_tmp(&[pngs[0].as_slice()], &pal).unwrap();
    let tmp2 = crate::tmp::TdTmpFile::parse(&reimported).unwrap();

    assert_eq!(original_tile_pixels, tmp2.tiles[0].pixels);
}

#[test]
fn lossless_wsa_png_round_trip() {
    let pal = test_palette();
    let frame0 = vec![1u8; 4];
    let frame1 = vec![2u8; 4];
    let wsa_data =
        crate::wsa::encode_frames(&[frame0.as_slice(), frame1.as_slice()], 2, 2).unwrap();
    let wsa = WsaFile::parse(&wsa_data).unwrap();
    let original_frames = wsa.decode_frames().unwrap();

    let pngs = wsa_frames_to_png(&wsa, &pal).unwrap();
    let png_refs: Vec<&[u8]> = pngs.iter().map(|p| p.as_slice()).collect();
    let reimported = png_to_wsa(&png_refs, &pal).unwrap();
    let wsa2 = WsaFile::parse(&reimported).unwrap();
    let reimported_frames = wsa2.decode_frames().unwrap();

    assert_eq!(original_frames, reimported_frames);
}

#[test]
fn lossless_wsa_gif_round_trip() {
    let pal = test_palette();
    let frame0 = vec![1u8; 4];
    let frame1 = vec![2u8; 4];
    let wsa_data =
        crate::wsa::encode_frames(&[frame0.as_slice(), frame1.as_slice()], 2, 2).unwrap();
    let wsa = WsaFile::parse(&wsa_data).unwrap();
    let original_frames = wsa.decode_frames().unwrap();

    let gif_data = wsa_frames_to_gif(&wsa, &pal, 10).unwrap();
    let reimported = gif_to_wsa(&gif_data, &pal).unwrap();
    let wsa2 = WsaFile::parse(&reimported).unwrap();
    let reimported_frames = wsa2.decode_frames().unwrap();

    assert_eq!(original_frames, reimported_frames);
}

#[test]
fn lossless_avi_round_trip() {
    let width: u32 = 8;
    let height: u32 = 4;
    let px_count = (width * height) as usize;

    let frame1: Vec<u8> = [255, 0, 0, 255].repeat(px_count);
    let frame2: Vec<u8> = [0, 128, 255, 255].repeat(px_count);
    let audio: Vec<i16> = (0..200)
        .map(|i| ((i * 137) % 65536 - 32768) as i16)
        .collect();

    let avi_data = avi::encode_avi(
        &[frame1.clone(), frame2.clone()],
        width,
        height,
        15,
        Some(&audio),
        22050,
        1,
    )
    .unwrap();
    let decoded = avi::decode_avi(&avi_data).unwrap();

    assert_eq!(decoded.frames[0], frame1);
    assert_eq!(decoded.frames[1], frame2);
    assert_eq!(decoded.audio, audio);
}

#[test]
fn lossless_aud_wav_idempotent() {
    let aud_data = build_test_aud();
    let aud = AudFile::parse(&aud_data).unwrap();

    let wav1 = aud_to_wav(&aud).unwrap();
    let aud1 = wav_to_aud(&wav1).unwrap();
    let aud1_parsed = AudFile::parse(&aud1).unwrap();
    let stereo1 = aud1_parsed.header.is_stereo();
    let max1 = aud1_parsed.header.uncompressed_size as usize / 2;
    let samples1 = aud::decode_adpcm(aud1_parsed.compressed_data, stereo1, max1);

    let wav2 = aud_to_wav(&aud1_parsed).unwrap();
    let aud2 = wav_to_aud(&wav2).unwrap();
    let aud2_parsed = AudFile::parse(&aud2).unwrap();
    let stereo2 = aud2_parsed.header.is_stereo();
    let max2 = aud2_parsed.header.uncompressed_size as usize / 2;
    let samples2 = aud::decode_adpcm(aud2_parsed.compressed_data, stereo2, max2);

    assert_eq!(samples1, samples2);
}

// ── Reverse-direction roundtrips ─────────────────────────────────────────────
//
// The tests above start from C&C formats (SHP → PNG → SHP, etc.) and prove
// the import path is correct given correct export.  The tests below start
// from common formats and prove the export path is correct given correct
// import.  Together they validate both directions independently.

/// Builds a minimal WAV file from raw PCM samples using the hound crate.
fn build_wav(samples: &[i16], sample_rate: u32, channels: u16) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
        for &s in samples {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
    }
    cursor.into_inner()
}

/// Builds an RGBA PNG from pixel data using the png crate.
fn build_rgba_png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut buf, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(rgba).unwrap();
    }
    buf
}

/// A palette with well-separated colors for unambiguous nearest-color matching.
fn distinct_palette() -> Palette {
    let mut pal_data = [0u8; 768];
    // Index 0: black (0,0,0) — transparent in SHP/WSA
    // Index 1: bright red
    pal_data[3] = 63;
    // Index 2: bright green
    pal_data[7] = 63;
    // Index 3: bright blue
    pal_data[11] = 63;
    // Index 4: white
    pal_data[12] = 63;
    pal_data[13] = 63;
    pal_data[14] = 63;
    Palette::parse(&pal_data).unwrap()
}

/// WAV → AUD → WAV: validates that the import path handles arbitrary PCM
/// and that the ADPCM codec preserves the signal within expected tolerance.
///
/// ADPCM is inherently lossy and does not converge to a fixed point from
/// arbitrary PCM, so this test checks bounded error rather than idempotency.
#[test]
fn wav_to_aud_to_wav_bounded_error() {
    // Smooth ramp starting from 0 — the ADPCM predictor initializes at 0,
    // so starting near zero avoids cold-start error.  Small step size keeps
    // the codec step table from ramping up.
    let samples: Vec<i16> = (0..512).map(|i| i as i16 * 32).collect();
    let wav1 = build_wav(&samples, 22050, 1);

    // WAV → AUD → WAV round trip.
    let aud_data = wav_to_aud(&wav1).unwrap();
    let aud_parsed = AudFile::parse(&aud_data).unwrap();
    let wav2 = aud_to_wav(&aud_parsed).unwrap();

    // Decode both WAVs to compare samples.
    let decode_wav = |data: &[u8]| -> Vec<i16> {
        let reader = hound::WavReader::new(std::io::Cursor::new(data)).unwrap();
        reader.into_samples::<i16>().map(|s| s.unwrap()).collect()
    };
    let orig = decode_wav(&wav1);
    let decoded = decode_wav(&wav2);

    // Same number of samples.
    assert_eq!(orig.len(), decoded.len(), "sample count must be preserved");

    // AUD metadata must match.
    assert_eq!(aud_parsed.header.sample_rate, 22050);
    assert!(!aud_parsed.header.is_stereo());

    // Every sample must be within ADPCM tolerance.
    // IMA ADPCM with a smooth ramp keeps step sizes small; worst-case
    // per-sample error should be well under 200 for this signal.
    let mut max_diff = 0u32;
    for (i, (&o, &d)) in orig.iter().zip(decoded.iter()).enumerate() {
        let diff = (o as i32 - d as i32).unsigned_abs();
        max_diff = max_diff.max(diff);
        assert!(
            diff < 500,
            "sample {i}: orig={o}, decoded={d}, diff={diff} exceeds tolerance"
        );
    }
    assert!(
        max_diff > 0,
        "expected some quantization error from lossy ADPCM"
    );
}

/// PNG → SHP → PNG: pixels survive the round trip losslessly when they
/// match palette colors exactly.
#[test]
fn png_to_shp_to_png_lossless() {
    let pal = distinct_palette();
    let lut = pal.to_rgb8_array();
    let width: u32 = 4;
    let height: u32 = 2;

    // Build RGBA pixels using exact palette colors (indices 1..4).
    let indices = [1u8, 2, 3, 4, 4, 3, 2, 1];
    let mut rgba = Vec::with_capacity(indices.len() * 4);
    for &idx in &indices {
        let [r, g, b] = lut[idx as usize];
        rgba.extend_from_slice(&[r, g, b, 255]);
    }

    let png_data = build_rgba_png(&rgba, width, height);
    let shp_data = png_to_shp(&[png_data.as_slice()], &pal).unwrap();
    let shp = ShpFile::parse(&shp_data).unwrap();
    let pngs = shp_frames_to_png(&shp, &pal).unwrap();

    // Decode the re-exported PNG and compare RGBA pixels.
    let (reimported_rgba, w, h) = decode_png(&pngs[0]).unwrap();
    assert_eq!(w, width);
    assert_eq!(h, height);
    assert_eq!(reimported_rgba, rgba);
}

/// PNG → WSA → PNG: animation frames survive the round trip losslessly.
#[test]
fn png_to_wsa_to_png_lossless() {
    let pal = distinct_palette();
    let lut = pal.to_rgb8_array();
    let width: u32 = 2;
    let height: u32 = 2;

    // Frame 0: indices [1,2,3,4], Frame 1: indices [4,3,2,1].
    let build_frame = |indices: &[u8]| -> Vec<u8> {
        let mut rgba = Vec::with_capacity(indices.len() * 4);
        for &idx in indices {
            let [r, g, b] = lut[idx as usize];
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
        rgba
    };

    let frame0_rgba = build_frame(&[1, 2, 3, 4]);
    let frame1_rgba = build_frame(&[4, 3, 2, 1]);

    let png0 = build_rgba_png(&frame0_rgba, width, height);
    let png1 = build_rgba_png(&frame1_rgba, width, height);

    let wsa_data = png_to_wsa(&[png0.as_slice(), png1.as_slice()], &pal).unwrap();
    let wsa = WsaFile::parse(&wsa_data).unwrap();
    let pngs = wsa_frames_to_png(&wsa, &pal).unwrap();

    let (re0, w0, h0) = decode_png(&pngs[0]).unwrap();
    let (re1, w1, h1) = decode_png(&pngs[1]).unwrap();
    assert_eq!((w0, h0), (width, height));
    assert_eq!((w1, h1), (width, height));
    assert_eq!(re0, frame0_rgba);
    assert_eq!(re1, frame1_rgba);
}

/// PNG → TMP (TD) → PNG: tile pixels survive the round trip losslessly.
#[test]
fn png_to_td_tmp_to_png_lossless() {
    let pal = distinct_palette();
    let lut = pal.to_rgb8_array();
    let tile_w: u32 = 2;
    let tile_h: u32 = 2;

    // Build a single tile with indices [1,2,3,4] — all opaque, non-transparent.
    let indices = [1u8, 2, 3, 4];
    let mut rgba = Vec::with_capacity(indices.len() * 4);
    for &idx in &indices {
        let [r, g, b] = lut[idx as usize];
        // TMP tiles are fully opaque (no transparency), so alpha=255.
        rgba.extend_from_slice(&[r, g, b, 255]);
    }

    let png_data = build_rgba_png(&rgba, tile_w, tile_h);
    let tmp_data = png_to_td_tmp(&[png_data.as_slice()], &pal).unwrap();
    let tmp = TdTmpFile::parse(&tmp_data).unwrap();
    let pngs = td_tmp_tiles_to_png(&tmp, &pal).unwrap();

    let (reimported_rgba, w, h) = decode_png(&pngs[0]).unwrap();
    assert_eq!((w, h), (tile_w, tile_h));
    assert_eq!(reimported_rgba, rgba);
}

/// AVI → VQA → AVI: frame content survives the round trip.
///
/// The VQA encoder converts the 8-bit palette to 6-bit VGA (`>> 2`) and the
/// decoder scales it back (`(v << 2) | (v >> 4)`), so a small rounding
/// difference (e.g. 252→63→255) is expected.  This test verifies that every
/// pixel maps to the *same palette index* through the round trip, proving
/// the VQ quantization and palette lookup are correct.
#[test]
fn avi_to_vqa_to_avi_content_preserved() {
    let pal = distinct_palette();
    let lut = pal.to_rgb8_array();
    let width: u32 = 8;
    let height: u32 = 4;
    let px_count = (width * height) as usize;

    // Build a solid-red frame (palette index 1).
    let [r, g, b] = lut[1];
    let frame: Vec<u8> = [r, g, b, 255].repeat(px_count);

    let avi_data = avi::encode_avi(&[frame], width, height, 15, None, 0, 0).unwrap();
    let vqa_data = avi_to_vqa(&avi_data, &pal).unwrap();

    // Verify the VQA container is valid and has the right dimensions.
    let vqa = crate::vqa::VqaFile::parse(&vqa_data).unwrap();
    assert_eq!(vqa.header.width, 8);
    assert_eq!(vqa.header.height, 4);
    assert_eq!(vqa.header.num_frames, 1);

    // Decode VQA frames to get indexed pixels — every pixel should be index 1.
    let decoded_frames = vqa.decode_frames().unwrap();
    assert_eq!(decoded_frames.len(), 1);
    for (i, &idx) in decoded_frames[0].pixels.iter().enumerate() {
        assert_eq!(idx, 1, "pixel {i}: expected palette index 1, got {idx}");
    }

    // Also verify the full AVI round trip produces valid output.
    let avi_data2 = vqa_to_avi(&vqa).unwrap();
    let decoded = avi::decode_avi(&avi_data2).unwrap();
    assert_eq!(decoded.width, width);
    assert_eq!(decoded.height, height);
    assert_eq!(decoded.frames.len(), 1);

    // Pixels should be the same color (after 8→6→8 bit scaling).
    // The VQA decoder uses (v<<2)|(v>>4) which maps 63→255, not 252.
    // All pixels must be uniform (same color), proving the VQ path is correct.
    let f = &decoded.frames[0];
    let first_r = f[0];
    let first_g = f[1];
    let first_b = f[2];
    for pixel_idx in 1..px_count {
        let base = pixel_idx * 4;
        assert_eq!(
            (f[base], f[base + 1], f[base + 2]),
            (first_r, first_g, first_b),
            "pixel {pixel_idx} differs from pixel 0"
        );
    }
    // Red channel should be non-zero (proves palette mapping worked).
    assert!(first_r > 200, "red channel should be bright, got {first_r}");
    assert_eq!(first_g, 0);
    assert_eq!(first_b, 0);
}

/// GIF → SHP → GIF: animated GIF frames survive import and re-export.
///
/// Builds a GIF from known SHP data, then tests the reverse direction
/// (GIF → SHP → GIF) to prove both paths are consistent.
#[test]
fn gif_to_shp_to_gif_lossless() {
    let pal = distinct_palette();

    // Create SHP frames using palette indices, then export to GIF
    // to get a known-good GIF as our starting point.
    let frame0 = vec![1u8; 4]; // 2×2 solid red
    let frame1 = vec![2u8; 4]; // 2×2 solid green
    let shp_data = crate::shp::encode_frames(&[&frame0, &frame1], 2, 2).unwrap();
    let shp = ShpFile::parse(&shp_data).unwrap();
    let gif_data = shp_frames_to_gif(&shp, &pal, 10).unwrap();

    // Now test the reverse: GIF → SHP → GIF.
    let reimported_shp = gif_to_shp(&gif_data, &pal).unwrap();
    let shp2 = ShpFile::parse(&reimported_shp).unwrap();
    let gif_data2 = shp_frames_to_gif(&shp2, &pal, 10).unwrap();

    // One more round: GIF → SHP → GIF should be idempotent.
    let reimported_shp3 = gif_to_shp(&gif_data2, &pal).unwrap();
    let shp3 = ShpFile::parse(&reimported_shp3).unwrap();

    // Compare decoded pixel data (GIF bytes may differ due to encoding).
    let pixels2 = shp2.decode_frames().unwrap();
    let pixels3 = shp3.decode_frames().unwrap();
    assert_eq!(pixels2, pixels3, "GIF→SHP→GIF must be idempotent");
}

/// GIF → WSA → GIF: animated GIF frames survive import and re-export.
#[test]
fn gif_to_wsa_to_gif_lossless() {
    let pal = distinct_palette();

    // Create WSA frames, export to GIF as starting point.
    let frame0 = vec![1u8; 4];
    let frame1 = vec![2u8; 4];
    let wsa_data =
        crate::wsa::encode_frames(&[frame0.as_slice(), frame1.as_slice()], 2, 2).unwrap();
    let wsa = WsaFile::parse(&wsa_data).unwrap();
    let gif_data = wsa_frames_to_gif(&wsa, &pal, 10).unwrap();

    // Reverse: GIF → WSA → GIF.
    let reimported_wsa = gif_to_wsa(&gif_data, &pal).unwrap();
    let wsa2 = WsaFile::parse(&reimported_wsa).unwrap();
    let gif_data2 = wsa_frames_to_gif(&wsa2, &pal, 10).unwrap();

    // One more round for idempotency.
    let reimported_wsa3 = gif_to_wsa(&gif_data2, &pal).unwrap();
    let wsa3 = WsaFile::parse(&reimported_wsa3).unwrap();

    let frames2 = wsa2.decode_frames().unwrap();
    let frames3 = wsa3.decode_frames().unwrap();
    assert_eq!(frames2, frames3, "GIF→WSA→GIF must be idempotent");
}
