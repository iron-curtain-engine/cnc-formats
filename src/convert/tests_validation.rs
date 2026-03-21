// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! AVI/VQA conversion tests, codec round-trips, and lossless equality tests.

use super::*;
use crate::pal::Palette;

// ── Helpers (duplicated from tests.rs for module isolation) ──────────────────

/// Builds a minimal 256-color VGA palette for round-trip testing.
fn test_palette() -> Palette {
    let mut pal_data = vec![0u8; 768];
    pal_data[3] = 63; // Index 1: R=63
    pal_data[7] = 63; // Index 2: G=63
    pal_data[11] = 63; // Index 3: B=63
    Palette::parse(&pal_data).unwrap()
}

// ── AVI encode/decode round-trip ────────────────────────────────────────────

/// A minimal AVI with 2 frames round-trips through encode → decode.
///
/// Why: validates the RIFF/AVI container writer and reader end-to-end.
/// Checks that dimensions, frame count, pixel data, and audio survive
/// the round-trip.
#[test]
fn avi_round_trip_video_only() {
    let width: u32 = 4;
    let height: u32 = 2;
    let px_count = (width * height) as usize;

    // Frame 1: red pixels, Frame 2: blue pixels (RGBA).
    let frame1: Vec<u8> = [255, 0, 0, 255].repeat(px_count);
    let frame2: Vec<u8> = [0, 0, 255, 255].repeat(px_count);
    let frames = vec![frame1, frame2];

    let avi_data = avi::encode_avi(&frames, width, height, 15, None, 0, 0).unwrap();

    // Must start with RIFF...AVI header.
    assert_eq!(&avi_data[..4], b"RIFF");
    assert_eq!(&avi_data[8..12], b"AVI ");

    // Decode it back.
    let decoded = avi::decode_avi(&avi_data).unwrap();
    assert_eq!(decoded.width, width);
    assert_eq!(decoded.height, height);
    assert_eq!(decoded.frames.len(), 2);
    assert_eq!(decoded.fps, 15);

    // Check first frame: all red pixels.
    let f1 = &decoded.frames[0];
    assert_eq!(f1.len(), px_count * 4);
    assert_eq!(f1[0], 255); // R
    assert_eq!(f1[1], 0); // G
    assert_eq!(f1[2], 0); // B
    assert_eq!(f1[3], 255); // A
}

/// AVI with audio round-trips correctly.
///
/// Why: validates audio interleaving in the movi chunk.
#[test]
fn avi_round_trip_with_audio() {
    let width: u32 = 4;
    let height: u32 = 2;
    let px_count = (width * height) as usize;
    let frame: Vec<u8> = [128, 128, 128, 255].repeat(px_count);
    let frames = vec![frame];

    // Simple sine-ish audio: 100 samples.
    let audio: Vec<i16> = (0..100).map(|i| ((i * 327) % 32768) as i16).collect();

    let avi_data = avi::encode_avi(&frames, width, height, 15, Some(&audio), 22050, 1).unwrap();
    let decoded = avi::decode_avi(&avi_data).unwrap();

    assert_eq!(decoded.frames.len(), 1);
    assert_eq!(decoded.sample_rate, 22050);
    assert_eq!(decoded.channels, 1);
    // Audio samples should round-trip exactly (PCM, no lossy codec).
    assert_eq!(decoded.audio.len(), audio.len());
    assert_eq!(decoded.audio, audio);
}

/// AVI decode rejects non-RIFF data.
///
/// Why: ensures the magic check prevents garbled input from being misinterpreted.
#[test]
fn avi_decode_invalid_magic() {
    let data = b"NOT_A_RIFF_FILE_AT_ALL__";
    let result = avi::decode_avi(data);
    assert!(result.is_err());
}

/// AVI encode rejects zero-dimension frames.
///
/// Why: V38 — zero dimensions would cause division-by-zero in row stride calc.
#[test]
fn avi_encode_zero_dimensions() {
    let frame = vec![0u8; 4];
    assert!(avi::encode_avi(std::slice::from_ref(&frame), 0, 1, 15, None, 0, 0).is_err());
    assert!(avi::encode_avi(std::slice::from_ref(&frame), 1, 0, 15, None, 0, 0).is_err());
}

// ── VQA → AVI conversion ───────────────────────────────────────────────────

