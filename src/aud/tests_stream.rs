// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

use super::tests::make_header_bytes;
use super::*;

use std::io::Cursor;
use std::time::Duration;

/// `AudStream::open` reads header fields and exposes them through `header()` and `media_info()`.
///
/// Verifies that sample rate, channel count, compressed size, uncompressed size, sample frame
/// count, and seek-support flag are all correctly propagated from the on-disk header.
#[test]
fn stream_open_reads_header() {
    let bytes = make_header_bytes(22050, 4, 8, AUD_FLAG_16BIT, SCOMP_WESTWOOD);
    let stream = AudStream::open(Cursor::new(bytes)).unwrap();
    let info = stream.media_info();
    assert_eq!(stream.header().sample_rate, 22050);
    assert_eq!(stream.header().compressed_size, 4);
    assert_eq!(stream.header().uncompressed_size, 8);
    assert_eq!(stream.channels(), 1);
    assert_eq!(stream.sample_frames(), 4);
    assert!(stream.duration().is_some());
    assert_eq!(info.sample_rate, 22050);
    assert_eq!(info.channels, 1);
    assert_eq!(info.bits_per_sample, 16);
    assert_eq!(info.seek_support, AudSeekSupport::None);
}

/// `AudStream::read_samples` produces identical output to batch `decode_adpcm`.
///
/// Why: the streaming and batch paths must be byte-for-byte equivalent — any
/// divergence would cause audio glitches when the streaming API is used in place of
/// the batch decoder.
#[test]
fn stream_matches_buffered_adpcm_decode() {
    let compressed = [0x07u8, 0x70, 0x11, 0x88];
    let mut bytes = make_header_bytes(
        22050,
        compressed.len() as u32,
        16,
        AUD_FLAG_16BIT,
        SCOMP_WESTWOOD,
    );
    bytes.extend_from_slice(&compressed);

    let expected = decode_adpcm(&compressed, false, 8);
    let mut stream = AudStream::open(Cursor::new(bytes)).unwrap();
    let mut decoded = Vec::new();
    let mut chunk = [0i16; 3];

    loop {
        let read = stream.read_samples(&mut chunk).unwrap();
        if read == 0 {
            break;
        }
        decoded.extend_from_slice(chunk.get(..read).unwrap_or(&[]));
    }

    assert_eq!(decoded, expected);
}

/// `AudStream` with `SCOMP_SOS` correctly strips the per-chunk header (size, uncompressed size,
/// 0x0000DEAF magic) and decodes only the raw ADPCM nibbles.
///
/// Why: SOS files interleave chunk framing with compressed data; if the framing bytes are fed
/// to the ADPCM decoder the output will be garbage.
#[test]
fn stream_scomp99_strips_chunk_headers() {
    let first = [0x07u8, 0x70];
    let second = [0x11u8, 0x88];
    let mut payload = Vec::new();
    for chunk in [first.as_slice(), second.as_slice()] {
        payload.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
        payload.extend_from_slice(&8u16.to_le_bytes());
        payload.extend_from_slice(&0x0000_DEAFu32.to_le_bytes());
        payload.extend_from_slice(chunk);
    }

    let mut bytes = make_header_bytes(22050, payload.len() as u32, 16, AUD_FLAG_16BIT, SCOMP_SOS);
    bytes.extend_from_slice(&payload);

    let mut raw = Vec::new();
    raw.extend_from_slice(&first);
    raw.extend_from_slice(&second);
    let expected = decode_adpcm(&raw, false, 8);

    let mut stream = AudStream::open(Cursor::new(bytes)).unwrap();
    let mut decoded = Vec::new();
    let mut chunk = [0i16; 1];

    loop {
        let read = stream.read_samples(&mut chunk).unwrap();
        if read == 0 {
            break;
        }
        decoded.extend_from_slice(chunk.get(..read).unwrap_or(&[]));
    }

    assert_eq!(decoded, expected);
}

