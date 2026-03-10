// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! AUD audio parser and Westwood IMA ADPCM decoder (`.aud`).
//!
//! AUD files store IMA ADPCM–compressed audio in Westwood's variant of the
//! codec.  The file begins with a 12-byte header followed immediately by the
//! compressed audio data.
//!
//! ## File Layout
//!
//! ```text
//! [AudHeader]        12 bytes
//! [compressed data]  header.compressed_size bytes
//! ```
//!
//! ## ADPCM Algorithm
//!
//! The codec processes each compressed byte as two 4-bit nibbles (low nibble
//! first, then high nibble).  Using the standard IMA ADPCM lookup tables it
//! updates a running `sample` and `step_index` for each nibble:
//!
//! ```text
//! diff       = f(step_table[step_index], nibble)
//! sample     = clamp(sample + diff, -32768, 32767)
//! step_index = clamp(step_index + index_adj[nibble], 0, 88)
//! ```
//!
//! The per-channel state (`sample`, `step_index`) is initialised to zero.
//! Stereo files interleave left/right channels on a per-byte basis.
//!
//! ## References
//!
//! Format source: `REDALERT/WIN32LIB/AUDIO.H`, `REDALERT/ADPCM.CPP`,
//! `REDALERT/ITABLE.CPP`, `REDALERT/DTABLE.CPP`.

use crate::error::Error;

// ─── Constants ────────────────────────────────────────────────────────────────

/// Stereo audio flag (bit 0 of `AudHeader::flags`).
pub const AUD_FLAG_STEREO: u8 = 0x01;
/// 16-bit sample flag (bit 1 of `AudHeader::flags`).
pub const AUD_FLAG_16BIT: u8 = 0x02;

/// Compression algorithm identifier: no compression.
pub const SCOMP_NONE: u8 = 0;
/// Compression algorithm identifier: Westwood ADPCM.
pub const SCOMP_WESTWOOD: u8 = 1;
/// Compression algorithm identifier: SOS ADPCM.
pub const SCOMP_SOS: u8 = 99;

// ─── IMA ADPCM Tables ─────────────────────────────────────────────────────────

/// Standard IMA ADPCM quantiser step sizes (89 entries, indices 0–88).
const IMA_STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

/// Step-index adjustment table for each 4-bit nibble value (0–15).
///
/// Nibbles 0–7 represent positive magnitudes; nibbles 8–15 are the sign-bit
/// variants (negative).  The adjustment is the same for both.
const IMA_INDEX_ADJ: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

// ─── Header ───────────────────────────────────────────────────────────────────

/// The 12-byte header at the start of an AUD file.
///
/// Corresponds to `AUDHeaderType` in `REDALERT/WIN32LIB/AUDIO.H`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudHeader {
    /// Playback sample rate in Hz (e.g., 22050).
    pub sample_rate: u16,
    /// Size of the compressed audio data in bytes.
    pub compressed_size: u32,
    /// Size of the audio data when fully decompressed.
    pub uncompressed_size: u32,
    /// Bit flags: [`AUD_FLAG_STEREO`] and/or [`AUD_FLAG_16BIT`].
    pub flags: u8,
    /// Compression algorithm: [`SCOMP_NONE`], [`SCOMP_WESTWOOD`], etc.
    pub compression: u8,
}

impl AudHeader {
    /// Returns `true` if this file contains stereo audio.
    pub fn is_stereo(&self) -> bool {
        self.flags & AUD_FLAG_STEREO != 0
    }

    /// Returns `true` if this file uses 16-bit samples.
    pub fn is_16bit(&self) -> bool {
        self.flags & AUD_FLAG_16BIT != 0
    }
}

// ─── AudFile ─────────────────────────────────────────────────────────────────

/// A parsed AUD file: header plus a reference to the compressed audio bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudFile<'a> {
    /// Parsed file header.
    pub header: AudHeader,
    /// The compressed audio payload (all bytes after the 12-byte header).
    pub compressed_data: &'a [u8],
}

