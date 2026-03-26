// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Westwood IMA ADPCM encoder and AUD file builder.
//!
//! Split from `mod.rs` to keep that file under the 600-line cap.
//! Provides [`encode_adpcm`], [`encode_adpcm_stateful`], and [`build_aud`].
//!
//! The encoder is the mathematical inverse of the decoder in `mod.rs`:
//! given a PCM sample, find the 4-bit nibble that minimises reconstruction
//! error, then update channel state identically to the decoder so encoder
//! and decoder stay in lockstep.  Clean-room implementation from the
//! published IMA ADPCM standard (1992).

use super::{
    AdpcmChannel, AUD_FLAG_16BIT, AUD_FLAG_STEREO, IMA_INDEX_ADJ, IMA_STEP_TABLE, SCOMP_WESTWOOD,
};

impl AdpcmChannel {
    /// Encodes a single PCM sample into a 4-bit ADPCM nibble.
    ///
    /// Updates internal state (sample, step_index) identically to
    /// `decode_nibble` so encoder and decoder remain synchronized.
    #[inline]
    pub(super) fn encode_nibble(&mut self, sample: i16) -> u8 {
        let step = IMA_STEP_TABLE.get(self.step_index).copied().unwrap_or(7);
        let diff = (sample as i32) - self.sample;

        // Determine the sign bit and magnitude.
        let sign = if diff < 0 { 1u8 } else { 0u8 };
        let abs_diff = diff.unsigned_abs() as i32;

        // Quantise: find the 3-bit magnitude that best represents abs_diff.
        let mut nibble = 0u8;
        let mut threshold = step;
        if abs_diff >= threshold {
            nibble |= 4;
        }
        if abs_diff
            >= threshold
                .wrapping_shr(1)
                .wrapping_add(if nibble & 4 != 0 { step } else { 0 })
        {
            nibble |= 2;
        }
        // Recompute threshold for bit 0 based on bits already set.
        threshold = step >> 3;
        if nibble & 4 != 0 {
            threshold += step;
        }
        if nibble & 2 != 0 {
            threshold += step >> 1;
        }
        if abs_diff >= threshold + (step >> 2) {
            nibble |= 1;
        }

        let token = nibble | (sign << 3);

        // Reconstruct exactly as the decoder does to stay in sync.
        let magnitude = (token & 0x07) as usize;
        let mut recon = step >> 3;
        if magnitude & 0x04 != 0 {
            recon += step;
        }
        if magnitude & 0x02 != 0 {
            recon += step >> 1;
        }
        if magnitude & 0x01 != 0 {
            recon += step >> 2;
        }
        if token & 0x08 != 0 {
            recon = -recon;
        }
        self.sample = (self.sample + recon).clamp(-32768, 32767);
        let adj = IMA_INDEX_ADJ
            .get((token & 0x0F) as usize)
            .copied()
            .unwrap_or(-1);
        self.step_index = ((self.step_index as i32) + adj).clamp(0, 88) as usize;

        token
    }
}

/// Encodes PCM samples into Westwood IMA ADPCM compressed bytes.
///
/// This is the inverse of [`super::decode_adpcm`].  Each pair of output nibbles
/// is packed into one byte (low nibble first, then high nibble).
///
/// For stereo, `samples` must be interleaved `[L, R, L, R, …]` and
/// `sample_count` should match the total number of samples.
pub fn encode_adpcm(samples: &[i16], stereo: bool) -> Vec<u8> {
    let mut left = AdpcmChannel::default();
    let mut right = AdpcmChannel::default();

    if stereo {
        // Stereo: group samples into L/R pairs, each channel produces one
        // byte per pair of nibbles.  Matches the interleave pattern of the
        // decoder: even bytes → left channel, odd bytes → right channel.
        let mut out = Vec::with_capacity(samples.len());
        // Process in groups of 4 samples: 2 left + 2 right nibbles = 2 bytes.
        let mut i = 0;
        while i + 3 < samples.len() {
            // Two left-channel samples → one byte.
            let lo_l = left.encode_nibble(samples.get(i).copied().unwrap_or(0));
            let hi_l = left.encode_nibble(samples.get(i + 2).copied().unwrap_or(0));
            out.push((hi_l << 4) | (lo_l & 0x0F));
            // Two right-channel samples → one byte.
            let lo_r = right.encode_nibble(samples.get(i + 1).copied().unwrap_or(0));
            let hi_r = right.encode_nibble(samples.get(i + 3).copied().unwrap_or(0));
            out.push((hi_r << 4) | (lo_r & 0x0F));
            i += 4;
        }
        out
    } else {
        // Mono: every two samples produce one byte of ADPCM.
        let mut out = Vec::with_capacity(samples.len().div_ceil(2));
        let mut i = 0;
        while i < samples.len() {
            let lo = left.encode_nibble(samples.get(i).copied().unwrap_or(0));
            let hi = if i + 1 < samples.len() {
                left.encode_nibble(samples.get(i + 1).copied().unwrap_or(0))
            } else {
                0
            };
            out.push((hi << 4) | (lo & 0x0F));
            i += 2;
        }
        out
    }
}

