// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

// ─── Test helpers ────────────────────────────────────────────────────────────

/// Writes a big-endian `u32` at the given offset in a buffer.
///
/// VQA/IFF uses big-endian sizes, so this helper mirrors the format's
/// byte order for test construction.
pub(super) fn write_u32_be(buf: &mut [u8], offset: usize, value: u32) {
    buf[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

/// Writes a little-endian `u16` at the given offset.
pub(super) fn write_u16_le(buf: &mut [u8], offset: usize, value: u16) {
    buf[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

/// Builds a minimal valid VQHD chunk payload (42 bytes).
///
/// Sets reasonable defaults for a 320×200 video with 10 frames.
/// `num_frames` is configurable; all other fields use typical RA values.
/// Field layout matches EA VQAFILE.H `VQAHeader` struct.
pub(super) fn build_vqhd(num_frames: u16) -> [u8; 42] {
    let mut hd = [0u8; 42];
    write_u16_le(&mut hd, 0, 2); // version = 2
    write_u16_le(&mut hd, 2, 0); // flags
    write_u16_le(&mut hd, 4, num_frames); // num_frames
    write_u16_le(&mut hd, 6, 320); // width
    write_u16_le(&mut hd, 8, 200); // height
    hd[10] = 4; // block_w
    hd[11] = 2; // block_h
    hd[12] = 15; // fps (frames per second)
    hd[13] = 8; // groupsize (frames per codebook update cycle)
    write_u16_le(&mut hd, 14, 27); // num_1_colors
    write_u16_le(&mut hd, 16, 256); // cb_entries (total codebook entries)
    write_u16_le(&mut hd, 18, 0); // x_pos
    write_u16_le(&mut hd, 20, 0); // y_pos
    write_u16_le(&mut hd, 22, 0x2000); // max_frame_size
    write_u16_le(&mut hd, 24, 22050); // freq
    hd[26] = 1; // channels (mono)
    hd[27] = 16; // bits
    hd
}

/// Builds a minimal valid VQA file containing a VQHD chunk.
///
/// The file has the FORM/WVQA envelope wrapping a single VQHD chunk.
/// Additional chunks can be appended before the FORM size is finalised.
pub(super) fn build_vqa_basic(num_frames: u16) -> Vec<u8> {
    let vqhd = build_vqhd(num_frames);

    // FORM envelope + VQHD chunk:
    // "FORM" (4) + form_size (4) + "WVQA" (4) +
    // "VQHD" (4) + chunk_size (4) + 42 bytes = 62 total
    let form_data_size = 4 + 8 + vqhd.len(); // "WVQA" + VQHD chunk header + payload
    let total = 8 + form_data_size; // "FORM" + form_size + rest
    let mut buf = vec![0u8; total];

    // FORM envelope.
    buf[0..4].copy_from_slice(b"FORM");
    write_u32_be(&mut buf, 4, form_data_size as u32);
    buf[8..12].copy_from_slice(b"WVQA");

    // VQHD chunk.
    buf[12..16].copy_from_slice(b"VQHD");
    write_u32_be(&mut buf, 16, vqhd.len() as u32);
    buf[20..20 + vqhd.len()].copy_from_slice(&vqhd);

    buf
}

/// Builds a VQA file with VQHD + FINF chunks.
///
/// The FINF chunk contains `num_frames` little-endian u32 entries, each
/// set to a dummy offset value.
pub(super) fn build_vqa_with_finf(num_frames: u16) -> Vec<u8> {
    let vqhd = build_vqhd(num_frames);
    let finf_size = (num_frames as usize) * 4;

    // Total: FORM envelope (12) + VQHD chunk (8+42) + FINF chunk (8+finf_size)
    let form_data_size = 4 + (8 + vqhd.len()) + (8 + finf_size);
    let total = 8 + form_data_size;
    let mut buf = vec![0u8; total];

    // FORM envelope.
    buf[0..4].copy_from_slice(b"FORM");
    write_u32_be(&mut buf, 4, form_data_size as u32);
    buf[8..12].copy_from_slice(b"WVQA");

    // VQHD chunk.
    let mut pos = 12;
    buf[pos..pos + 4].copy_from_slice(b"VQHD");
    write_u32_be(&mut buf, pos + 4, vqhd.len() as u32);
    buf[pos + 8..pos + 8 + vqhd.len()].copy_from_slice(&vqhd);
    pos += 8 + vqhd.len();

    // FINF chunk: each entry is a dummy offset (100 * i).
    buf[pos..pos + 4].copy_from_slice(b"FINF");
    write_u32_be(&mut buf, pos + 4, finf_size as u32);
    let data_start = pos + 8;
    for i in 0..num_frames as usize {
        let offset = data_start + i * 4;
        buf[offset..offset + 4].copy_from_slice(&((i as u32) * 100).to_le_bytes());
    }

    buf
}

/// Builds a VQA file with caller-provided chunks after the VQHD header.
///
/// Why: decoder tests need malformed `CBPZ` and `SND1` payloads that still
/// live inside a structurally valid VQA container.
pub(super) fn build_vqa_with_chunks(
    num_frames: u16,
    groupsize: u8,
    audio: Option<(u16, u8, u8)>,
    chunks: &[([u8; 4], Vec<u8>)],
) -> Vec<u8> {
    let mut vqhd = build_vqhd(num_frames);
    vqhd[13] = groupsize;

    if let Some((freq, channels, bits)) = audio {
        write_u16_le(&mut vqhd, 24, freq);
        vqhd[26] = channels;
        vqhd[27] = bits;
    } else {
        write_u16_le(&mut vqhd, 24, 0);
        vqhd[26] = 0;
        vqhd[27] = 0;
    }

    let extra_size: usize = chunks
        .iter()
        .map(|(_, payload)| {
            8usize
                .saturating_add(payload.len())
                .saturating_add(payload.len() & 1)
        })
        .sum();
    let form_data_size = 4usize
        .saturating_add(8usize.saturating_add(vqhd.len()))
        .saturating_add(extra_size);

    let mut out = Vec::with_capacity(8usize.saturating_add(form_data_size));
    out.extend_from_slice(b"FORM");
    out.extend_from_slice(&(form_data_size as u32).to_be_bytes());
    out.extend_from_slice(b"WVQA");

    out.extend_from_slice(b"VQHD");
    out.extend_from_slice(&(vqhd.len() as u32).to_be_bytes());
    out.extend_from_slice(&vqhd);

    for (fourcc, payload) in chunks {
        out.extend_from_slice(fourcc);
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        out.extend_from_slice(payload);
        if payload.len() & 1 != 0 {
            out.push(0);
        }
    }

    out
}

// ─── Basic functionality ─────────────────────────────────────────────────────

/// Parses a well-formed VQA file with a single VQHD chunk.
#[test]
fn parse_basic_vqhd_only() {
    let data = build_vqa_basic(10);
    let vqa = VqaFile::parse(&data).unwrap();
    assert_eq!(vqa.header.version, 2);
    assert_eq!(vqa.header.num_frames, 10);
    assert_eq!(vqa.header.width, 320);
    assert_eq!(vqa.header.height, 200);
    assert_eq!(vqa.header.block_w, 4);
    assert_eq!(vqa.header.block_h, 2);
    assert_eq!(vqa.header.freq, 22050);
    assert_eq!(vqa.header.channels, 1);
    assert_eq!(vqa.header.bits, 16);
    assert_eq!(vqa.chunks.len(), 1);
    assert!(vqa.frame_index.is_none());
}

/// Parses a VQA file with VQHD + FINF chunks.
#[test]
fn parse_with_finf() {
    let data = build_vqa_with_finf(5);
    let vqa = VqaFile::parse(&data).unwrap();
    assert_eq!(vqa.header.num_frames, 5);
    assert_eq!(vqa.chunks.len(), 2);
    let finf = vqa.frame_index.as_ref().unwrap();
    assert_eq!(finf.len(), 5);
    // First offset is 0, second is 100, etc.
    assert_eq!(finf[0], 0);
    assert_eq!(finf[1], 100);
    assert_eq!(finf[4], 400);
}

/// Header has_audio() returns true when freq > 0 and channels > 0.
#[test]
fn header_has_audio() {
    let data = build_vqa_basic(1);
    let vqa = VqaFile::parse(&data).unwrap();
    assert!(vqa.header.has_audio());
    assert!(!vqa.header.is_stereo());
}

/// is_stereo() returns true when channels >= 2.
///
/// Why: this accessor is the only way callers distinguish mono from stereo
/// audio, so it must correctly reflect the VQHD channel field.
#[test]
fn header_is_stereo() {
    let mut data = build_vqa_basic(1);
    // VQHD payload starts at byte 20 in the file.  channels is at offset 26
    // within the VQHD (byte 46 absolute).
    data[46] = 2; // set channels to 2 = stereo
    let vqa = VqaFile::parse(&data).unwrap();
    assert!(vqa.header.is_stereo());
    assert_eq!(vqa.header.channels, 2);
}

/// has_audio() returns false when freq is zero.
///
/// Why: a VQA with freq=0 has no audio stream, even if channels > 0.
#[test]
fn header_no_audio_when_freq_zero() {
    let mut data = build_vqa_basic(1);
    // freq is at VQHD offset 24, which is file byte 44–45.
    data[44] = 0;
    data[45] = 0;
    let vqa = VqaFile::parse(&data).unwrap();
    assert!(!vqa.header.has_audio());
}

/// Chunk FourCC is correctly captured.
#[test]
fn chunk_fourcc_preserved() {
    let data = build_vqa_basic(1);
    let vqa = VqaFile::parse(&data).unwrap();
    assert_eq!(&vqa.chunks[0].fourcc, b"VQHD");
    assert_eq!(vqa.chunks[0].data.len(), 42);
}

// ─── Error paths ─────────────────────────────────────────────────────────────

/// Input shorter than the FORM envelope (12 bytes) returns UnexpectedEof.
#[test]
fn truncated_form_envelope() {
    let data = [0u8; 11];
    let err = VqaFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::UnexpectedEof {
            needed: 12,
            available: 11
        }
    ));
}

/// Missing FORM magic returns InvalidMagic.
#[test]
fn bad_form_magic() {
    let mut data = build_vqa_basic(1);
    data[0..4].copy_from_slice(b"XXXX");
    let err = VqaFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidMagic {
            context: "VQA FORM"
        }
    ));
}

