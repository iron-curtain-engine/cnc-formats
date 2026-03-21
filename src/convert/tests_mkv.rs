// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::mkv::{encode_mkv, MkvAudio, MkvVideoCodec};

// ── Default codec (V_UNCOMPRESSED) ──────────────────────────────────────────

/// Verify the EBML header starts with the correct magic bytes and contains
/// the "matroska" DocType string.
#[test]
fn mkv_ebml_header_contains_matroska_doctype() {
    let frame = vec![0u8; 4 * 4 * 4]; // 4×4 RGBA
    let out = encode_mkv(&[&frame], 4, 4, 15, None, MkvVideoCodec::default()).unwrap();

    // EBML header element ID: 0x1A 0x45 0xDF 0xA3
    assert_eq!(out.get(0..4), Some([0x1A, 0x45, 0xDF, 0xA3].as_slice()));
    // The string "matroska" must appear in the header region.
    let header_region = out.get(..64).unwrap_or(&out);
    assert!(
        header_region.windows(8).any(|w| w == b"matroska"),
        "EBML header must contain 'matroska' DocType"
    );
}

/// Verify the output contains Segment, Info, and Tracks elements.
#[test]
fn mkv_contains_required_top_level_elements() {
    let frame = vec![0u8; 4 * 4 * 4]; // 4×4 RGBA
    let out = encode_mkv(&[&frame], 4, 4, 15, None, MkvVideoCodec::default()).unwrap();

    // Segment ID: 0x18 0x53 0x80 0x67
    assert!(
        out.windows(4).any(|w| w == [0x18, 0x53, 0x80, 0x67]),
        "output must contain Segment element"
    );
    // Info ID: 0x15 0x49 0xA9 0x66
    assert!(
        out.windows(4).any(|w| w == [0x15, 0x49, 0xA9, 0x66]),
        "output must contain Info element"
    );
    // Tracks ID: 0x16 0x54 0xAE 0x6B
    assert!(
        out.windows(4).any(|w| w == [0x16, 0x54, 0xAE, 0x6B]),
        "output must contain Tracks element"
    );
    // Cluster ID: 0x1F 0x43 0xB6 0x75
    assert!(
        out.windows(4).any(|w| w == [0x1F, 0x43, 0xB6, 0x75]),
        "output must contain at least one Cluster element"
    );
}

/// Verify that a video-only MKV does NOT contain the audio codec ID.
#[test]
fn mkv_video_only_omits_audio_track() {
    let frame = vec![0u8; 4 * 4 * 4]; // 4×4 RGBA
    let out = encode_mkv(&[&frame], 4, 4, 15, None, MkvVideoCodec::default()).unwrap();

    assert!(
        !out.windows(13).any(|w| w == b"A_PCM/INT/LIT"),
        "video-only MKV should not contain audio codec ID"
    );
}

/// Verify that multiple frames produce a reasonably sized output.
#[test]
fn mkv_multiple_frames_scales_output_size() {
    let frame = vec![0u8; 8 * 8 * 4]; // 8×8 RGBA
    let frames: Vec<&[u8]> = vec![frame.as_slice(); 10];
    let out = encode_mkv(&frames, 8, 8, 15, None, MkvVideoCodec::default()).unwrap();

    // Each frame is 8×8×3 = 192 bytes BGR24, plus SimpleBlock overhead.
    // 10 frames × ~200 bytes = ~2000 bytes minimum payload.
    assert!(
        out.len() > 2000,
        "10 frames of 8×8 should produce >2000 bytes, got {}",
        out.len()
    );
}

/// Verify validation rejects empty frame list.
#[test]
fn mkv_rejects_empty_frames() {
    let result = encode_mkv::<&[u8]>(&[], 4, 4, 15, None, MkvVideoCodec::default());
    assert!(result.is_err());
}