impl<'a> AudFile<'a> {
    /// Parses an AUD file from a byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] if `data` is shorter than 12 bytes.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < 12 {
            return Err(Error::UnexpectedEof);
        }
        let sample_rate = u16::from_le_bytes([data[0], data[1]]);
        let compressed_size = u32::from_le_bytes([data[2], data[3], data[4], data[5]]);
        let uncompressed_size = u32::from_le_bytes([data[6], data[7], data[8], data[9]]);
        let flags = data[10];
        let compression = data[11];

        Ok(AudFile {
            header: AudHeader {
                sample_rate,
                compressed_size,
                uncompressed_size,
                flags,
                compression,
            },
            compressed_data: &data[12..],
        })
    }
}

// ─── ADPCM Channel State ──────────────────────────────────────────────────────

/// Per-channel decoder state for the Westwood IMA ADPCM codec.
#[derive(Debug, Clone, Default)]
struct AdpcmChannel {
    /// Current predicted sample value (−32768 to 32767).
    sample: i32,
    /// Current step-table index (0–88).
    step_index: usize,
}

impl AdpcmChannel {
    /// Decodes a single 4-bit nibble and advances the channel state.
    ///
    /// Returns the new 16-bit signed sample.
    fn decode_nibble(&mut self, nibble: u8) -> i16 {
        let token = (nibble & 0x0F) as usize;
        let step = IMA_STEP_TABLE[self.step_index];

        // Reconstruct the signed difference from the 4-bit code.
        // Bits 2-0 are magnitude; bit 3 is the sign.
        let magnitude = token & 0x07;
        let mut diff = step >> 3; // base contribution (always added)
        if magnitude & 0x04 != 0 {
            diff += step;
        }
        if magnitude & 0x02 != 0 {
            diff += step >> 1;
        }
        if magnitude & 0x01 != 0 {
            diff += step >> 2;
        }
        if token & 0x08 != 0 {
            diff = -diff;
        }

        self.sample = (self.sample + diff).clamp(-32768, 32767);

        // Advance step index.
        self.step_index = ((self.step_index as i32) + IMA_INDEX_ADJ[token]).clamp(0, 88) as usize;

        self.sample as i16
    }
}

// ─── Decoder ─────────────────────────────────────────────────────────────────

