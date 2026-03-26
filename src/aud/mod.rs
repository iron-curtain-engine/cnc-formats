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
//! Implemented from the IMA ADPCM standard (1992) and binary analysis of
//! game files.  The original game's corresponding types are documented in
//! `AUDIO.H`, `ADPCM.CPP`, `ITABLE.CPP`, and `DTABLE.CPP`.

use crate::error::Error;
use crate::read::{read_u16_le, read_u32_le, read_u8};
use std::time::Duration;

mod encode;
mod info;
mod stream;
pub(crate) use encode::encode_adpcm_stateful;
pub use encode::{build_aud, encode_adpcm};
pub use info::{AudMediaInfo, AudSeekSupport};
pub use stream::{AudPcmChunk, AudStream};

// ─── Constants ────────────────────────────────────────────────────────────────
//
// Flag and compression-ID constants are `pub` so callers can construct or
// inspect headers without hard-coding magic numbers.  The parser itself is
// permissive on these values (it stores unknown IDs as-is).

/// Stereo audio flag (bit 0 of `AudHeader::flags`).
pub const AUD_FLAG_STEREO: u8 = 0x01;
/// 16-bit sample flag (bit 1 of `AudHeader::flags`).
pub const AUD_FLAG_16BIT: u8 = 0x02;

/// Compression algorithm identifier: no compression.
pub const SCOMP_NONE: u8 = 0;
/// Compression algorithm identifier: Westwood ADPCM.
pub const SCOMP_WESTWOOD: u8 = 1;
/// Compression algorithm identifier: Sonarc compression.
pub const SCOMP_SONARC: u8 = 33;
/// Compression algorithm identifier: SOS ADPCM.
pub const SCOMP_SOS: u8 = 99;

/// Fixed size of the AUD file header in bytes.
pub(crate) const AUD_HEADER_SIZE: usize = 12;

// ─── IMA ADPCM Tables ─────────────────────────────────────────────────────────
//
// Standard IMA ADPCM lookup tables.  These are identical across all IMA
// implementations (not Westwood-specific).  The step table maps a step_index
// (0–88) to a quantiser step size; the index adjustment table maps a 4-bit
// nibble to a step_index delta.
//
// Equivalence with Westwood's pre-multiplied tables:
// `binary-codecs.md` describes 1424-entry `IndexTable` / `DiffTable` arrays
// indexed by `[step_index * 16 + token]`.  These are an optimisation that
// pre-computes the same arithmetic this decoder performs per-nibble:
//   DiffTable[step_index * 16 + token] ≡ f(IMA_STEP_TABLE[step_index], token)
//   IndexTable[step_index * 16 + token] ≡ clamp(step_index + IMA_INDEX_ADJ[token], 0, 88)
// Both representations produce bit-identical decoded audio — the standard IMA
// formulation is used here because it is smaller and easier to audit.
//
// Source: IMA Digital Audio Focus and Technical Standards Subcommittee,
//         Recommended Practices for Enhancing Digital Audio Compatibility
//         in Multimedia Systems, revision 3.00, 1992.

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
/// Nibbles 0–7 represent positive deltas; nibbles 8–15 represent negative
/// deltas (bit 3 is the sign).  The adjustment to `step_index` is
/// **symmetric**: nibbles 0 and 8 both adjust by −1, nibbles 7 and 15
/// both adjust by +8.  This symmetry is an IMA design feature, not a
/// Westwood invention.
const IMA_INDEX_ADJ: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

// ─── Header ───────────────────────────────────────────────────────────────────
//
// The 12-byte AUD header is read as raw little-endian fields.  The parser
// does *not* reject unknown flag combinations or compression IDs — it stores
// them as-is and lets callers decide what they support.  This design makes
// the parser forward-compatible with modded or future game files.