/// Builds a minimal valid VQA file with decodable frames for testing.
///
/// Creates a tiny 4×2 video with 1 frame containing a single solid-fill
/// block at palette index 1, complete with CPL + CBF + VPT sub-chunks.
fn build_decodable_vqa() -> Vec<u8> {
    // Build VQHD for a 4×2, 1-frame video.
    let mut hd = [0u8; 42];
    hd[0..2].copy_from_slice(&2u16.to_le_bytes()); // version = 2
    hd[2..4].copy_from_slice(&0u16.to_le_bytes()); // flags (no audio)
    hd[4..6].copy_from_slice(&1u16.to_le_bytes()); // num_frames = 1
    hd[6..8].copy_from_slice(&4u16.to_le_bytes()); // width = 4
    hd[8..10].copy_from_slice(&2u16.to_le_bytes()); // height = 2
    hd[10] = 4; // block_w
    hd[11] = 2; // block_h
    hd[12] = 15; // fps
    hd[13] = 8; // groupsize
    hd[14..16].copy_from_slice(&0u16.to_le_bytes()); // num_1_colors
    hd[16..18].copy_from_slice(&1u16.to_le_bytes()); // cb_entries = 1
    hd[18..20].copy_from_slice(&0u16.to_le_bytes()); // x_pos
    hd[20..22].copy_from_slice(&0u16.to_le_bytes()); // y_pos
    hd[22..24].copy_from_slice(&0u16.to_le_bytes()); // max_frame_size
                                                     // No audio (freq=0, channels=0, bits=0).

    // CPL0: 768 bytes of palette (6-bit VGA). Index 1 = red (63,0,0).
    let mut cpl = vec![0u8; 768];
    cpl[3] = 63; // Index 1: R=63

    // CBF0: one codebook entry of 4×2 = 8 bytes, all index 0.
    let cbf = vec![0u8; 8];

    // VPT0: for a 4×2 frame with 4×2 blocks, we have 1×1 = 1 block.
    // VPT = 2 bytes: lo = index 1 (palette color), hi = 0x0F (solid fill).
    let vpt = vec![1u8, 0x0F];

    // Build VQFR sub-chunks.
    let mut vqfr_payload = Vec::new();
    // CPL0
    vqfr_payload.extend_from_slice(b"CPL0");
    vqfr_payload.extend_from_slice(&(cpl.len() as u32).to_be_bytes());
    vqfr_payload.extend_from_slice(&cpl);
    // CBF0
    vqfr_payload.extend_from_slice(b"CBF0");
    vqfr_payload.extend_from_slice(&(cbf.len() as u32).to_be_bytes());
    vqfr_payload.extend_from_slice(&cbf);
    // VPT0
    vqfr_payload.extend_from_slice(b"VPT0");
    vqfr_payload.extend_from_slice(&(vpt.len() as u32).to_be_bytes());
    vqfr_payload.extend_from_slice(&vpt);

    // Assemble: FORM + WVQA + VQHD + VQFR.
    let form_payload_size = 4 + (8 + hd.len()) + (8 + vqfr_payload.len());
    let mut out = Vec::new();
    out.extend_from_slice(b"FORM");
    out.extend_from_slice(&(form_payload_size as u32).to_be_bytes());
    out.extend_from_slice(b"WVQA");

    // VQHD chunk
    out.extend_from_slice(b"VQHD");
    out.extend_from_slice(&(hd.len() as u32).to_be_bytes());
    out.extend_from_slice(&hd);

    // VQFR chunk
    out.extend_from_slice(b"VQFR");
    out.extend_from_slice(&(vqfr_payload.len() as u32).to_be_bytes());
    out.extend_from_slice(&vqfr_payload);

    out
}

/// VQA → AVI produces a valid AVI with decodable frame content.
///
/// Why: end-to-end validation of the entire VQA decode + AVI encode pipeline.
#[test]
fn vqa_to_avi_basic() {
    let vqa_data = build_decodable_vqa();
    let vqa = crate::vqa::VqaFile::parse(&vqa_data).unwrap();
    let avi_data = vqa_to_avi(&vqa).unwrap();

    // Verify AVI structure.
    assert_eq!(&avi_data[..4], b"RIFF");
    assert_eq!(&avi_data[8..12], b"AVI ");

    // Decode and check.
    let decoded = avi::decode_avi(&avi_data).unwrap();
    assert_eq!(decoded.width, 4);
    assert_eq!(decoded.height, 2);
    assert_eq!(decoded.frames.len(), 1);
    assert_eq!(decoded.fps, 15);
}