/// `AudStream` with `SCOMP_NONE` passes raw 16-bit PCM samples through without decoding.
///
/// Why: uncompressed AUD files exist in some game assets; the stream must forward them
/// unchanged rather than running the ADPCM decoder on raw PCM bytes.
#[test]
fn stream_scomp_none_pcm16() {
    let samples = [100i16, -200, 300, -400];
    let mut payload = Vec::new();
    for sample in samples {
        payload.extend_from_slice(&sample.to_le_bytes());
    }

    let mut bytes = make_header_bytes(
        11025,
        payload.len() as u32,
        payload.len() as u32,
        AUD_FLAG_16BIT,
        SCOMP_NONE,
    );
    bytes.extend_from_slice(&payload);

    let mut stream = AudStream::open(Cursor::new(bytes)).unwrap();
    let mut decoded = [0i16; 4];
    let read = stream.read_samples(&mut decoded).unwrap();

    assert_eq!(read, 4);
    assert_eq!(decoded, samples);
}

/// A truncated SOS chunk payload (declared size 4, only 2 bytes present) returns `UnexpectedEof`.
///
/// Why (V38): malformed SOS chunk headers could otherwise cause the decoder to read past the
/// end of the buffer; the stream must detect and reject incomplete payloads.
#[test]
fn stream_rejects_truncated_scomp99_chunk_payload() {
    let mut payload = Vec::new();
    payload.extend_from_slice(&4u16.to_le_bytes());
    payload.extend_from_slice(&8u16.to_le_bytes());
    payload.extend_from_slice(&0x0000_DEAFu32.to_le_bytes());
    payload.extend_from_slice(&[0x07, 0x70]);

    let mut bytes = make_header_bytes(22050, payload.len() as u32, 16, AUD_FLAG_16BIT, SCOMP_SOS);
    bytes.extend_from_slice(&payload);

    let mut stream = AudStream::open(Cursor::new(bytes)).unwrap();
    let mut out = [0i16; 8];
    let err = stream.read_samples(&mut out).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// `AudStream::rewind` resets the seekable stream to the start, replaying identical samples.
///
/// Why: rewind is the prerequisite for looping audio; if it produces different samples on the
/// second pass the loop will contain an audible glitch.
#[test]
fn seekable_stream_rewind_replays_samples() {
    let compressed = [0x07u8, 0x70, 0x11, 0x88];
    let mut bytes = make_header_bytes(
        22050,
        compressed.len() as u32,
        16,
        AUD_FLAG_16BIT,
        SCOMP_WESTWOOD,
    );
    bytes.extend_from_slice(&compressed);

    let mut stream = AudStream::open_seekable(Cursor::new(bytes)).unwrap();
    let mut first = [0i16; 4];
    let mut second = [0i16; 4];

    let read = stream.read_samples(&mut first).unwrap();
    assert_eq!(read, 4);

    stream.rewind().unwrap();

    let read = stream.read_samples(&mut second).unwrap();
    assert_eq!(read, 4);
    assert_eq!(first, second);
}

/// `AudStream::next_chunk` returns chunks with correct `start_sample_frame`, sample count,
/// start timestamp, and updates the stream's progress counters after each call.
///
/// Why: the chunk API is consumed by audio mixers that must schedule samples at precise
/// playback timestamps; wrong offsets or counts would cause audio desynchronisation.
#[test]
fn stream_next_chunk_reports_timing_and_progress() {
    let compressed = [0x07u8, 0x70, 0x11, 0x88];
    let mut bytes = make_header_bytes(
        22050,
        compressed.len() as u32,
        16,
        AUD_FLAG_16BIT,
        SCOMP_WESTWOOD,
    );
    bytes.extend_from_slice(&compressed);

    let mut stream = AudStream::open(Cursor::new(bytes)).unwrap();
    assert_eq!(stream.decoded_sample_frames(), 0);
    assert_eq!(stream.remaining_sample_frames(), 8);
    assert_eq!(stream.decoded_duration(), Some(Duration::ZERO));

    let first = stream.next_chunk(3).unwrap().unwrap();
    assert_eq!(first.start_sample_frame, 0);
    assert_eq!(first.sample_frames(), 3);
    assert_eq!(first.start_time(), Some(Duration::ZERO));
    assert_eq!(stream.decoded_sample_frames(), 3);
    assert_eq!(stream.remaining_sample_frames(), 5);

    let second = stream.next_chunk(3).unwrap().unwrap();
    assert_eq!(second.start_sample_frame, 3);
    assert_eq!(second.sample_frames(), 3);
    assert_eq!(
        second.start_time(),
        stream.header().sample_frame_timestamp(3),
    );
    assert_eq!(stream.decoded_sample_frames(), 6);
    assert_eq!(stream.remaining_sample_frames(), 2);

    let third = stream.next_chunk(3).unwrap().unwrap();
    assert_eq!(third.start_sample_frame, 6);
    assert_eq!(third.sample_frames(), 2);
    assert_eq!(stream.decoded_sample_frames(), 8);
    assert_eq!(stream.remaining_sample_frames(), 0);
    assert!(stream.next_chunk(3).unwrap().is_none());
}

/// `AudStream::restart` resets chunk-based playback to the beginning of the stream.
///
/// Why: correctness of `restart` must be verified independently of `rewind` — both use seek
/// under the hood but operate through different code paths in the chunk-queue API.
#[test]
fn seekable_stream_restart_replays_chunks() {
    let compressed = [0x07u8, 0x70, 0x11, 0x88];
    let mut bytes = make_header_bytes(
        22050,
        compressed.len() as u32,
        16,
        AUD_FLAG_16BIT,
        SCOMP_WESTWOOD,
    );
    bytes.extend_from_slice(&compressed);

    let mut stream = AudStream::open_seekable(Cursor::new(bytes)).unwrap();
    let first = stream.next_chunk(4).unwrap().unwrap();
    stream.restart().unwrap();
    let second = stream.next_chunk(4).unwrap().unwrap();

    assert_eq!(first.start_sample_frame, second.start_sample_frame);
    assert_eq!(first.samples, second.samples);
}

/// `AudStream::open_seekable` reports `AudSeekSupport::Restart` in both `seek_support()`
/// and `media_info().seek_support`.
///
/// Why: consumers query `media_info` to decide whether to offer looping or scrubbing UI;
/// a mismatch between the two accessors would cause incorrect capability detection.
#[test]
fn seekable_stream_reports_restart_support() {
    let compressed = [0x07u8, 0x70, 0x11, 0x88];
    let mut bytes = make_header_bytes(
        22050,
        compressed.len() as u32,
        16,
        AUD_FLAG_16BIT,
        SCOMP_WESTWOOD,
    );
    bytes.extend_from_slice(&compressed);

    let stream = AudStream::open_seekable(Cursor::new(bytes)).unwrap();
    let info = stream.media_info();

    assert_eq!(stream.seek_support(), AudSeekSupport::Restart);
    assert_eq!(info.seek_support, AudSeekSupport::Restart);
}

/// `AudStream::try_resync` scans past garbage bytes and finds the next valid SOS chunk header.
///
/// Why: network-streamed or partially corrupted AUD data may have isolated bad bytes; a
/// resync capability allows the decoder to recover and continue playing rather than hard-failing.
///
/// How: two valid SOS chunks are written with garbage bytes between them; the stream is
/// deliberately positioned mid-stream so the next read hits the corruption, then `try_resync`
/// scans forward to the second chunk's `0x0000DEAF` magic.
#[test]
fn try_resync_finds_next_deaf_magic() {
    // Build a valid SOS AUD with two chunks, then inject garbage between them.
    let chunk_data = [0x55u8, 0xAA];
    let mut payload = Vec::new();
    // First chunk (valid).
    payload.extend_from_slice(&(chunk_data.len() as u16).to_le_bytes());
    payload.extend_from_slice(&8u16.to_le_bytes());
    payload.extend_from_slice(&0x0000_DEAFu32.to_le_bytes());
    payload.extend_from_slice(&chunk_data);
    // Garbage bytes (simulating corruption).
    payload.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02]);
    // Second valid chunk.
    payload.extend_from_slice(&(chunk_data.len() as u16).to_le_bytes());
    payload.extend_from_slice(&8u16.to_le_bytes());
    payload.extend_from_slice(&0x0000_DEAFu32.to_le_bytes());
    payload.extend_from_slice(&chunk_data);

    let mut bytes = make_header_bytes(22050, payload.len() as u32, 32, AUD_FLAG_16BIT, SCOMP_SOS);
    bytes.extend_from_slice(&payload);

    let mut stream = AudStream::open_seekable(Cursor::new(bytes)).unwrap();
    // Read the first chunk — this will decode samples from chunk 1, then
    // attempt to read chunk 2's header and fail on the garbage.
    let mut scratch = [0i16; 64];
    let result = stream.read_samples(&mut scratch);
    // The read fails because the garbage doesn't have 0x0000DEAF magic.
    assert!(result.is_err(), "should fail on corrupted chunk header");

    // Resync should scan forward and find the second valid DEAF magic.
    let found = stream.try_resync().unwrap();
    assert!(found, "try_resync should find the second DEAF magic");
}