/// The 12-byte header at the start of an AUD file.
///
/// Layout matches the original game's `AUDHeaderType` (12 bytes, LE fields).
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
    #[inline]
    pub fn is_stereo(&self) -> bool {
        self.flags & AUD_FLAG_STEREO != 0
    }

    /// Returns `true` if this file uses 16-bit samples.
    #[inline]
    pub fn is_16bit(&self) -> bool {
        self.flags & AUD_FLAG_16BIT != 0
    }

    /// Returns the number of audio channels implied by the flags.
    #[inline]
    pub fn channel_count(&self) -> u8 {
        if self.is_stereo() {
            2
        } else {
            1
        }
    }

    /// Returns the decoded sample-frame count implied by `uncompressed_size`.
    ///
    /// This counts interleaved stereo pairs as one sample frame.
    #[inline]
    pub fn sample_frames(&self) -> usize {
        let bytes_per_sample = if self.is_16bit() { 2usize } else { 1usize };
        let bytes_per_frame = bytes_per_sample.saturating_mul(self.channel_count() as usize);
        (self.uncompressed_size as usize) / bytes_per_frame.max(1)
    }

    /// Returns the nominal playback duration implied by the header.
    #[inline]
    pub fn duration(&self) -> Option<Duration> {
        if self.sample_rate == 0 {
            return None;
        }

        let frames = self.sample_frames() as u64;
        let secs = frames / u64::from(self.sample_rate);
        let nanos = ((frames % u64::from(self.sample_rate)) * 1_000_000_000u64)
            / u64::from(self.sample_rate);
        Some(Duration::new(secs, nanos as u32))
    }

    /// Returns the playback timestamp of `sample_frame`.
    ///
    /// The timestamp is relative to the start of the stream and counts
    /// interleaved stereo pairs as one sample frame.
    #[inline]
    pub fn sample_frame_timestamp(&self, sample_frame: u64) -> Option<Duration> {
        if self.sample_rate == 0 {
            return None;
        }

        let secs = sample_frame / u64::from(self.sample_rate);
        let nanos = ((sample_frame % u64::from(self.sample_rate)) * 1_000_000_000u64)
            / u64::from(self.sample_rate);
        Some(Duration::new(secs, nanos as u32))
    }
}

/// Parses the fixed-size AUD header fields from raw bytes.
pub(crate) fn parse_header_bytes(data: &[u8]) -> Result<AudHeader, Error> {
    if data.len() < AUD_HEADER_SIZE {
        return Err(Error::UnexpectedEof {
            needed: AUD_HEADER_SIZE,
            available: data.len(),
        });
    }

    Ok(AudHeader {
        sample_rate: read_u16_le(data, 0)?,
        compressed_size: read_u32_le(data, 2)?,
        uncompressed_size: read_u32_le(data, 6)?,
        flags: read_u8(data, 10)?,
        compression: read_u8(data, 11)?,
    })
}

// ─── AudFile ─────────────────────────────────────────────────────────────────

/// A parsed AUD file: header plus a reference to the compressed audio bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudFile<'input> {
    /// Parsed file header.
    pub header: AudHeader,
    /// The compressed audio payload (all bytes after the 12-byte header).
    pub compressed_data: &'input [u8],
}

impl<'input> AudFile<'input> {
    /// Parses an AUD file from a byte slice.
    ///
    /// The parser reads the 12-byte header, then slices out the compressed
    /// payload using `compressed_size`.  It does **not** decompress the audio;
    /// call [`decode_adpcm`] separately.
    ///
    /// ## Permissive Design
    ///
    /// The parser accepts any `flags` and `compression` values.  Callers
    /// decide whether they can handle a particular compression scheme.
    ///
    /// # Errors
    ///
    /// - [`Error::UnexpectedEof`] — `data` is shorter than 12 bytes or shorter
    ///   than the header's declared compressed size.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        let header = parse_header_bytes(data)?;
        let compressed_size = header.compressed_size as usize;

        // Validate that the file actually contains the declared compressed data.
        // V38: use saturating_add so `12 + compressed_size` cannot wrap to a
        // small number on 32-bit platforms, which would bypass the length check.
        let payload = data.get(AUD_HEADER_SIZE..).ok_or(Error::UnexpectedEof {
            needed: AUD_HEADER_SIZE,
            available: data.len(),
        })?;
        if compressed_size > payload.len() {
            return Err(Error::UnexpectedEof {
                needed: AUD_HEADER_SIZE.saturating_add(compressed_size),
                available: data.len(),
            });
        }

