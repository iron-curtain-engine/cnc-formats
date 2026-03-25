// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::tests::{build_vqhd, write_u16_le};
use super::*;

use crate::aud::encode_adpcm;
use std::time::Duration;

fn build_small_vqhd(num_frames: u16, audio: Option<(u16, u8, u8)>) -> [u8; 42] {
    let mut hd = build_vqhd(num_frames);
    write_u16_le(&mut hd, 6, 4);
    write_u16_le(&mut hd, 8, 2);
    hd[10] = 4;
    hd[11] = 2;
    hd[13] = 1;
    write_u16_le(&mut hd, 16, 1);
    if let Some((freq, channels, bits)) = audio {
        write_u16_le(&mut hd, 24, freq);
        hd[26] = channels;
        hd[27] = bits;
    } else {
        write_u16_le(&mut hd, 24, 0);
        hd[26] = 0;
        hd[27] = 0;
    }
    hd
}

fn build_chunk(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(fourcc);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    if payload.len() & 1 != 0 {
        out.push(0);
    }
    out
}

fn build_frame_payload(codebook_value: u8) -> Vec<u8> {
    let codebook = vec![codebook_value; 8];
    let vpt = vec![0u8; 2];

    let mut payload = Vec::new();
    payload.extend_from_slice(b"CBF0");
    payload.extend_from_slice(&(codebook.len() as u32).to_be_bytes());
    payload.extend_from_slice(&codebook);
    payload.extend_from_slice(b"VPT0");
    payload.extend_from_slice(&(vpt.len() as u32).to_be_bytes());
    payload.extend_from_slice(&vpt);
    payload
}

fn build_snd1_raw_chunk(samples: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(4usize.saturating_add(samples.len()));
    payload.extend_from_slice(&(samples.len() as u16).to_le_bytes());
    payload.extend_from_slice(&(samples.len() as u16).to_le_bytes());
    payload.extend_from_slice(samples);
    payload
}

fn build_playback_vqa(with_audio: bool) -> Vec<u8> {
    let audio_meta = if with_audio {
        Some((22050, 1, 16))
    } else {
        None
    };
    let vqhd = build_small_vqhd(2, audio_meta);

    let finf = [0u32.to_le_bytes(), 100u32.to_le_bytes()].concat();
    let frame0 = build_frame_payload(1);
    let frame1 = build_frame_payload(2);

    let mut chunks = Vec::new();
    chunks.push(build_chunk(b"VQHD", &vqhd));
    chunks.push(build_chunk(b"FINF", &finf));

    if with_audio {
        let samples0 = vec![120i16; 1470];
        let samples1 = vec![-240i16; 1470];
        let audio0 = encode_adpcm(&samples0, false);
        let audio1 = encode_adpcm(&samples1, false);
        chunks.push(build_chunk(b"SND2", &audio0));
        chunks.push(build_chunk(b"VQFR", &frame0));
        chunks.push(build_chunk(b"SND2", &audio1));
        chunks.push(build_chunk(b"VQFR", &frame1));
    } else {
        chunks.push(build_chunk(b"VQFR", &frame0));
        chunks.push(build_chunk(b"VQFR", &frame1));
    }

    let form_size = 4usize + chunks.iter().map(Vec::len).sum::<usize>();
    let mut out = Vec::new();
    out.extend_from_slice(b"FORM");
    out.extend_from_slice(&(form_size as u32).to_be_bytes());
    out.extend_from_slice(b"WVQA");
    for chunk in &chunks {
        out.extend_from_slice(chunk);
    }
    out
}

fn build_snd1_playback_vqa() -> Vec<u8> {
    let vqhd = build_small_vqhd(1, Some((22050, 1, 8)));
    let frame = build_frame_payload(3);
    let audio = build_snd1_raw_chunk(&[0x10, 0x40, 0x80, 0xFF, 0x20]);
    let chunks = [
        build_chunk(b"VQHD", &vqhd),
        build_chunk(b"SND1", &audio),
        build_chunk(b"VQFR", &frame),
    ];

    let form_size = 4usize + chunks.iter().map(Vec::len).sum::<usize>();
    let mut out = Vec::new();
    out.extend_from_slice(b"FORM");
    out.extend_from_slice(&(form_size as u32).to_be_bytes());
    out.extend_from_slice(b"WVQA");
    for chunk in &chunks {
        out.extend_from_slice(chunk);
    }
    out
}