/// Verify validation rejects zero dimensions.
#[test]
fn mkv_rejects_zero_dimensions() {
    let frame = vec![0u8; 4];
    assert!(encode_mkv(&[&frame], 0, 4, 15, None, MkvVideoCodec::default()).is_err());
    assert!(encode_mkv(&[&frame], 4, 0, 15, None, MkvVideoCodec::default()).is_err());
}

/// Verify that the "cnc-formats" muxing/writing app string appears in output.
#[test]
fn mkv_contains_muxing_app_string() {
    let frame = vec![0u8; 4 * 4 * 4];
    let out = encode_mkv(&[&frame], 4, 4, 15, None, MkvVideoCodec::default()).unwrap();

    assert!(
        out.windows(11).any(|w| w == b"cnc-formats"),
        "output must contain 'cnc-formats' app string"
    );
}

// ── V_UNCOMPRESSED codec ────────────────────────────────────────────────────

/// Verify that `V_UNCOMPRESSED` mode embeds the correct codec ID and the
/// UncompressedFourCC element (BI_RGB = 4 zero bytes) for BGR24 top-down.
#[test]
fn mkv_uncompressed_codec_id_and_fourcc() {
    let frame = vec![0u8; 4 * 2 * 4]; // 4×2 RGBA
    let samples: Vec<i16> = (0..1470).map(|i| (i * 7) as i16).collect();
    let audio = MkvAudio {
        samples: &samples,
        sample_rate: 22050,
        channels: 1,
    };
    let out = encode_mkv(
        &[&frame],
        4,
        2,
        15,
        Some(&audio),
        MkvVideoCodec::Uncompressed,
    )
    .unwrap();

    assert!(
        out.windows(14).any(|w| w == b"V_UNCOMPRESSED"),
        "V_UNCOMPRESSED codec ID must be present"
    );
    assert!(
        out.windows(13).any(|w| w == b"A_PCM/INT/LIT"),
        "audio codec ID must be present"
    );
    // Must NOT contain the VFW codec string.
    assert!(
        !out.windows(15).any(|w| w == b"V_MS/VFW/FOURCC"),
        "V_MS/VFW/FOURCC must not appear in V_UNCOMPRESSED mode"
    );
}

/// Verify that `V_UNCOMPRESSED` video frames use top-down row order.
///
/// A 2×2 RGBA frame with distinct row colours:
///   row 0 = red  (255,0,0,255), row 1 = blue (0,0,255,255)
/// In BGR24 top-down, the first 3 output bytes should be BGR of red = [0,0,255].
#[test]
fn mkv_uncompressed_top_down_row_order() {
    // 2×2: row 0 = red, row 1 = blue
    let rgba: Vec<u8> = [
        255, 0, 0, 255, 255, 0, 0, 255, // row 0: 2 red pixels
        0, 0, 255, 255, 0, 0, 255, 255, // row 1: 2 blue pixels
    ]
    .to_vec();
    let out = encode_mkv(&[&rgba], 2, 2, 15, None, MkvVideoCodec::Uncompressed).unwrap();

    // BGR24 for red = [0, 0, 255].  Find this pattern in the SimpleBlock
    // payload.  Frame is 2×2×3 = 12 bytes.  In top-down order the first
    // 3 bytes of the frame payload should be BGR of red.
    let bgr_red = [0u8, 0, 255];
    let bgr_blue = [255u8, 0, 0];
    // Find the frame data by locating the 12-byte BGR payload.
    // Top-down: red row first, then blue row.
    let expected_frame = [
        bgr_red[0],
        bgr_red[1],
        bgr_red[2],
        bgr_red[0],
        bgr_red[1],
        bgr_red[2],
        bgr_blue[0],
        bgr_blue[1],
        bgr_blue[2],
        bgr_blue[0],
        bgr_blue[1],
        bgr_blue[2],
    ];
    assert!(
        out.windows(12).any(|w| w == expected_frame),
        "V_UNCOMPRESSED frame must be top-down (red row first, then blue)"
    );
}

// ── V_MS/VFW/FOURCC codec ───────────────────────────────────────────────────