        Ok(AudFile {
            header,
            compressed_data: payload.get(..compressed_size).ok_or(Error::UnexpectedEof {
                needed: AUD_HEADER_SIZE.saturating_add(compressed_size),
                available: data.len(),
            })?,
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
    ///
    /// ## Algorithm
    ///
    /// 1. Look up `step` from `IMA_STEP_TABLE[step_index]`.
    /// 2. Reconstruct `diff` from the nibble’s magnitude bits (2–0) and sign
    ///    bit (3), using the step as the quantiser.
    /// 3. Accumulate `sample += diff`, clamped to `i16` range.
    /// 4. Advance `step_index` by `IMA_INDEX_ADJ[nibble]`, clamped to 0–88.
    #[inline]
    fn decode_nibble(&mut self, nibble: u8) -> i16 {
        let token = (nibble & 0x0F) as usize;
        // Safe via .get(): step_index is clamped to 0–88 (table has 89 entries).
        let step = IMA_STEP_TABLE.get(self.step_index).copied().unwrap_or(7);

        // Reconstruct the signed difference from the 4-bit code.
        // The quantiser step is divided into 8 levels by the 3 magnitude bits.
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
        // Negate if sign bit (bit 3) is set.
        if token & 0x08 != 0 {
            diff = -diff;
        }

        // Accumulate and clamp to i16 range (−32768..32767).
        self.sample = (self.sample + diff).clamp(-32768, 32767);

        // Advance step index, clamped to valid table range.
        // Safe via .get(): token is nibble & 0x0F (0–15), table has 16 entries.
        let adj = IMA_INDEX_ADJ.get(token).copied().unwrap_or(-1);
        self.step_index = ((self.step_index as i32) + adj).clamp(0, 88) as usize;

        self.sample as i16
    }
}

// ─── Decoder ─────────────────────────────────────────────────────────────────

/// V38 safety cap: maximum ADPCM output samples when no explicit limit is
/// given.  16 MB of `i16` samples = 8 M samples.
///
/// Why: without a cap, a crafted header could claim billions of samples.
/// Real C&C audio files are well under 1 M samples.
const MAX_ADPCM_SAMPLES: usize = 8 * 1024 * 1024;

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
/// `max_samples` caps the number of output samples (V38 iteration guard).
/// Pass `0` to use the default cap (`MAX_ADPCM_SAMPLES`).
///
/// # Note
///
/// This decoder handles `SCOMP_WESTWOOD` (compression ID 1).  For
/// `SCOMP_NONE`, the caller should interpret the bytes directly as 8-bit or
/// 16-bit PCM without calling this function.
pub fn decode_adpcm(compressed: &[u8], stereo: bool, max_samples: usize) -> Vec<i16> {
    let limit = if max_samples == 0 {
        MAX_ADPCM_SAMPLES
    } else {
        max_samples
    };

    let mut left = AdpcmChannel::default();
    let mut right = AdpcmChannel::default();
    let mut samples = Vec::with_capacity(compressed.len().saturating_mul(2).min(limit));

    if stereo {
        // Stereo interleave: even-indexed bytes → left, odd → right.
        // Each byte produces two samples on the same channel.
        // The per-byte channel assignment matches Westwood’s original codec.
        for (i, &byte) in compressed.iter().enumerate() {
            if samples.len() >= limit {
                break;
            }
            let ch = if i % 2 == 0 { &mut left } else { &mut right };
            // Low nibble first, then high nibble (IMA convention).
            let lo = ch.decode_nibble(byte & 0x0F);
            samples.push(lo);
            if samples.len() >= limit {
                break;
            }
            let hi = ch.decode_nibble(byte >> 4);
            samples.push(hi);
        }
    } else {
        for &byte in compressed {
            if samples.len() >= limit {
                break;
            }
            let lo = left.decode_nibble(byte & 0x0F);
            samples.push(lo);
            if samples.len() >= limit {
                break;
            }
            let hi = left.decode_nibble(byte >> 4);
            samples.push(hi);
        }
    }

    samples
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_validation;

#[cfg(test)]
mod tests_stream;