#[test]
fn decoder_metadata_available_before_decode() {
    let data = build_playback_vqa(true);
    let decoder = VqaDecoder::open(std::io::Cursor::new(data)).unwrap();
    let info = decoder.media_info();
    let seek_index = decoder.seek_index().unwrap();

    assert_eq!(decoder.width(), 4);
    assert_eq!(decoder.height(), 2);
    assert_eq!(decoder.fps(), 15);
    assert_eq!(decoder.frame_count(), 2);
    assert!(decoder.has_audio());
    assert_eq!(decoder.audio_sample_rate(), Some(22050));
    assert_eq!(decoder.audio_channels(), Some(1));
    assert_eq!(decoder.frame_index().map(|index| index.len()), Some(2));
    assert_eq!(decoder.frame_duration(), Duration::new(0, 66_666_666));
    assert_eq!(decoder.frame_timestamp(0), Some(Duration::ZERO));
    assert_eq!(
        decoder.frame_timestamp(1),
        Some(Duration::new(0, 66_666_666))
    );
    assert_eq!(
        decoder.frame_index_for_time(Duration::from_millis(1)),
        Some(0)
    );
    assert_eq!(
        decoder.frame_index_for_time(Duration::from_millis(80)),
        Some(1)
    );
    assert_eq!(
        decoder.audio_sample_frames_per_video_frame(),
        Some((22050, 15))
    );

    let index_entries = decoder.frame_index_entries().unwrap();
    assert_eq!(index_entries.len(), 2);
    assert_eq!(index_entries[0].raw_flags, 0);
    assert_eq!(index_entries[1].raw_offset, 100);
    assert_eq!(info.width, 4);
    assert_eq!(info.height, 2);
    assert_eq!(info.frame_count, 2);
    assert_eq!(info.index_entry_count, 2);
    assert_eq!(info.seek_support, VqaSeekSupport::IndexedLinearFromStart);
    assert_eq!(seek_index.len(), 2);
    assert_eq!(seek_index.entries()[1].byte_offset, 200);
    assert_eq!(
        seek_index.entries()[1].timestamp,
        Duration::new(0, 66_666_666)
    );
}

#[test]
fn decoder_advances_frames_and_audio_chunks_incrementally() {
    let data = build_playback_vqa(true);
    let mut decoder = VqaDecoder::open(std::io::Cursor::new(data)).unwrap();

    let first_frame = decoder.next_frame().unwrap().unwrap();
    assert_eq!(first_frame.index, 0);
    assert!(first_frame.frame.pixels.iter().all(|&pixel| pixel == 1));

    let first_audio = decoder.next_audio_chunk().unwrap().unwrap();
    assert_eq!(first_audio.start_sample_frame, 0);
    assert_eq!(first_audio.sample_frames(), 1470);

    let second_frame = decoder.next_frame().unwrap().unwrap();
    assert_eq!(second_frame.index, 1);
    assert!(second_frame.frame.pixels.iter().all(|&pixel| pixel == 2));

    let second_audio = decoder.next_audio_chunk().unwrap().unwrap();
    assert_eq!(second_audio.start_sample_frame, 1470);
    assert_eq!(second_audio.sample_frames(), 1470);

    assert!(decoder.next_frame().unwrap().is_none());
    assert!(decoder.next_audio_chunk().unwrap().is_none());
}

#[test]
fn decoder_next_frame_into_reuses_caller_buffer() {
    let data = build_playback_vqa(true);
    let mut decoder = VqaDecoder::open(std::io::Cursor::new(data)).unwrap();
    let info = decoder.media_info();
    let mut buffer = VqaFrameBuffer::from_media_info(&info);
    let pixels_ptr = buffer.pixels().as_ptr();

    let first = decoder.next_frame_into(&mut buffer).unwrap().unwrap();
    assert_eq!(first, 0);
    assert_eq!(buffer.pixels().as_ptr(), pixels_ptr);
    assert!(buffer.pixels().iter().all(|&pixel| pixel == 1));

    let second = decoder.next_frame_into(&mut buffer).unwrap().unwrap();
    assert_eq!(second, 1);
    assert_eq!(buffer.pixels().as_ptr(), pixels_ptr);
    assert!(buffer.pixels().iter().all(|&pixel| pixel == 2));
}