/// Wrong form type (not WVQA) returns InvalidMagic.
#[test]
fn bad_form_type() {
    let mut data = build_vqa_basic(1);
    data[8..12].copy_from_slice(b"XXXX");
    let err = VqaFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidMagic {
            context: "VQA WVQA type"
        }
    ));
}

/// File with no VQHD chunk returns InvalidMagic (missing header).
#[test]
fn missing_vqhd_chunk() {
    // Build a FORM/WVQA envelope with a non-VQHD chunk.
    let payload = [0u8; 10];
    let form_data_size = 4 + 8 + payload.len(); // "WVQA" + chunk header + payload
    let total = 8 + form_data_size;
    let mut buf = vec![0u8; total];
    buf[0..4].copy_from_slice(b"FORM");
    write_u32_be(&mut buf, 4, form_data_size as u32);
    buf[8..12].copy_from_slice(b"WVQA");
    buf[12..16].copy_from_slice(b"SND0");
    write_u32_be(&mut buf, 16, payload.len() as u32);
    let err = VqaFile::parse(&buf).unwrap_err();
    assert!(matches!(err, Error::InvalidMagic { .. }));
}

/// Truncated VQHD payload returns UnexpectedEof.
#[test]
fn truncated_vqhd() {
    // Build a file claiming VQHD size 42 but only providing 20 bytes.
    let form_data_size = 4 + 8 + 20;
    let total = 8 + form_data_size;
    let mut buf = vec![0u8; total];
    buf[0..4].copy_from_slice(b"FORM");
    write_u32_be(&mut buf, 4, form_data_size as u32);
    buf[8..12].copy_from_slice(b"WVQA");
    buf[12..16].copy_from_slice(b"VQHD");
    write_u32_be(&mut buf, 16, 20); // claims only 20 bytes
    let err = VqaFile::parse(&buf).unwrap_err();
    assert!(matches!(
        err,
        Error::UnexpectedEof {
            needed: 42,
            available: 20
        }
    ));
}

