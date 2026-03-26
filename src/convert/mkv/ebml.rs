// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! EBML encoding primitives and Matroska element ID constants.
//!
//! Split from `mod.rs` to keep that file under the 600-line cap.
//! Provides VINT/element-ID writers, typed-value writers, and
//! SeekHead/Cues/Void builders used by `encode_mkv`.

// ─── EBML Element IDs (Matroska spec) ───────────────────────────────────────

pub(super) const EBML_ID: u32 = 0x1A45_DFA3;
pub(super) const EBML_VERSION: u32 = 0x4286;
pub(super) const EBML_READ_VERSION: u32 = 0x42F7;
pub(super) const EBML_MAX_ID_LENGTH: u32 = 0x42F2;
pub(super) const EBML_MAX_SIZE_LENGTH: u32 = 0x42F3;
pub(super) const DOC_TYPE: u32 = 0x4282;
pub(super) const DOC_TYPE_VERSION: u32 = 0x4287;
pub(super) const DOC_TYPE_READ_VERSION: u32 = 0x4285;

pub(super) const SEGMENT: u32 = 0x1853_8067;
pub(super) const INFO: u32 = 0x1549_A966;
pub(super) const TIMESTAMP_SCALE: u32 = 0x2A_D7B1;
pub(super) const DURATION_ID: u32 = 0x4489;
pub(super) const MUXING_APP: u32 = 0x4D80;
pub(super) const WRITING_APP: u32 = 0x5741;

pub(super) const TRACKS: u32 = 0x1654_AE6B;
pub(super) const TRACK_ENTRY: u32 = 0xAE;
pub(super) const TRACK_NUMBER: u32 = 0xD7;
pub(super) const TRACK_UID: u32 = 0x73C5;
pub(super) const TRACK_TYPE: u32 = 0x83;
pub(super) const FLAG_LACING: u32 = 0x9C;
pub(super) const DEFAULT_DURATION: u32 = 0x23_E383;
pub(super) const CODEC_ID: u32 = 0x86;
pub(super) const CODEC_PRIVATE: u32 = 0x63A2;

pub(super) const VIDEO_ID: u32 = 0xE0;
pub(super) const PIXEL_WIDTH: u32 = 0xB0;
pub(super) const PIXEL_HEIGHT: u32 = 0xBA;
pub(super) const UNCOMPRESSED_FOURCC: u32 = 0x2E_B524;
pub(super) const COLOUR: u32 = 0x55B0;
pub(super) const BITS_PER_CHANNEL: u32 = 0x55B2;

pub(super) const AUDIO_ID: u32 = 0xE1;
pub(super) const SAMPLING_FREQUENCY: u32 = 0xB5;
pub(super) const CHANNELS: u32 = 0x9F;
pub(super) const BIT_DEPTH: u32 = 0x6264;

pub(super) const CLUSTER: u32 = 0x1F43_B675;
pub(super) const TIMESTAMP_ID: u32 = 0xE7;
pub(super) const SIMPLE_BLOCK: u32 = 0xA3;

pub(super) const SEEK_HEAD: u32 = 0x114D_9B74;
pub(super) const SEEK: u32 = 0x4DBB;
pub(super) const SEEK_ID_EL: u32 = 0x53AB;
pub(super) const SEEK_POSITION: u32 = 0x53AC;

pub(super) const CUES: u32 = 0x1C53_BB6B;
pub(super) const CUE_POINT: u32 = 0xBB;
pub(super) const CUE_TIME: u32 = 0xB3;
pub(super) const CUE_TRACK_POSITIONS: u32 = 0xB7;
pub(super) const CUE_TRACK: u32 = 0xF7;
pub(super) const CUE_CLUSTER_POSITION: u32 = 0xF1;

/// Total bytes reserved at the start of the Segment for the SeekHead element
/// plus a padding Void element.  160 bytes is sufficient for 3 Seek entries
/// (Info, Tracks, Cues) with positions up to 4 GB.
pub(super) const SEEKHEAD_RESERVE: usize = 160;