#[test]
fn decoder_read_audio_samples_matches_whole_file_audio() {
    let data = build_playback_vqa(true);
    let vqa = VqaFile::parse(&data).unwrap();
    let whole_audio = vqa.extract_audio().unwrap().unwrap();
    let mut decoder = VqaDecoder::open(std::io::Cursor::new(data)).unwrap();
    let mut scratch = [0i16; 257];
    let mut decoded = Vec::new();

    loop {
        let read = decoder.read_audio_samples(&mut scratch).unwrap();
        if read == 0 {
            break;
        }
        decoded.extend_from_slice(scratch.get(..read).unwrap_or(&[]));
    }

    assert_eq!(decoded, whole_audio.samples);
    assert_eq!(decoder.decoded_audio_sample_frames(), 2940);
    assert_eq!(
        decoder.decoded_audio_duration(),
        Some(Duration::new(0, 133_333_333))
    );
}

#[test]
fn decoder_partial_scratch_read_preserves_remaining_audio_chunk() {
    let data = build_playback_vqa(true);
    let mut decoder = VqaDecoder::open(std::io::Cursor::new(data)).unwrap();
    let mut scratch = [0i16; 512];

    let read = decoder.read_audio_samples(&mut scratch).unwrap();
    assert_eq!(read, 512);
    assert_eq!(decoder.decoded_audio_sample_frames(), 512);

    let remainder = decoder.next_audio_chunk().unwrap().unwrap();
    assert_eq!(remainder.start_sample_frame, 512);
    assert_eq!(remainder.sample_frames(), 958);

    let next = decoder.next_audio_chunk().unwrap().unwrap();
    assert_eq!(next.start_sample_frame, 1470);
    assert_eq!(next.sample_frames(), 1470);
}

#[test]
fn decoder_read_audio_samples_matches_snd1_whole_file_audio() {
    let data = build_snd1_playback_vqa();
    let vqa = VqaFile::parse(&data).unwrap();
    let whole_audio = vqa.extract_audio().unwrap().unwrap();
    let mut decoder = VqaDecoder::open(std::io::Cursor::new(data)).unwrap();
    let mut scratch = [0i16; 3];
    let mut decoded = Vec::new();

    loop {
        let read = decoder.read_audio_samples(&mut scratch).unwrap();
        if read == 0 {
            break;
        }
        decoded.extend_from_slice(scratch.get(..read).unwrap_or(&[]));
    }

    assert_eq!(decoded, whole_audio.samples);
}

#[test]
fn decoder_audio_for_frame_interval_is_bounded() {
    let data = build_playback_vqa(true);
    let mut decoder = VqaDecoder::open(std::io::Cursor::new(data)).unwrap();

    let first = decoder.next_audio_for_frame_interval().unwrap().unwrap();
    assert_eq!(first.start_sample_frame, 0);
    assert_eq!(first.sample_frames(), 1470);

    let second = decoder.next_audio_for_frame_interval().unwrap().unwrap();
    assert_eq!(second.start_sample_frame, 1470);
    assert_eq!(second.sample_frames(), 1470);

    let first_frame = decoder.next_frame().unwrap().unwrap();
    let second_frame = decoder.next_frame().unwrap().unwrap();
    assert_eq!(first_frame.index, 0);
    assert_eq!(second_frame.index, 1);
}

#[test]
fn decoder_rewind_replays_same_media() {
    let data = build_playback_vqa(true);
    let mut decoder = VqaDecoder::open(std::io::Cursor::new(data)).unwrap();

    let frame_before = decoder.next_frame().unwrap().unwrap();
    let audio_before = decoder.next_audio_for_frame_interval().unwrap().unwrap();

    decoder.rewind().unwrap();

    let frame_after = decoder.next_frame().unwrap().unwrap();
    let audio_after = decoder.next_audio_for_frame_interval().unwrap().unwrap();

    assert_eq!(frame_before.index, frame_after.index);
    assert_eq!(frame_before.frame.pixels, frame_after.frame.pixels);
    assert_eq!(frame_before.frame.palette, frame_after.frame.palette);
    assert_eq!(
        audio_before.start_sample_frame,
        audio_after.start_sample_frame
    );
    assert_eq!(audio_before.samples, audio_after.samples);
}

#[test]
fn decoder_seek_to_frame_restarts_and_realigns_audio() {
    let data = build_playback_vqa(true);
    let mut decoder = VqaDecoder::open(std::io::Cursor::new(data)).unwrap();

    decoder.seek_to_frame(1).unwrap();
    assert_eq!(decoder.queued_frame_count(), 0);
    assert_eq!(decoder.queued_audio_sample_frames(), 0);

    let frame = decoder.next_frame().unwrap().unwrap();
    assert_eq!(frame.index, 1);
    assert!(frame.frame.pixels.iter().all(|&pixel| pixel == 2));

    let audio = decoder.next_audio_for_frame_interval().unwrap().unwrap();
    assert_eq!(audio.start_sample_frame, 1470);
    assert_eq!(audio.sample_frames(), 1470);
}