/// VQA → AVI → decode_avi produces frame with correct palette colors.
///
/// Why: validates palette-indexed → RGBA conversion in the export path.
/// The test VQA has all pixels at palette index 1 (red = 63,0,0 in 6-bit
/// VGA, scaled to 252,0,0 in 8-bit RGB).
#[test]
fn vqa_to_avi_palette_mapping() {
    let vqa_data = build_decodable_vqa();
    let vqa = crate::vqa::VqaFile::parse(&vqa_data).unwrap();
    let avi_data = vqa_to_avi(&vqa).unwrap();
    let decoded = avi::decode_avi(&avi_data).unwrap();

    // First pixel of first frame should be red (palette index 1 = 63→252).
    let f1 = &decoded.frames[0];
    let r = f1[0];
    let g = f1[1];
    let b = f1[2];
    // 6-bit 63 → 8-bit: (63 << 2) | (63 >> 4) = 252 | 3 = 255.
    assert_eq!(r, 255, "red channel should be 255");
    assert_eq!(g, 0, "green channel should be 0");
    assert_eq!(b, 0, "blue channel should be 0");
}

// ── AVI → VQA conversion ───────────────────────────────────────────────────

/// AVI → VQA round-trip produces a parseable VQA container.
///
/// Why: validates the VQA encoder + IFF container assembly.
#[test]
fn avi_to_vqa_round_trip() {
    let width: u32 = 8;
    let height: u32 = 4;
    let px_count = (width * height) as usize;
    // Simple frame: palette index 1 in RGBA (252, 0, 0, 255).
    let frame: Vec<u8> = [252, 0, 0, 255].repeat(px_count);
    let frames = vec![frame];

    let pal = test_palette();

    // Encode as AVI first.
    let avi_data = avi::encode_avi(&frames, width, height, 15, None, 0, 0).unwrap();

    // Convert AVI → VQA.
    let vqa_data = avi_to_vqa(&avi_data, &pal).unwrap();

    // The output must be a valid VQA container.
    let vqa = crate::vqa::VqaFile::parse(&vqa_data).unwrap();
    assert_eq!(vqa.header.width, 8);
    assert_eq!(vqa.header.height, 4);
    assert_eq!(vqa.header.num_frames, 1);
    assert_eq!(vqa.header.fps, 15);
    assert_eq!(vqa.header.version, 2);
}

// ── V38 safety cap tests ─────────────────────────────────────────────────────

/// PNG with dimensions exceeding MAX_IMAGE_DIMENSION (4096) is rejected.
///
/// Why: V38 — a crafted PNG with huge dimensions (e.g. 5000×1) could trigger
/// OOM via the RGBA pixel buffer allocation.  The `decode_png` helper must
/// reject such images before the allocation occurs.  Since `decode_png` is
/// private, we test through the public `png_to_shp` import path.
#[test]
fn png_oversized_dimensions_rejected() {
    let pal = test_palette();

    // Build a real PNG with width=5000, height=1 using the png crate encoder.
    // One row of 5000 RGBA pixels = 20000 bytes — small enough to construct
    // in-memory but exceeds the 4096-pixel dimension cap.
    let width: u32 = 5000;
    let height: u32 = 1;
    let row_data = vec![0u8; (width as usize) * 4]; // One row of RGBA pixels.

    let mut png_buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_buf, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("PNG header write failed");
        writer
            .write_image_data(&row_data)
            .expect("PNG image data write failed");
    }

    // Verify the PNG is structurally valid before testing the cap.
    assert_eq!(&png_buf[..8], b"\x89PNG\r\n\x1a\n");

    let result = png_to_shp(&[png_buf.as_slice()], &pal);
    assert!(result.is_err(), "oversized PNG should be rejected");

    let err = result.unwrap_err();
    assert!(
        matches!(
            err,
            crate::error::Error::InvalidSize {
                context: "PNG image dimension",
                ..
            }
        ),
        "expected InvalidSize for PNG dimension, got: {err:?}",
    );
}