/// `AudStream::try_resync` returns `false` for non-SOS (Westwood/None) streams.
///
/// Why: the DEAF magic scan is only meaningful for SOS-framed data; returning `false` for
/// other codecs lets callers distinguish "corrupt SOS stream" from "non-SOS codec".
#[test]
fn try_resync_returns_false_for_non_sos() {
    let compressed = [0x07u8, 0x70, 0x11, 0x88];
    let mut bytes = make_header_bytes(
        22050,
        compressed.len() as u32,
        16,
        AUD_FLAG_16BIT,
        SCOMP_WESTWOOD,
    );
    bytes.extend_from_slice(&compressed);

    let mut stream = AudStream::open_seekable(Cursor::new(bytes)).unwrap();
    let found = stream.try_resync().unwrap();
    assert!(!found, "try_resync should return false for non-SOS streams");
}

// ─── Streaming-vs-batch correctness proofs ──────────────────────────────────

/// Proves mono streaming correctness with a larger payload that forces multiple
/// `read_samples` iterations through a small scratch buffer.
#[test]
fn stream_larger_mono_matches_batch() {
    let compressed: Vec<u8> = (0..64u8)
        .map(|i| i.wrapping_mul(37).wrapping_add(5))
        .collect();
    let uncompressed_samples = compressed.len() * 2; // 2 nibbles per byte
    let mut bytes = make_header_bytes(
        22050,
        compressed.len() as u32,
        (uncompressed_samples * 2) as u32,
        AUD_FLAG_16BIT,
        SCOMP_WESTWOOD,
    );
    bytes.extend_from_slice(&compressed);

    let expected = decode_adpcm(&compressed, false, 0);
    assert_eq!(expected.len(), 128);

    let mut stream = AudStream::open(Cursor::new(bytes)).unwrap();
    let mut decoded = Vec::new();
    let mut scratch = [0i16; 13]; // prime-sized to split across nibble boundaries

    loop {
        let read = stream.read_samples(&mut scratch).unwrap();
        if read == 0 {
            break;
        }
        decoded.extend_from_slice(scratch.get(..read).unwrap_or(&[]));
    }

    assert_eq!(
        decoded, expected,
        "streaming must produce identical samples to batch"
    );
}