#[test]
fn decoder_seek_to_time_clamps_to_requested_frame() {
    let data = build_playback_vqa(true);
    let mut decoder = VqaDecoder::open(std::io::Cursor::new(data)).unwrap();

    decoder.seek_to_time(Duration::from_millis(80)).unwrap();
    let frame = decoder.next_frame().unwrap().unwrap();
    assert_eq!(frame.index, 1);

    decoder.seek_to_time(Duration::from_secs(10)).unwrap();
    let frame = decoder.next_frame().unwrap().unwrap();
    assert_eq!(frame.index, 1);
}

#[test]
fn whole_file_helpers_align_with_incremental_decoder() {
    let data = build_playback_vqa(true);
    let vqa = VqaFile::parse(&data).unwrap();
    let whole_frames = vqa.decode_frames().unwrap();
    let whole_audio = vqa.extract_audio().unwrap().unwrap();

    let mut decoder = VqaDecoder::open(std::io::Cursor::new(&data)).unwrap();
    let mut incremental_frames = Vec::new();
    while let Some(frame) = decoder.next_frame().unwrap() {
        incremental_frames.push(frame.frame);
    }

    let mut incremental_samples = Vec::new();
    while let Some(audio) = decoder.next_audio_chunk().unwrap() {
        incremental_samples.extend_from_slice(&audio.samples);
    }

    assert_eq!(whole_frames.len(), incremental_frames.len());
    for (whole, incremental) in whole_frames.iter().zip(&incremental_frames) {
        assert_eq!(whole.pixels, incremental.pixels);
        assert_eq!(whole.palette, incremental.palette);
    }
    assert_eq!(whole_audio.samples, incremental_samples);
    assert_eq!(whole_audio.sample_rate, 22050);
    assert_eq!(whole_audio.channels, 1);
}

#[test]
fn decoder_handles_audio_less_single_frame() {
    let vqhd = build_small_vqhd(1, None);
    let frame = build_frame_payload(7);
    let form_size = 4 + (8 + vqhd.len()) + (8 + frame.len());

    let mut data = Vec::new();
    data.extend_from_slice(b"FORM");
    data.extend_from_slice(&(form_size as u32).to_be_bytes());
    data.extend_from_slice(b"WVQA");
    data.extend_from_slice(b"VQHD");
    data.extend_from_slice(&(vqhd.len() as u32).to_be_bytes());
    data.extend_from_slice(&vqhd);
    data.extend_from_slice(b"VQFR");
    data.extend_from_slice(&(frame.len() as u32).to_be_bytes());
    data.extend_from_slice(&frame);

    let mut decoder = VqaDecoder::open(std::io::Cursor::new(data)).unwrap();
    assert!(!decoder.has_audio());
    assert!(decoder.next_audio_chunk().unwrap().is_none());
    assert!(decoder.next_audio_for_frame_interval().unwrap().is_none());

    let frame = decoder.next_frame().unwrap().unwrap();
    assert_eq!(frame.index, 0);
    assert!(frame.frame.pixels.iter().all(|&pixel| pixel == 7));
    assert!(decoder.next_frame().unwrap().is_none());
}

#[test]
fn decoder_truncated_media_fails_cleanly() {
    let mut data = build_playback_vqa(true);
    data.truncate(data.len().saturating_sub(5));

    let mut decoder = VqaDecoder::open(std::io::Cursor::new(data)).unwrap();
    let first = decoder.next_frame().unwrap().unwrap();
    assert_eq!(first.index, 0);

    let err = decoder.next_frame().unwrap_err();
    assert!(matches!(
        err,
        Error::UnexpectedEof { .. } | Error::InvalidOffset { .. }
    ));
}

#[test]
fn decoder_reports_drain_state_after_media_consumed() {
    let data = build_playback_vqa(true);
    let mut decoder = VqaDecoder::open(std::io::Cursor::new(data)).unwrap();

    assert!(!decoder.is_drained());
    while decoder.next_frame().unwrap().is_some() {}
    while decoder.next_audio_chunk().unwrap().is_some() {}
    assert!(decoder.is_drained());
}

// ─── Streaming-vs-batch correctness proofs ──────────────────────────────────

