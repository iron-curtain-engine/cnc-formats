// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Returns the byte offsets and payload sizes of all `01wb` audio chunks.
///
/// Why: AVI's timing behavior is encoded in the movi chunk layout, so the
/// encoder tests inspect the written chunk sizes directly instead of relying
/// on a player-specific interpretation.
fn movi_audio_chunks(data: &[u8]) -> Vec<(usize, u32)> {
    let mut movi_payload_start = None;
    let mut movi_payload_end = None;
    let mut pos = 12usize;

    while pos.saturating_add(12) <= data.len() {
        let fourcc = data.get(pos..pos.saturating_add(4)).unwrap_or(&[]);
        if fourcc != b"LIST" {
            let size = data
                .get(pos.saturating_add(4)..pos.saturating_add(8))
                .map(|s| {
                    let mut buf = [0u8; 4];
                    buf.copy_from_slice(s);
                    u32::from_le_bytes(buf) as usize
                })
                .unwrap_or(0);
            pos = pos
                .saturating_add(8)
                .saturating_add(size)
                .saturating_add(size & 1);
            continue;
        }

        let size = data
            .get(pos.saturating_add(4)..pos.saturating_add(8))
            .map(|s| {
                let mut buf = [0u8; 4];
                buf.copy_from_slice(s);
                u32::from_le_bytes(buf) as usize
            })
            .unwrap_or(0);
        let list_type = data
            .get(pos.saturating_add(8)..pos.saturating_add(12))
            .unwrap_or(&[]);
        if list_type == b"movi" {
            movi_payload_start = Some(pos.saturating_add(12));
            movi_payload_end = Some(pos.saturating_add(8).saturating_add(size));
            break;
        }

        pos = pos
            .saturating_add(8)
            .saturating_add(size)
            .saturating_add(size & 1);
    }

    let start = movi_payload_start.unwrap_or(0);
    let end = movi_payload_end.unwrap_or(0).min(data.len());
    let movi = data.get(start..end).unwrap_or(&[]);

    let mut chunks = Vec::new();
    let mut inner = 0usize;
    while inner.saturating_add(8) <= movi.len() {
        let fourcc = movi.get(inner..inner.saturating_add(4)).unwrap_or(&[]);
        let size = movi
            .get(inner.saturating_add(4)..inner.saturating_add(8))
            .map(|s| {
                let mut buf = [0u8; 4];
                buf.copy_from_slice(s);
                u32::from_le_bytes(buf)
            })
            .unwrap_or(0);
        if fourcc == b"01wb" {
            chunks.push((start.saturating_add(inner), size));
        }

        let padded = (size as usize).saturating_add((size as usize) & 1);
        inner = inner.saturating_add(8).saturating_add(padded);
    }

    chunks
}

/// Patches a little-endian u32 inside a mutable byte buffer.
fn patch_u32_le(buf: &mut [u8], offset: usize, value: u32) {
    if let Some(dst) = buf.get_mut(offset..offset.saturating_add(4)) {
        dst.copy_from_slice(&value.to_le_bytes());
    }
}

// ── AVI timing and validation ───────────────────────────────────────────────

/// Audio chunks are distributed by frame duration, not front-loaded.
///
/// Why: when `sample_rate / fps` is fractional, the encoder must carry the
/// remainder across frames instead of dumping a half-second of audio into the
/// first chunk and skewing playback timing.
#[test]
fn avi_audio_chunks_track_fractional_frame_timing() {
    let width = 2;
    let height = 2;
    let frames = vec![vec![0u8; 16], vec![0u8; 16], vec![0u8; 16]];
    let audio: Vec<i16> = (0..10).collect();

    let avi_data = avi::encode_avi(&frames, width, height, 3, Some(&audio), 10, 1).unwrap();
    let chunk_sizes: Vec<u32> = movi_audio_chunks(&avi_data)
        .into_iter()
        .map(|(_, size)| size)
        .collect();

    assert_eq!(chunk_sizes, vec![6, 6, 8]);
}

/// AVI decode rejects video chunks whose payload is shorter than dimensions.
///
/// Why: silently zero-filling truncated frames hides container corruption and
/// produces pixel data that never existed in the file.
#[test]
fn avi_decode_rejects_short_video_payload() {
    let frame = vec![255u8; 4 * 4 * 4];
    let mut avi_data = avi::encode_avi(&[frame], 4, 4, 15, None, 0, 0).unwrap();
    let video_chunk = movi_audio_chunks(&avi_data);
    assert!(video_chunk.is_empty(), "fixture should contain video only");

    let video_pos = avi_data
        .windows(4)
        .position(|w| w == b"00dc")
        .expect("encoded AVI should contain a 00dc chunk");
    let size_offset = video_pos.saturating_add(4);
    let original_size = avi_data
        .get(size_offset..size_offset.saturating_add(4))
        .map(|s| {
            let mut buf = [0u8; 4];
            buf.copy_from_slice(s);
            u32::from_le_bytes(buf)
        })
        .unwrap_or(0);
    patch_u32_le(&mut avi_data, size_offset, original_size.saturating_sub(1));

    let err = match avi::decode_avi(&avi_data) {
        Ok(_) => panic!("short video payload must be rejected"),
        Err(err) => err,
    };
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// Passing audio with zero channels does not emit a broken audio stream.
///
/// Why: zero-channel PCM is structurally invalid, so the encoder should treat
/// it as "no audio" instead of writing unusable headers.
#[test]
fn avi_zero_channels_omits_audio_stream() {
    let frames = vec![vec![0u8; 16]];
    let audio = vec![1i16, 2, 3, 4];

    let avi_data = avi::encode_avi(&frames, 2, 2, 15, Some(&audio), 22050, 0).unwrap();
    let decoded = avi::decode_avi(&avi_data).unwrap();

    assert_eq!(decoded.channels, 0);
    assert!(decoded.audio.is_empty());
    assert_eq!(decoded.sample_rate, 0);
}

/// Stereo PCM round-trips at an extreme but valid sample rate.
///
/// Why: high sample rates exercise the AVI header arithmetic and audio chunk
/// sizing without relying on the common 22.05/44.1 kHz paths only.
#[test]
fn avi_round_trip_high_sample_rate_stereo() {
    let frames = vec![vec![0u8; 16], vec![128u8; 16]];
    let audio: Vec<i16> = (0..512).map(|i| (i as i16).wrapping_mul(17)).collect();

    let avi_data = avi::encode_avi(&frames, 2, 2, 60, Some(&audio), 192_000, 2).unwrap();
    let decoded = avi::decode_avi(&avi_data).unwrap();

    assert_eq!(decoded.sample_rate, 192_000);
    assert_eq!(decoded.channels, 2);
    assert_eq!(decoded.audio, audio);
}