// ─── EBML Encoding Primitives ───────────────────────────────────────────────

/// Writes a variable-length EBML element ID.
///
/// IDs are big-endian with the VINT marker bit baked into the most significant
/// byte:
/// - `1xxxxxxx` → 1 byte
/// - `01xxxxxx …` → 2 bytes
/// - `001xxxxx …` → 3 bytes
/// - `0001xxxx …` → 4 bytes
#[inline]
pub(super) fn write_element_id(out: &mut Vec<u8>, id: u32) {
    let bytes = id.to_be_bytes();
    if id >= 0x1000_0000 {
        out.extend_from_slice(&bytes);
    } else if id >= 0x0020_0000 {
        out.extend_from_slice(bytes.get(1..).unwrap_or(&[]));
    } else if id >= 0x0000_4000 {
        out.extend_from_slice(bytes.get(2..).unwrap_or(&[]));
    } else {
        out.push(bytes.get(3).copied().unwrap_or(0));
    }
}

/// Writes a VINT-encoded size value (1–8 bytes).
///
/// The leading byte contains a VINT marker bit that indicates the total byte
/// width.  The all-bits-1 pattern at each width is reserved for "unknown size"
/// and is never used for a concrete value.
#[inline]
pub(super) fn write_vint_size(out: &mut Vec<u8>, size: usize) {
    if size <= 126 {
        out.push(0x80 | size as u8);
    } else if size <= 16382 {
        out.push(0x40 | ((size >> 8) as u8));
        out.push(size as u8);
    } else if size <= 2_097_150 {
        out.push(0x20 | ((size >> 16) as u8));
        out.push((size >> 8) as u8);
        out.push(size as u8);
    } else if size <= 268_435_454 {
        out.push(0x10 | ((size >> 24) as u8));
        out.push((size >> 16) as u8);
        out.push((size >> 8) as u8);
        out.push(size as u8);
    } else {
        // 8-byte VINT for very large sizes (>256 MB).
        let s = size as u64;
        out.push(0x01);
        out.extend_from_slice(s.to_be_bytes().get(1..).unwrap_or(&[]));
    }
}

