// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! VQA playback decoder tests (second half) — split from `tests_playback.rs`
//! to keep files under the ~600-line cap.
//!
//! Covers: PcmDirect variant correctness and SND1 cross-chunk state
//! continuity.

use super::tests_playback::{
    build_chunk, build_frame_payload, build_playback_vqa, build_small_vqhd, build_snd1_raw_chunk,
};
use super::*;

// ─── PcmDirect variant tests ─────────────────────────────────────────────────

/// `PcmDirect` must report remaining count correctly and drain to empty.
#[test]
fn pcm_direct_reports_remaining_count_and_drains() {
    use super::snd::VqaAudioChunkDecoder;

    let samples = vec![100i16, 200, -300, 400, -500];
    let mut decoder = VqaAudioChunkDecoder::PcmDirect { samples, pos: 0 };

    assert_eq!(decoder.remaining_sample_count(), 5);
    assert!(!decoder.is_finished());

    let mut buf = [0i16; 3];
    let n = decoder.read_samples(&mut buf).unwrap();
    assert_eq!(n, 3);
    assert_eq!(&buf[..3], &[100, 200, -300]);
    assert_eq!(decoder.remaining_sample_count(), 2);
    assert!(!decoder.is_finished());

    let mut buf2 = [0i16; 10];
    let n2 = decoder.read_samples(&mut buf2).unwrap();
    assert_eq!(n2, 2);
    assert_eq!(&buf2[..2], &[400, -500]);
    assert_eq!(decoder.remaining_sample_count(), 0);
    assert!(decoder.is_finished());
}

/// SND2 output must be identical before and after the `PcmDirect` refactor.
///
/// This test builds two SND2 chunks, decodes them via the full streaming
/// pipeline, and verifies the result matches batch `extract_audio` — proving
/// the `PcmDirect` path produces bit-identical output to the old byte-roundtrip.
#[test]
fn snd2_pcm_direct_matches_batch_extraction() {
    use crate::aud::encode_adpcm;
    let data = build_playback_vqa(true); // uses SND2 chunks
    let vqa = VqaFile::parse(&data).unwrap();
    let batch = vqa.extract_audio().unwrap().unwrap();

    let mut decoder = VqaDecoder::open(std::io::Cursor::new(&data)).unwrap();
    let mut streamed = Vec::new();
    while let Some(chunk) = decoder.next_audio_chunk().unwrap() {
        streamed.extend_from_slice(&chunk.samples);
    }

    assert_eq!(
        streamed, batch.samples,
        "SND2 PcmDirect streaming must match batch extraction"
    );
    let _ = encode_adpcm; // suppress unused import warning in case of no std
}

// ─── SND1 state continuity tests ─────────────────────────────────────────────

/// SND1 `cur_sample` must carry from the last sample of chunk N to the start
/// of chunk N+1.
///
/// Setup:
///   Chunk 0 — raw-copy of `[200u8]`: sets `cur_sample = 200`, outputs
///             `pcm8_to_i16(200) = (200-128)*256 = 18432`.
///   Chunk 1 — repeat ×2 (opcode 0xC4, compressed size 1, output size 2):
///             repeats the *current* `cur_sample`.
///
///   With state carry:    `cur_sample = 200` → both samples = 18432.
///   Without state carry: `cur_sample = 0x80 = 128` → both samples = 0.
#[test]
fn snd1_state_carries_across_chunk_boundaries() {
    let vqhd = build_small_vqhd(2, Some((22050, 1, 8)));
    let frame0 = build_frame_payload(1);
    let frame1 = build_frame_payload(2);

    // Chunk 0: raw-copy of 1 sample with value 200.
    // out_size == size → raw-copy mode → cur_sample is set to 200.
    let chunk0_snd1 = build_snd1_raw_chunk(&[200u8]);

    // Chunk 1: repeat ×2 using opcode 0xC4.
    // out_size=2, size=1 → ADPCM mode (Idle → reads opcode).
    // Opcode 0xC4 = 1100_0100 → top 2 bits = 11 → Repeat, count_raw = 1 → remaining = 2.
    let mut chunk1_snd1 = Vec::new();
    chunk1_snd1.extend_from_slice(&2u16.to_le_bytes()); // out_size = 2
    chunk1_snd1.extend_from_slice(&1u16.to_le_bytes()); // in_size  = 1
    chunk1_snd1.push(0xC4); // Repeat ×2

    let chunks = [
        build_chunk(b"VQHD", &vqhd),
        build_chunk(b"SND1", &chunk0_snd1),
        build_chunk(b"VQFR", &frame0),
        build_chunk(b"SND1", &chunk1_snd1),
        build_chunk(b"VQFR", &frame1),
    ];

    let form_size = 4usize + chunks.iter().map(Vec::len).sum::<usize>();
    let mut data = Vec::new();
    data.extend_from_slice(b"FORM");
    data.extend_from_slice(&(form_size as u32).to_be_bytes());
    data.extend_from_slice(b"WVQA");
    for chunk in &chunks {
        data.extend_from_slice(chunk);
    }

    let mut decoder = VqaDecoder::open(std::io::Cursor::new(&data)).unwrap();

    // Advance past frame 0, which queues chunk 0 audio.
    decoder.next_frame().unwrap();
    let audio0 = decoder.next_audio_chunk().unwrap().unwrap();
    assert_eq!(
        audio0.samples,
        vec![18432i16],
        "chunk 0: raw-copy 200 → 18432"
    );

    // Advance to frame 1, which queues chunk 1 audio.
    decoder.next_frame().unwrap();
    let audio1 = decoder.next_audio_chunk().unwrap().unwrap();
    assert_eq!(
        audio1.samples,
        vec![18432i16, 18432],
        "chunk 1 repeat must carry cur_sample=200 from prior chunk, not reset to 0x80"
    );
}

/// Batch `extract_audio` must carry SND1 state identically to the streaming path.
#[test]
fn snd1_batch_state_carry_matches_streaming() {
    let vqhd = build_small_vqhd(2, Some((22050, 1, 8)));
    let frame0 = build_frame_payload(1);
    let frame1 = build_frame_payload(2);

    let chunk0_snd1 = build_snd1_raw_chunk(&[200u8]);
    let mut chunk1_snd1 = Vec::new();
    chunk1_snd1.extend_from_slice(&2u16.to_le_bytes());
    chunk1_snd1.extend_from_slice(&1u16.to_le_bytes());
    chunk1_snd1.push(0xC4); // Repeat ×2

    let chunks = [
        build_chunk(b"VQHD", &vqhd),
        build_chunk(b"SND1", &chunk0_snd1),
        build_chunk(b"VQFR", &frame0),
        build_chunk(b"SND1", &chunk1_snd1),
        build_chunk(b"VQFR", &frame1),
    ];

    let form_size = 4usize + chunks.iter().map(Vec::len).sum::<usize>();
    let mut data = Vec::new();
    data.extend_from_slice(b"FORM");
    data.extend_from_slice(&(form_size as u32).to_be_bytes());
    data.extend_from_slice(b"WVQA");
    for chunk in &chunks {
        data.extend_from_slice(chunk);
    }

    let vqa = VqaFile::parse(&data).unwrap();
    let batch = vqa.extract_audio().unwrap().unwrap();

    let mut decoder = VqaDecoder::open(std::io::Cursor::new(&data)).unwrap();
    let mut streamed = Vec::new();
    while let Some(chunk) = decoder.next_audio_chunk().unwrap() {
        streamed.extend_from_slice(&chunk.samples);
    }

    assert_eq!(
        batch.samples, streamed,
        "SND1 batch and streaming must produce identical output (both stateful)"
    );
}