/// Verify that `Vfw` mode embeds the correct codec ID and a 40-byte
/// BITMAPINFOHEADER as CodecPrivate.
#[test]
fn mkv_vfw_codec_id_and_bitmapinfoheader() {
    let frame = vec![0u8; 4 * 2 * 4]; // 4×2 RGBA
    let samples: Vec<i16> = (0..1470).map(|i| (i * 7) as i16).collect();
    let audio = MkvAudio {
        samples: &samples,
        sample_rate: 22050,
        channels: 1,
    };
    let out = encode_mkv(&[&frame], 4, 2, 15, Some(&audio), MkvVideoCodec::Vfw).unwrap();

    assert!(
        out.windows(15).any(|w| w == b"V_MS/VFW/FOURCC"),
        "V_MS/VFW/FOURCC codec ID must be present"
    );
    assert!(
        out.windows(13).any(|w| w == b"A_PCM/INT/LIT"),
        "audio codec ID must be present"
    );
    // Must NOT contain the V_UNCOMPRESSED codec string.
    assert!(
        !out.windows(14).any(|w| w == b"V_UNCOMPRESSED"),
        "V_UNCOMPRESSED must not appear in VFW mode"
    );

    // Verify BITMAPINFOHEADER is embedded: biSize=40 (LE) as first 4 bytes.
    let bi_size_le = 40u32.to_le_bytes();
    assert!(
        out.windows(4).any(|w| w == bi_size_le),
        "BITMAPINFOHEADER biSize (40 LE) must be present in output"
    );

    // Verify biWidth=4 and biHeight=2 appear at correct offsets within the BIH.
    // Find the BIH start by locating biSize=40.
    let bih_pos = out
        .windows(4)
        .position(|w| w == bi_size_le)
        .expect("biSize must be found");
    // biWidth at offset 4, biHeight at offset 8.
    assert_eq!(
        out.get(bih_pos + 4..bih_pos + 8),
        Some(4u32.to_le_bytes().as_slice()),
        "biWidth must be 4"
    );
    assert_eq!(
        out.get(bih_pos + 8..bih_pos + 12),
        Some(2u32.to_le_bytes().as_slice()),
        "biHeight must be 2 (positive = bottom-up)"
    );
    // biBitCount at offset 14.
    assert_eq!(
        out.get(bih_pos + 14..bih_pos + 16),
        Some(24u16.to_le_bytes().as_slice()),
        "biBitCount must be 24"
    );
}

/// Verify that `Vfw` video frames use bottom-up row order.
///
/// Same 2×2 frame as the top-down test: row 0 = red, row 1 = blue.
/// In BGR24 bottom-up, the first 3 output bytes should be BGR of blue
/// (the last row comes first in memory).
#[test]
fn mkv_vfw_bottom_up_row_order() {
    // 2×2: row 0 = red, row 1 = blue (top-down RGBA input)
    let rgba: Vec<u8> = [
        255, 0, 0, 255, 255, 0, 0, 255, // row 0: 2 red pixels
        0, 0, 255, 255, 0, 0, 255, 255, // row 1: 2 blue pixels
    ]
    .to_vec();
    let out = encode_mkv(&[&rgba], 2, 2, 15, None, MkvVideoCodec::Vfw).unwrap();

    // BGR24 bottom-up: blue row stored first in memory, then red row.
    let bgr_red = [0u8, 0, 255];
    let bgr_blue = [255u8, 0, 0];
    let expected_frame = [
        bgr_blue[0],
        bgr_blue[1],
        bgr_blue[2],
        bgr_blue[0],
        bgr_blue[1],
        bgr_blue[2],
        bgr_red[0],
        bgr_red[1],
        bgr_red[2],
        bgr_red[0],
        bgr_red[1],
        bgr_red[2],
    ];
    assert!(
        out.windows(12).any(|w| w == expected_frame),
        "V_MS/VFW/FOURCC frame must be bottom-up (blue row first, then red)"
    );
}