/// Writes an 8-byte "unknown size" placeholder (`0x01FF_FFFF_FFFF_FFFF`).
#[inline]
pub(super) fn write_unknown_size_placeholder(out: &mut Vec<u8>) {
    out.extend_from_slice(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
}

/// Overwrites an 8-byte VINT placeholder at `pos` with the actual size.
///
/// Builds the 8-byte VINT (0x01 marker + 7 big-endian size bytes) as an array,
/// then writes it in one `copy_from_slice` — no direct indexing.
pub(super) fn patch_8byte_vint(buf: &mut [u8], pos: usize, size: usize) {
    if let Some(dst) = buf.get_mut(pos..pos.saturating_add(8)) {
        let be = (size as u64).to_be_bytes();
        // VINT marker 0x01 followed by the lower 7 bytes of the big-endian size.
        let vint = [
            0x01,
            be.get(1).copied().unwrap_or(0),
            be.get(2).copied().unwrap_or(0),
            be.get(3).copied().unwrap_or(0),
            be.get(4).copied().unwrap_or(0),
            be.get(5).copied().unwrap_or(0),
            be.get(6).copied().unwrap_or(0),
            be.get(7).copied().unwrap_or(0),
        ];
        dst.copy_from_slice(&vint);
    }
}

/// Writes a UINT element (variable-length big-endian, minimal encoding).
pub(super) fn write_uint_element(out: &mut Vec<u8>, id: u32, value: u64) {
    write_element_id(out, id);
    let bytes = value.to_be_bytes();
    let first_nonzero = bytes.iter().position(|&b| b != 0).unwrap_or(7);
    let len = (8usize.saturating_sub(first_nonzero)).max(1);
    write_vint_size(out, len);
    out.extend_from_slice(bytes.get(8usize.saturating_sub(len)..).unwrap_or(&[]));
}

/// Writes a FLOAT element (always 8 bytes, big-endian IEEE 754).
pub(super) fn write_float_element(out: &mut Vec<u8>, id: u32, value: f64) {
    write_element_id(out, id);
    write_vint_size(out, 8);
    out.extend_from_slice(&value.to_be_bytes());
}

/// Writes a UTF-8 STRING element.
pub(super) fn write_string_element(out: &mut Vec<u8>, id: u32, value: &str) {
    write_element_id(out, id);
    write_vint_size(out, value.len());
    out.extend_from_slice(value.as_bytes());
}

/// Writes a BINARY element.
pub(super) fn write_binary_element(out: &mut Vec<u8>, id: u32, value: &[u8]) {
    write_element_id(out, id);
    write_vint_size(out, value.len());
    out.extend_from_slice(value);
}

/// Writes a MASTER element whose children are already serialized.
pub(super) fn write_master_element(out: &mut Vec<u8>, id: u32, children: &[u8]) {
    write_element_id(out, id);
    write_vint_size(out, children.len());
    out.extend_from_slice(children);
}

// ─── SeekHead / Cues Builders ───────────────────────────────────────────────

/// Returns the raw EBML ID bytes for a given element ID (big-endian, with
/// the VINT marker bit already embedded).
pub(super) fn element_id_bytes(id: u32) -> Vec<u8> {
    let bytes = id.to_be_bytes();
    if id >= 0x1000_0000 {
        bytes.to_vec()
    } else if id >= 0x0020_0000 {
        bytes.get(1..).unwrap_or(&[]).to_vec()
    } else if id >= 0x0000_4000 {
        bytes.get(2..).unwrap_or(&[]).to_vec()
    } else {
        vec![bytes.get(3).copied().unwrap_or(0)]
    }
}

/// Builds a complete SeekHead master element pointing to the given
/// (element_id, segment_offset) pairs.
pub(super) fn build_seekhead(entries: &[(u32, usize)]) -> Vec<u8> {
    let mut children = Vec::new();
    for &(id, pos) in entries {
        let mut seek_children = Vec::new();
        write_binary_element(&mut seek_children, SEEK_ID_EL, &element_id_bytes(id));
        write_uint_element(&mut seek_children, SEEK_POSITION, pos as u64);
        write_master_element(&mut children, SEEK, &seek_children);
    }
    let mut buf = Vec::new();
    write_master_element(&mut buf, SEEK_HEAD, &children);
    buf
}

/// Builds the inner content of a Cues element from cluster entries.
///
/// Each entry is `(timestamp_ms, segment_offset)` for one Cluster.
pub(super) fn build_cues(entries: &[(u64, usize)]) -> Vec<u8> {
    let mut children = Vec::new();
    for &(ts, offset) in entries {
        let mut ctp = Vec::new();
        write_uint_element(&mut ctp, CUE_TRACK, 1);
        write_uint_element(&mut ctp, CUE_CLUSTER_POSITION, offset as u64);

        let mut cp = Vec::new();
        write_uint_element(&mut cp, CUE_TIME, ts);
        write_master_element(&mut cp, CUE_TRACK_POSITIONS, &ctp);

        write_master_element(&mut children, CUE_POINT, &cp);
    }
    children
}

/// Builds a Void element of exactly `total_bytes`.
///
/// Used to reserve space for the SeekHead and to pad any leftover bytes
/// after the SeekHead is patched in.
pub(super) fn build_void(total_bytes: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(total_bytes);
    if total_bytes < 2 {
        buf.resize(total_bytes, 0);
        return buf;
    }
    buf.push(0xEC); // Void element ID
    let data_len = total_bytes - 2;
    if data_len <= 126 {
        // 1-byte VINT: total = 1 (ID) + 1 (size) + data_len
        write_vint_size(&mut buf, data_len);
    } else {
        // Force 2-byte VINT to avoid the 126/127 boundary mismatch.
        // total = 1 (ID) + 2 (size) + (total_bytes - 3)
        let data_len = total_bytes - 3;
        buf.push(0x40 | ((data_len >> 8) as u8));
        buf.push(data_len as u8);
    }
    buf.resize(total_bytes, 0);
    buf
}