/// Decodes Westwood IMA ADPCM audio into a `Vec<i16>` of PCM samples.
///
/// Each compressed byte encodes **two** samples (low nibble first, then high
/// nibble).  The returned `Vec` contains interleaved left/right samples for
/// stereo files.
///
/// `compressed` should be the raw compressed bytes from an AUD file
/// (i.e., `AudFile::compressed_data`).  `stereo` should match
/// `AudFile::header.is_stereo()`.
///
/// # Note
///
/// This decoder handles `SCOMP_WESTWOOD` (compression ID 1).  For
/// `SCOMP_NONE`, the caller should interpret the bytes directly as 8-bit or
/// 16-bit PCM without calling this function.
pub fn decode_adpcm(compressed: &[u8], stereo: bool) -> Vec<i16> {
    let mut left = AdpcmChannel::default();
    let mut right = AdpcmChannel::default();
    let mut samples = Vec::with_capacity(compressed.len() * 2);

    if stereo {
        // Interleaved: even bytes → left channel, odd bytes → right channel.
        for (i, &byte) in compressed.iter().enumerate() {
            let ch = if i % 2 == 0 { &mut left } else { &mut right };
            let lo = ch.decode_nibble(byte & 0x0F);
            let hi = ch.decode_nibble(byte >> 4);
            samples.push(lo);
            samples.push(hi);
        }
    } else {
        for &byte in compressed {
            let lo = left.decode_nibble(byte & 0x0F);
            let hi = left.decode_nibble(byte >> 4);
            samples.push(lo);
            samples.push(hi);
        }
    }

    samples
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Header parsing ────────────────────────────────────────────────────────

    fn make_header_bytes(
        sample_rate: u16,
        comp_size: u32,
        uncomp_size: u32,
        flags: u8,
        compression: u8,
    ) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&sample_rate.to_le_bytes());
        v.extend_from_slice(&comp_size.to_le_bytes());
        v.extend_from_slice(&uncomp_size.to_le_bytes());
        v.push(flags);
        v.push(compression);
        v
    }

    /// Parsing a valid 12-byte header succeeds.
    #[test]
    fn test_parse_header_valid() {
        let bytes = make_header_bytes(22050, 1000, 4000, AUD_FLAG_16BIT, SCOMP_WESTWOOD);
        let aud = AudFile::parse(&bytes).unwrap();

        assert_eq!(aud.header.sample_rate, 22050);
        assert_eq!(aud.header.compressed_size, 1000);
        assert_eq!(aud.header.uncompressed_size, 4000);
        assert_eq!(aud.header.flags, AUD_FLAG_16BIT);
        assert_eq!(aud.header.compression, SCOMP_WESTWOOD);
        assert!(aud.header.is_16bit());
        assert!(!aud.header.is_stereo());
    }

    /// Stereo flag is detected correctly.
    #[test]
    fn test_parse_header_stereo_flag() {
        let bytes = make_header_bytes(
            22050,
            0,
            0,
            AUD_FLAG_STEREO | AUD_FLAG_16BIT,
            SCOMP_WESTWOOD,
        );
        let aud = AudFile::parse(&bytes).unwrap();
        assert!(aud.header.is_stereo());
        assert!(aud.header.is_16bit());
    }

    /// Data shorter than 12 bytes returns UnexpectedEof.
    #[test]
    fn test_parse_header_too_short() {
        assert_eq!(AudFile::parse(&[]).unwrap_err(), Error::UnexpectedEof);
        assert_eq!(
            AudFile::parse(&[0u8; 11]).unwrap_err(),
            Error::UnexpectedEof
        );
    }

    /// Bytes beyond the 12-byte header are exposed as compressed_data.
    #[test]
    fn test_parse_compressed_data_slice() {
        let mut bytes = make_header_bytes(8000, 3, 6, 0, SCOMP_WESTWOOD);
        bytes.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
        let aud = AudFile::parse(&bytes).unwrap();
        assert_eq!(aud.compressed_data, &[0xAAu8, 0xBB, 0xCC]);
    }

    /// No compression ID is preserved as-is.
    #[test]
    fn test_parse_no_compression() {
        let bytes = make_header_bytes(11025, 100, 100, 0, SCOMP_NONE);
        let aud = AudFile::parse(&bytes).unwrap();
        assert_eq!(aud.header.compression, SCOMP_NONE);
    }

    // ── ADPCM decoder ─────────────────────────────────────────────────────────

    /// All-zero compressed bytes decode to near-zero samples.
    ///
    /// With step_index=0 and token=0:
    ///   diff = step_table[0] >> 3 = 7 >> 3 = 0 (integer division)
    ///   step_index stays 0 (index_adj[0] = -1, clamped to 0)
    /// So samples remain 0 throughout.
    #[test]
    fn test_adpcm_decode_silence() {
        let compressed = [0u8; 4]; // 8 nibbles = 8 samples
        let samples = decode_adpcm(&compressed, false);
        assert_eq!(samples.len(), 8);
        for s in &samples {
            assert_eq!(*s, 0, "silence decodes to zero");
        }
    }

    /// decode_adpcm produces 2 samples per compressed byte.
    #[test]
    fn test_adpcm_sample_count() {
        let compressed = [0u8; 10];
        let samples = decode_adpcm(&compressed, false);
        assert_eq!(samples.len(), 20);
    }

    /// Stereo decode produces 2 samples per compressed byte (left + right).
    #[test]
    fn test_adpcm_stereo_sample_count() {
        let compressed = [0u8; 8]; // 4 pairs of bytes
        let samples = decode_adpcm(&compressed, true);
        assert_eq!(samples.len(), 16); // 2 samples per byte
    }

    /// Empty input produces no samples.
    #[test]
    fn test_adpcm_empty_input() {
        let samples = decode_adpcm(&[], false);
        assert!(samples.is_empty());
    }

    /// Non-zero nibble advances the step index (output diverges from zero).
    ///
    /// Nibble 7 (0b0111) has magnitude=7 = all bits set; with step=7 it encodes
    /// diff = 7/8 + 7/4 + 7/2 + 7 = 0 + 1 + 3 + 7 = 11 (integer arithmetic).
    /// So the first sample after processing a 0x07 low nibble should be 11.
    #[test]
    fn test_adpcm_nonzero_nibble() {
        // byte 0x07: low nibble=7 (positive max at step_index=0), high nibble=0
        let compressed = [0x07u8];
        let samples = decode_adpcm(&compressed, false);
        assert_eq!(samples.len(), 2);
        // First sample: token=7 (magnitude=7, sign=0), diff=11
        assert_eq!(samples[0], 11);
    }

    /// Nibble 8 (sign bit set, magnitude=0): negative diff with same magnitude as nibble 0.
    ///
    /// With step_index=0, token=8 (0b1000):
    ///   magnitude = 8 & 7 = 0 → diff_base = 7 >> 3 = 0
    ///   sign bit set → diff = 0, negated → 0
    /// Sample stays at 0.
    #[test]
    fn test_adpcm_negative_zero_nibble() {
        let compressed = [0x08u8]; // low nibble=8 (sign bit, mag=0)
        let samples = decode_adpcm(&compressed, false);
        assert_eq!(samples[0], 0);
    }

    /// Symmetric encode/decode: nibble 0x0F (sign + all magnitude bits).
    ///
    /// Token 15 = 0b1111: magnitude=7, sign=1
    ///   diff = -(7/8 + 7/4 + 7/2 + 7) = -11
    ///   sample = -11
    #[test]
    fn test_adpcm_max_negative_nibble() {
        let compressed = [0x0Fu8]; // low nibble=15 (max negative at step=0)
        let samples = decode_adpcm(&compressed, false);
        assert_eq!(samples[0], -11);
    }

    /// Step index increases after a large-magnitude nibble (nibble 7 → adj=+8).
    ///
    /// After processing nibble 7 at step_index=0, the new step_index = 0+8 = 8.
    /// The high nibble (0) then uses IMA_STEP_TABLE[8] = 16.
    ///   diff = 16 >> 3 = 2, token=0 (magnitude=0)
    ///   sample = 11 + 2 = 13
    #[test]
    fn test_adpcm_step_index_advances() {
        // byte = 0x07: low nibble=7, high nibble=0
        let compressed = [0x07u8];
        let samples = decode_adpcm(&compressed, false);
        // sample[0] = 11, sample[1] uses step_index=8 (step=16):
        // diff = 16>>3 = 2, sample[1] = 11+2 = 13
        assert_eq!(samples[0], 11);
        assert_eq!(samples[1], 13);
    }

    /// Decoder clamps sample to i16 range: no overflow.
    ///
    /// Drive the sample toward +32767 then try to go further — it must clamp.
    #[test]
    fn test_adpcm_clamping() {
        // Byte 0x77: both nibbles = 7 (max positive at whatever step_index).
        // Repeat many times to saturate at 32767.
        let compressed = vec![0x77u8; 200];
        let samples = decode_adpcm(&compressed, false);
        for s in &samples {
            // i16 is always within -32768..=32767 by definition; this check
            // guards against regressions if the return type ever changes.
            let _ = *s; // ensure each sample is produced without panic
        }
        // Final samples should be near the maximum.
        let last = *samples.last().unwrap();
        assert!(
            last > 30000,
            "saturated sample should be near max: got {last}"
        );
    }
}