// ─── Determinism ─────────────────────────────────────────────────────────────

/// Parsing the same input twice yields identical results.
#[test]
fn deterministic() {
    let data = build_vqa_with_finf(8);
    let a = VqaFile::parse(&data).unwrap();
    let b = VqaFile::parse(&data).unwrap();
    assert_eq!(a, b);
}

// ─── Decoder strictness ─────────────────────────────────────────────────────

/// `decode_frames()` handles malformed `CBPZ` data without panic.
///
/// Why: LCW decompression is tolerant of out-of-range offsets (matching
/// the original EA engine's pre-zeroed buffer behaviour).  Garbage CBPZ
/// data may produce wrong pixels but must not panic or crash.
#[test]
fn decode_frames_handles_invalid_cbpz_without_panic() {
    let data = build_vqa_with_chunks(1, 1, None, &[(*b"CBPZ", vec![0x00, 0x00])]);
    let vqa = VqaFile::parse(&data).unwrap();
    // May succeed (producing garbage) or error — either is acceptable.
    // The key invariant is no panic.
    let _ = vqa.decode_frames();
}

/// `extract_audio()` rejects a truncated uncompressed `SND1` payload.
///
/// Why: SND1 must return an error when the declared payload is shorter than
/// the raw 8-bit sample count instead of returning partial audio.
#[test]
fn extract_audio_rejects_truncated_snd1_uncompressed() {
    let snd1 = vec![
        4, 0, // out_size = 4
        4, 0, // size = 4 (uncompressed)
        0x80, 0x81, // only 2 payload bytes instead of 4
    ];
    let data = build_vqa_with_chunks(1, 1, Some((22050, 1, 8)), &[(*b"SND1", snd1)]);
    let vqa = VqaFile::parse(&data).unwrap();
    let err = vqa.extract_audio().unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// `extract_audio()` rejects a compressed `SND1` stream that ends mid-copy.
///
/// Why: truncated ADPCM commands must not decode to a shorter sample buffer
/// without reporting that the compressed payload ran out.
#[test]
fn extract_audio_rejects_truncated_snd1_compressed() {
    let snd1 = vec![
        2, 0, // out_size = 2
        1, 0,    // size = 1 (compressed payload)
        0x80, // code 2 copy command; needs one more byte that is missing
    ];
    let data = build_vqa_with_chunks(1, 1, Some((22050, 1, 8)), &[(*b"SND1", snd1)]);
    let vqa = VqaFile::parse(&data).unwrap();
    let err = vqa.extract_audio().unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

// ─── Boundary tests ─────────────────────────────────────────────────────────

/// Zero-frame VQA is accepted (valid but degenerate).
#[test]
fn zero_frames() {
    let data = build_vqa_basic(0);
    let vqa = VqaFile::parse(&data).unwrap();
    assert_eq!(vqa.header.num_frames, 0);
}

/// V38: chunk size exceeding 256 MB cap is rejected.
#[test]
fn chunk_size_over_cap() {
    let mut data = build_vqa_basic(1);
    // Overwrite the VQHD chunk size to exceed MAX_CHUNK_SIZE.
    write_u32_be(&mut data, 16, 256 * 1024 * 1024 + 1);
    let err = VqaFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::InvalidSize { .. }));
}