/// Proves stereo streaming correctness: channel interleaving is identical
/// between batch `decode_adpcm` and incremental `AudStream::read_samples`.
#[test]
fn stream_stereo_matches_batch() {
    // 32 bytes → 16 byte-pairs → 64 samples (32 per channel, interleaved).
    let compressed: Vec<u8> = (0..32u8)
        .map(|i| i.wrapping_mul(53).wrapping_add(17))
        .collect();
    let uncompressed_samples = compressed.len() * 2;
    let mut bytes = make_header_bytes(
        22050,
        compressed.len() as u32,
        (uncompressed_samples * 2) as u32,
        AUD_FLAG_STEREO | AUD_FLAG_16BIT,
        SCOMP_WESTWOOD,
    );
    bytes.extend_from_slice(&compressed);

    let expected = decode_adpcm(&compressed, true, 0);
    assert_eq!(expected.len(), 64);

    let mut stream = AudStream::open(Cursor::new(bytes)).unwrap();
    assert_eq!(stream.channels(), 2);

    let mut decoded = Vec::new();
    let mut scratch = [0i16; 7]; // odd size to stress boundary handling

    loop {
        let read = stream.read_samples(&mut scratch).unwrap();
        if read == 0 {
            break;
        }
        decoded.extend_from_slice(scratch.get(..read).unwrap_or(&[]));
    }

    assert_eq!(
        decoded, expected,
        "stereo streaming must match batch decode"
    );
}