/// Encodes PCM samples into Westwood IMA ADPCM compressed bytes, carrying
/// encoder state across calls.
///
/// This is the stateful variant of [`encode_adpcm`].  Pass the same state
/// tuple for every chunk in a stream.  Initialise it to `(0, 0, 0, 0)` for
/// the first chunk.  State layout: `(l_sample, l_index, r_sample, r_index)`.
///
/// Returns the encoded bytes and the updated state.
pub(crate) fn encode_adpcm_stateful(
    samples: &[i16],
    stereo: bool,
    l_sample: &mut i32,
    l_index: &mut usize,
    r_sample: &mut i32,
    r_index: &mut usize,
) -> Vec<u8> {
    let mut left = AdpcmChannel {
        sample: *l_sample,
        step_index: *l_index,
    };
    let mut right = AdpcmChannel {
        sample: *r_sample,
        step_index: *r_index,
    };

    let out = if stereo {
        let mut out = Vec::with_capacity(samples.len());
        let mut i = 0;
        while i + 3 < samples.len() {
            let lo_l = left.encode_nibble(samples.get(i).copied().unwrap_or(0));
            let hi_l = left.encode_nibble(samples.get(i + 2).copied().unwrap_or(0));
            out.push((hi_l << 4) | (lo_l & 0x0F));
            let lo_r = right.encode_nibble(samples.get(i + 1).copied().unwrap_or(0));
            let hi_r = right.encode_nibble(samples.get(i + 3).copied().unwrap_or(0));
            out.push((hi_r << 4) | (lo_r & 0x0F));
            i += 4;
        }
        out
    } else {
        let mut out = Vec::with_capacity(samples.len().div_ceil(2));
        let mut i = 0;
        while i < samples.len() {
            let lo = left.encode_nibble(samples.get(i).copied().unwrap_or(0));
            let hi = if i + 1 < samples.len() {
                left.encode_nibble(samples.get(i + 1).copied().unwrap_or(0))
            } else {
                0
            };
            out.push((hi << 4) | (lo & 0x0F));
            i += 2;
        }
        out
    };

    *l_sample = left.sample;
    *l_index = left.step_index;
    *r_sample = right.sample;
    *r_index = right.step_index;

    out
}

/// Builds a complete AUD file from PCM samples.
///
/// Encodes the given PCM samples as Westwood IMA ADPCM and wraps them in a
/// 12-byte AUD header.  The output is a valid AUD file that [`super::AudFile::parse`]
/// can round-trip.
///
/// # Arguments
///
/// - `samples`: interleaved PCM samples (mono or stereo `[L, R, L, R, …]`).
/// - `sample_rate`: playback rate in Hz (e.g. 22050).
/// - `stereo`: whether the samples are stereo-interleaved.
pub fn build_aud(samples: &[i16], sample_rate: u16, stereo: bool) -> Vec<u8> {
    let compressed = encode_adpcm(samples, stereo);
    // Compressed size includes the 4-byte per-chunk Westwood frame header.
    let compressed_size = (compressed.len() as u32).saturating_add(4);
    // Uncompressed size: 2 bytes per sample (16-bit PCM).
    let uncompressed_size = (samples.len() as u32).saturating_mul(2);
    let flags = if stereo {
        AUD_FLAG_STEREO | AUD_FLAG_16BIT
    } else {
        AUD_FLAG_16BIT
    };

    // AUD header: 12 bytes.
    let mut out = Vec::with_capacity(12 + compressed.len());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&compressed_size.to_le_bytes());
    out.extend_from_slice(&uncompressed_size.to_le_bytes());
    out.push(flags);
    out.push(SCOMP_WESTWOOD);
    // Westwood AUD files use per-chunk framing: one chunk header (4 bytes)
    // followed by the ADPCM data.  For simplicity, wrap all compressed
    // data as a single chunk.
    let chunk_compressed_size = compressed.len() as u16;
    let chunk_uncompressed_size = (samples.len().saturating_mul(2)) as u16;
    out.extend_from_slice(&chunk_compressed_size.to_le_bytes());
    out.extend_from_slice(&chunk_uncompressed_size.to_le_bytes());
    out.extend_from_slice(&compressed);

    out
}