/// AVI with video dimensions exceeding MAX_DIMENSION (4096) is rejected.
///
/// Why: V38 — a crafted AVI header claiming width=5000 could trigger OOM
/// when the decoder tries to allocate frame buffers.  `decode_avi` must
/// reject oversized dimensions after parsing the hdrl headers.
#[test]
fn avi_oversized_dimensions_rejected() {
    // Build a minimal RIFF AVI by hand with width=5000 in the BITMAPINFOHEADER.
    // Structure:
    //   RIFF <size> 'AVI '
    //     LIST <size> 'hdrl'
    //       'avih' <56 bytes>   — main AVI header
    //       LIST <size> 'strl'
    //         'strh' <56 bytes> — video stream header
    //         'strf' <40 bytes> — BITMAPINFOHEADER with oversized width
    //     LIST <size> 'movi'    — empty (no frames)

    let oversized_width: u32 = 5000;
    let normal_height: u32 = 1;

    // ── avih chunk (56 bytes of payload) ──
    let mut avih = vec![0u8; 56];
    // dwMicroSecPerFrame = 66667 (~15 fps).
    avih[0..4].copy_from_slice(&66667u32.to_le_bytes());
    // dwTotalFrames at offset 16 = 0 (no frames).

    // ── strh chunk (56 bytes of payload) ──
    let mut strh = vec![0u8; 56];
    // fccType = "vids" at offset 0.
    strh[0..4].copy_from_slice(b"vids");
    // dwScale at offset 20 = 1.
    strh[20..24].copy_from_slice(&1u32.to_le_bytes());
    // dwRate at offset 24 = 15.
    strh[24..28].copy_from_slice(&15u32.to_le_bytes());

    // ── strf chunk (40-byte BITMAPINFOHEADER) ──
    let mut strf = vec![0u8; 40];
    // biSize = 40.
    strf[0..4].copy_from_slice(&40u32.to_le_bytes());
    // biWidth at offset 4.
    strf[4..8].copy_from_slice(&(oversized_width as i32).to_le_bytes());
    // biHeight at offset 8.
    strf[8..12].copy_from_slice(&(normal_height as i32).to_le_bytes());
    // biPlanes at offset 12 = 1.
    strf[12..14].copy_from_slice(&1u16.to_le_bytes());
    // biBitCount at offset 14 = 24.
    strf[14..16].copy_from_slice(&24u16.to_le_bytes());
    // biCompression at offset 16 = 0 (BI_RGB).

    // ── Assemble strl LIST ──
    // strl payload = strh chunk + strf chunk.
    let strl_payload_size: u32 = 4 + (8 + strh.len() as u32) + (8 + strf.len() as u32);
    let mut strl = Vec::new();
    strl.extend_from_slice(b"LIST");
    strl.extend_from_slice(&strl_payload_size.to_le_bytes());
    strl.extend_from_slice(b"strl");
    strl.extend_from_slice(b"strh");
    strl.extend_from_slice(&(strh.len() as u32).to_le_bytes());
    strl.extend_from_slice(&strh);
    strl.extend_from_slice(b"strf");
    strl.extend_from_slice(&(strf.len() as u32).to_le_bytes());
    strl.extend_from_slice(&strf);

    // ── Assemble hdrl LIST ──
    let hdrl_payload_size: u32 = 4 + (8 + avih.len() as u32) + (8 + strl_payload_size);
    let mut hdrl = Vec::new();
    hdrl.extend_from_slice(b"LIST");
    hdrl.extend_from_slice(&hdrl_payload_size.to_le_bytes());
    hdrl.extend_from_slice(b"hdrl");
    hdrl.extend_from_slice(b"avih");
    hdrl.extend_from_slice(&(avih.len() as u32).to_le_bytes());
    hdrl.extend_from_slice(&avih);
    hdrl.extend_from_slice(&strl);

    // ── Assemble empty movi LIST ──
    let movi_payload_size: u32 = 4; // Just "movi" type tag, no chunks.
    let mut movi = Vec::new();
    movi.extend_from_slice(b"LIST");
    movi.extend_from_slice(&movi_payload_size.to_le_bytes());
    movi.extend_from_slice(b"movi");

    // ── Assemble RIFF AVI ──
    let riff_payload_size: u32 = 4 + (8 + hdrl_payload_size) + (8 + movi_payload_size);
    let mut avi_data = Vec::new();
    avi_data.extend_from_slice(b"RIFF");
    avi_data.extend_from_slice(&riff_payload_size.to_le_bytes());
    avi_data.extend_from_slice(b"AVI ");
    avi_data.extend_from_slice(&hdrl);
    avi_data.extend_from_slice(&movi);

    let result = avi::decode_avi(&avi_data);
    assert!(result.is_err(), "oversized AVI should be rejected");

    let err = result.unwrap_err();
    assert!(
        matches!(
            err,
            crate::error::Error::InvalidSize {
                context: "AVI video dimension",
                ..
            }
        ),
        "expected InvalidSize for AVI dimension, got: {err:?}",
    );
}