/// Proves `next_audio_for_frame_interval` collects all audio — the bounded
/// frame-interval API must produce the same total samples as batch extraction.
#[test]
fn frame_interval_audio_total_matches_batch() {
    let data = build_playback_vqa(true);
    let vqa = VqaFile::parse(&data).unwrap();
    let whole_audio = vqa.extract_audio().unwrap().unwrap();

    let mut decoder = VqaDecoder::open(std::io::Cursor::new(&data)).unwrap();
    let mut interval_samples = Vec::new();
    let mut chunk_count = 0usize;
    while let Some(chunk) = decoder.next_audio_for_frame_interval().unwrap() {
        interval_samples.extend_from_slice(&chunk.samples);
        chunk_count = chunk_count.saturating_add(1);
    }

    assert!(
        chunk_count >= 2,
        "test requires multiple interval chunks to be meaningful"
    );
    assert_eq!(
        interval_samples, whole_audio.samples,
        "frame-interval audio total must match batch extract_audio"
    );
}

/// Builds a VQA file with SND0 (raw PCM16) audio chunks.
fn build_snd0_playback_vqa() -> Vec<u8> {
    let vqhd = build_small_vqhd(2, Some((22050, 1, 16)));
    let frame0 = build_frame_payload(1);
    let frame1 = build_frame_payload(2);

    // SND0 = raw PCM16 LE samples (no compression framing).
    let samples0: Vec<i16> = (0..735).map(|i| (i * 3) as i16).collect();
    let samples1: Vec<i16> = (0..735).map(|i| -(i * 5) as i16).collect();

    let mut audio0 = Vec::new();
    for s in &samples0 {
        audio0.extend_from_slice(&s.to_le_bytes());
    }
    let mut audio1 = Vec::new();
    for s in &samples1 {
        audio1.extend_from_slice(&s.to_le_bytes());
    }

    let chunks = [
        build_chunk(b"VQHD", &vqhd),
        build_chunk(b"SND0", &audio0),
        build_chunk(b"VQFR", &frame0),
        build_chunk(b"SND0", &audio1),
        build_chunk(b"VQFR", &frame1),
    ];

    let form_size = 4usize + chunks.iter().map(Vec::len).sum::<usize>();
    let mut out = Vec::new();
    out.extend_from_slice(b"FORM");
    out.extend_from_slice(&(form_size as u32).to_be_bytes());
    out.extend_from_slice(b"WVQA");
    for chunk in &chunks {
        out.extend_from_slice(chunk);
    }
    out
}

/// Proves SND0 (raw PCM16) audio streaming matches batch extraction.
///
/// SND0 is the simplest audio path — no ADPCM state to carry — so this test
/// validates that the streaming infrastructure itself is correct, independent
/// of codec complexity.
#[test]
fn snd0_pcm16_streaming_matches_batch() {
    let data = build_snd0_playback_vqa();
    let vqa = VqaFile::parse(&data).unwrap();
    let whole_audio = vqa.extract_audio().unwrap().unwrap();
    assert_eq!(whole_audio.samples.len(), 1470);

    let mut decoder = VqaDecoder::open(std::io::Cursor::new(data)).unwrap();
    let mut scratch = [0i16; 100]; // small buffer to force multiple iterations
    let mut decoded = Vec::new();

    loop {
        let read = decoder.read_audio_samples(&mut scratch).unwrap();
        if read == 0 {
            break;
        }
        decoded.extend_from_slice(scratch.get(..read).unwrap_or(&[]));
    }

    assert_eq!(
        decoded, whole_audio.samples,
        "SND0 streaming must match batch extract_audio"
    );
}

/// Proves `next_audio_chunk` collects all SND0 audio and matches batch.
#[test]
fn snd0_next_audio_chunk_matches_batch() {
    let data = build_snd0_playback_vqa();
    let vqa = VqaFile::parse(&data).unwrap();
    let whole_audio = vqa.extract_audio().unwrap().unwrap();

    let mut decoder = VqaDecoder::open(std::io::Cursor::new(data)).unwrap();
    let mut chunk_samples = Vec::new();
    while let Some(chunk) = decoder.next_audio_chunk().unwrap() {
        chunk_samples.extend_from_slice(&chunk.samples);
    }

    assert_eq!(
        chunk_samples, whole_audio.samples,
        "SND0 next_audio_chunk total must match batch"
    );
}

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
    assert_eq!(audio0.samples, vec![18432i16], "chunk 0: raw-copy 200 → 18432");

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