/// Proves `next_chunk` reassembly produces the same samples as `read_samples`.
///
/// This validates the queue-friendly API by collecting all chunks, concatenating
/// their samples, and comparing to the flat `read_samples` output.
#[test]
fn next_chunk_reassembly_matches_read_samples() {
    let compressed: Vec<u8> = (0..48u8)
        .map(|i| i.wrapping_mul(41).wrapping_add(9))
        .collect();
    let uncompressed_samples = compressed.len() * 2;
    let mut bytes = make_header_bytes(
        22050,
        compressed.len() as u32,
        (uncompressed_samples * 2) as u32,
        AUD_FLAG_16BIT,
        SCOMP_WESTWOOD,
    );
    bytes.extend_from_slice(&compressed);

    // Collect via read_samples.
    let mut stream_flat = AudStream::open(Cursor::new(&bytes)).unwrap();
    let mut flat_samples = Vec::new();
    let mut scratch = [0i16; 96];
    loop {
        let read = stream_flat.read_samples(&mut scratch).unwrap();
        if read == 0 {
            break;
        }
        flat_samples.extend_from_slice(scratch.get(..read).unwrap_or(&[]));
    }

    // Collect via next_chunk (small chunks to force many iterations).
    let mut stream_chunked = AudStream::open(Cursor::new(&bytes)).unwrap();
    let mut chunk_samples = Vec::new();
    let mut chunk_count = 0usize;
    while let Some(chunk) = stream_chunked.next_chunk(11).unwrap() {
        assert!(chunk.sample_frames() <= 11);
        chunk_samples.extend_from_slice(&chunk.samples);
        chunk_count = chunk_count.saturating_add(1);
    }

    assert!(
        chunk_count > 1,
        "test requires multiple chunks to be meaningful"
    );
    assert_eq!(
        chunk_samples, flat_samples,
        "next_chunk reassembly must match read_samples"
    );
}

/// Proves `AudStream::from_payload` (the export path used by `aud_to_wav`)
/// produces the same samples as direct batch `decode_adpcm` on raw ADPCM bytes.
///
/// This is the exact path that `aud_to_wav` takes: parse an `AudFile`, then
/// hand `compressed_data` to `AudStream::from_payload` for incremental decode.
#[test]
fn from_payload_matches_batch_on_raw_adpcm() {
    let compressed: Vec<u8> = (0..80u8)
        .map(|i| i.wrapping_mul(29).wrapping_add(3))
        .collect();
    let uncompressed_samples = compressed.len() * 2;
    let mut file_bytes = make_header_bytes(
        22050,
        compressed.len() as u32,
        (uncompressed_samples * 2) as u32,
        AUD_FLAG_16BIT,
        SCOMP_WESTWOOD,
    );
    file_bytes.extend_from_slice(&compressed);

    let aud = AudFile::parse(&file_bytes).unwrap();
    let batch = decode_adpcm(aud.compressed_data, false, 0);

    // Mirror the aud_to_wav export path.
    let mut stream = AudStream::from_payload(aud.header.clone(), Cursor::new(aud.compressed_data));
    let mut streamed = Vec::new();
    let mut scratch = [0i16; 19];
    loop {
        let read = stream.read_samples(&mut scratch).unwrap();
        if read == 0 {
            break;
        }
        streamed.extend_from_slice(scratch.get(..read).unwrap_or(&[]));
    }

    assert_eq!(
        streamed, batch,
        "from_payload must match batch decode_adpcm"
    );
}
