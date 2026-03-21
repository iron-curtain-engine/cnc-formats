// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::mkv::encode_mkv;

/// Verify the EBML header starts with the correct magic bytes and contains
/// the "matroska" DocType string.
#[test]
fn mkv_ebml_header_contains_matroska_doctype() {
    let frame = vec![0u8; 4 * 4 * 4]; // 4×4 RGBA
    let out = encode_mkv(&[&frame], 4, 4, 15, None, 0, 0).unwrap();

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
    let out = encode_mkv(&[&frame], 4, 4, 15, None, 0, 0).unwrap();

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

/// Verify the codec ID strings are embedded in the output.
#[test]
fn mkv_contains_codec_ids() {
    let frame = vec![0u8; 4 * 2 * 4]; // 2×2 RGBA (doubled for width=4,height=2)
    let audio: Vec<i16> = (0..1470).map(|i| (i * 7) as i16).collect();
    let out = encode_mkv(&[&frame], 4, 2, 15, Some(&audio), 22050, 1).unwrap();

    assert!(
        out.windows(14).any(|w| w == b"V_UNCOMPRESSED"),
        "output must contain V_UNCOMPRESSED codec ID"
    );
    assert!(
        out.windows(13).any(|w| w == b"A_PCM/INT/LIT"),
        "output must contain A_PCM/INT/LIT codec ID"
    );
}

/// Verify that a video-only MKV does NOT contain the audio codec ID.
#[test]
fn mkv_video_only_omits_audio_track() {
    let frame = vec![0u8; 4 * 4 * 4]; // 4×4 RGBA
    let out = encode_mkv(&[&frame], 4, 4, 15, None, 0, 0).unwrap();

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
    let out = encode_mkv(&frames, 8, 8, 15, None, 0, 0).unwrap();

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
    let result = encode_mkv::<&[u8]>(&[], 4, 4, 15, None, 0, 0);
    assert!(result.is_err());
}

/// Verify validation rejects zero dimensions.
#[test]
fn mkv_rejects_zero_dimensions() {
    let frame = vec![0u8; 4];
    assert!(encode_mkv(&[&frame], 0, 4, 15, None, 0, 0).is_err());
    assert!(encode_mkv(&[&frame], 4, 0, 15, None, 0, 0).is_err());
}

/// Verify that the "cnc-formats" muxing/writing app string appears in output.
#[test]
fn mkv_contains_muxing_app_string() {
    let frame = vec![0u8; 4 * 4 * 4];
    let out = encode_mkv(&[&frame], 4, 4, 15, None, 0, 0).unwrap();

    assert!(
        out.windows(11).any(|w| w == b"cnc-formats"),
        "output must contain 'cnc-formats' app string"
    );
}