// ─── Display messages ────────────────────────────────────────────────────────

/// Error Display output includes numeric context.
#[test]
fn error_display_includes_values() {
    let data = [0u8; 8];
    let err = VqaFile::parse(&data).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("12"));
    assert!(msg.contains("8"));
}

// ─── Integer overflow safety ─────────────────────────────────────────────────

/// FORM size near usize::MAX doesn't panic — saturating_add clamps it.
#[test]
fn form_size_overflow_no_panic() {
    let mut data = vec![0u8; 62]; // minimal valid VQA
    data[0..4].copy_from_slice(b"FORM");
    write_u32_be(&mut data, 4, u32::MAX); // absurdly large form_size
    data[8..12].copy_from_slice(b"WVQA");
    // Place a VQHD chunk.
    data[12..16].copy_from_slice(b"VQHD");
    write_u32_be(&mut data, 16, 42);
    // The file is only 62 bytes, so form_end is clamped to data.len().
    // The VQHD payload extends past the buffer → parse should succeed
    // or return an appropriate error, but must not panic.
    let _ = VqaFile::parse(&data);
}

// ─── Security adversarial tests ──────────────────────────────────────────────

/// All-0xFF input must not panic — exercises worst-case field values.
///
/// Why: an all-max-byte payload triggers maximum values in every parsed
/// field.  The parser must handle this gracefully (error or clamp) without
/// panicking on overflow or out-of-bounds access.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = [0xFF; 128];
    let _ = VqaFile::parse(&data);
}

/// All-zero input (past the FORM envelope) must not panic.
///
/// Why: zero-valued FourCCs, sizes, and frame counts are valid degenerates.
/// The parser must not divide by zero or index out of bounds.
#[test]
fn adversarial_all_zero_no_panic() {
    let mut data = vec![0u8; 128];
    data[0..4].copy_from_slice(b"FORM");
    data[8..12].copy_from_slice(b"WVQA");
    let _ = VqaFile::parse(&data);
}

/// Chunk claiming max u32 size but file is small — must not allocate GBs.
///
/// Why: a crafted file could declare a huge chunk size to trigger
/// out-of-memory.  V38 caps prevent this.
#[test]
fn adversarial_huge_chunk_size_no_oom() {
    let mut data = build_vqa_basic(1);
    // Re-write the VQHD chunk size to just under the 256 MB cap.
    // This claims 200 MB of data but the file is tiny — must be rejected
    // or safely handled without attempting to allocate.
    write_u32_be(&mut data, 16, 200 * 1024 * 1024);
    let _ = VqaFile::parse(&data);
}

/// FINF chunk with frame count mismatch — FINF has fewer entries than VQHD
/// claims.  Must not panic on short payload.
#[test]
fn adversarial_finf_count_mismatch() {
    let mut data = build_vqa_with_finf(10);
    // Shrink the FINF chunk size so it only has room for 2 entries (8 bytes)
    // but VQHD says 10 frames.
    let finf_chunk_offset = 62; // after 12 FORM + 50 (VQHD 8+42) = 62
    write_u32_be(&mut data, finf_chunk_offset + 4, 8); // only 8 bytes
    let _ = VqaFile::parse(&data);
}

/// A chunk whose declared size extends past the FORM boundary must be
/// rejected, not silently truncated.
///
/// Why: silent truncation would allow structurally malformed containers
/// to pass the parser, violating the strict structural validation rule.
#[test]
fn chunk_past_form_boundary_rejected() {
    // Build a valid VQA, then inflate the VQHD chunk size so it claims
    // to extend past the FORM data boundary.
    let mut data = build_vqa_basic(2);
    let form_data_size = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;

    // Set VQHD chunk size to 1 byte more than fits inside the FORM.
    // VQHD payload starts at byte 20; FORM data ends at 8 + form_data_size.
    let max_payload = (8 + form_data_size) - 20;
    write_u32_be(&mut data, 16, (max_payload + 1) as u32);

    let result = VqaFile::parse(&data);
    assert!(result.is_err(), "chunk past FORM boundary must be rejected");
}
