// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! VQA frame decoder and audio extraction.
//!
//! Decodes VQA video frames (version 2, 8-bit palette-indexed) and extracts
//! audio streams from parsed VQA containers.  Only version 2 VQA files
//! (C&C: Tiberian Dawn, Red Alert) are fully supported; version 3 HiColor
//! VQAs are not decoded.
//!
//! ## Video Decoding
//!
//! Each frame is reconstructed from:
//!
//! 1. **Codebook (CBF/CBP)** — a table of `block_w × block_h` pixel blocks.
//!    CBF0/CBFZ provide the full table; CBP0/CBPZ provide partial updates
//!    accumulated over `groupsize` frames before replacing the active table.
//! 2. **Vector Pointer Table (VPT)** — indices into the codebook for each
//!    screen block.  The VPT is split into lo/hi halves.  If `hi == 0x0F`
//!    (or `0xFF` for hi-res), the block is a solid fill of color `lo`;
//!    otherwise block index is `hi * 256 + lo`.
//! 3. **Palette (CPL)** — 256-entry RGB palette (6-bit VGA values, 0–63).
//!
//! ## Audio Extraction
//!
//! SND0 = raw PCM, SND1 = Westwood ADPCM (8-bit), SND2 = IMA ADPCM (16-bit).
//! Audio chunks are collected in file order and decoded into PCM samples.
//!
//! ## References
//!
//! Gordan Ugarkovic, "VQA_INFO.TXT" (2004); Valery V. Anisimovsky;
//! community C&C Modding Wiki; MultimediaWiki VQA page.  Clean-room
//! implementation from publicly documented format specifications.

use crate::error::Error;
use crate::lcw;

use super::render::{build_compact_codebook, render_frame_pixels, VqaRenderGeometry};
use super::snd_decode::{append_snd0, append_snd1_stateful, append_snd2_stateful};
use super::{VqaFile, VqaHeader};
use std::borrow::Cow;

// ─── Constants ───────────────────────────────────────────────────────────────

/// V38: maximum codebook size in bytes (16 MB).  A 320×200 video with 4×2
/// blocks has at most ~3840 entries × 8 bytes = ~30 KB; 16 MB is generous.
const MAX_CODEBOOK_SIZE: usize = 16 * 1024 * 1024;

/// V38: maximum decompressed VPT size (1 MB).  A 640×400 / 2×2 frame has
/// 320×200 = 64000 blocks × 2 = 128 KB.  1 MB handles any realistic case.
const MAX_VPT_SIZE: usize = 1024 * 1024;

/// V38: maximum total audio output size (256 MB).
const MAX_AUDIO_TOTAL: usize = 256 * 1024 * 1024;

// ─── Decoded Frame / Audio ───────────────────────────────────────────────────

/// A decoded VQA video frame: palette-indexed pixel data.
#[derive(Debug, Clone)]
pub struct VqaFrame {
    /// Palette-indexed pixels, row-major, `width × height` bytes.
    pub pixels: Vec<u8>,
    /// Active 256-entry palette as 768 bytes (R, G, B × 256).
    /// Values are 8-bit (0–255), already scaled from 6-bit VGA if needed.
    pub palette: [u8; 768],
}

/// Decoded VQA audio: raw PCM samples.
#[derive(Debug, Clone)]
pub struct VqaAudio {
    /// Signed 16-bit PCM samples.  For stereo, samples are interleaved
    /// `[L, R, L, R, …]`.
    pub samples: Vec<i16>,
    /// Sample rate in Hz (e.g. 22050).
    pub sample_rate: u16,
    /// Number of channels (1 = mono, 2 = stereo).
    pub channels: u8,
}

// ─── Decoder State ───────────────────────────────────────────────────────────

/// Internal decoder state that tracks the codebook across frames.
#[derive(Debug)]
pub(crate) struct VqaDecodeState {
    header: VqaHeader,
    /// Active codebook: a flat byte array of `cb_entries × block_size` bytes.
    /// Each entry is `block_w × block_h` palette-indexed pixels.
    codebook: Vec<u8>,
    /// Partial codebook accumulator for CBP chunks.
    cbp_buffer: Vec<u8>,
    /// Number of CBP parts accumulated so far.
    cbp_count: u8,
    /// Encoding mode for the in-progress CBP accumulator.
    cbp_encoding: Option<CbpEncoding>,
    /// Active palette (768 bytes: R, G, B × 256).
    palette: [u8; 768],
    /// Block size in pixels: `block_w × block_h`.
    block_size: usize,
    /// Number of blocks horizontally.
    blocks_x: usize,
    /// Number of blocks vertically.
    blocks_y: usize,
    /// Hi-value threshold for solid-fill detection.
    /// 0x0F for normal VQAs, 0xFF for hi-res (640×400).
    fill_marker: u8,
}

/// Encoding mode for a partial codebook update cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CbpEncoding {
    /// Raw, uncompressed `CBP0` data.
    Raw,
    /// LCW-compressed `CBPZ` data.
    Compressed,
}

impl VqaDecodeState {
    /// Creates a new decoder from a parsed VQA header.
    pub(crate) fn new(header: VqaHeader) -> Self {
        let block_w = (header.block_w as usize).max(1);
        let block_h = (header.block_h as usize).max(1);
        let block_size = block_w.saturating_mul(block_h);
        let width = header.width as usize;
        let height = header.height as usize;
        let blocks_x = width.checked_div(block_w).unwrap_or(0);
        let blocks_y = height.checked_div(block_h).unwrap_or(0);

        // Determine fill marker: 0xFF for hi-res (block_h == 4), 0x0F otherwise.
        // Per VQA_INFO.TXT: "If BlockW=2 -> 0x0f is used, if BlockH=4 -> 0xff is used"
        let fill_marker = if block_h == 4 { 0xFF } else { 0x0F };

        // Default VGA palette: all black.
        let palette = [0u8; 768];

        VqaDecodeState {
            header,
            codebook: Vec::new(),
            cbp_buffer: Vec::new(),
            cbp_count: 0,
            cbp_encoding: None,
            palette,
            block_size,
            blocks_x,
            blocks_y,
            fill_marker,
        }
    }

    fn process_frame_chunk<T, F>(&mut self, data: &[u8], render: F) -> Result<Option<T>, Error>
    where
        F: FnOnce(&mut Self, &[u8]) -> Result<T, Error>,
    {
        let mut pos: usize = 0;
        let mut vpt_data: Option<Cow<'_, [u8]>> = None;
        let mut deferred_cbp: Vec<(&[u8], CbpEncoding)> = Vec::new();

        while pos.saturating_add(8) <= data.len() {
            let fourcc = data
                .get(pos..pos.saturating_add(4))
                .ok_or(Error::UnexpectedEof {
                    needed: pos.saturating_add(4),
                    available: data.len(),
                })?;
            let size_bytes = data
                .get(pos.saturating_add(4)..pos.saturating_add(8))
                .ok_or(Error::UnexpectedEof {
                    needed: pos.saturating_add(8),
                    available: data.len(),
                })?;
            // Sub-chunk sizes are big-endian (IFF convention).
            let mut size_buf = [0u8; 4];
            size_buf.copy_from_slice(size_bytes);
            let chunk_size = u32::from_be_bytes(size_buf) as usize;

            let payload_start = pos.saturating_add(8);
            let payload_end = payload_start.saturating_add(chunk_size);
            let payload = data
                .get(payload_start..payload_end)
                .ok_or(Error::InvalidOffset {
                    offset: payload_end,
                    bound: data.len(),
                })?;

            match fourcc {
                b"CBF0" => {
                    self.copy_codebook(payload)?;
                }
                b"CBFZ" => {
                    lcw::decompress_into(payload, &mut self.codebook, MAX_CODEBOOK_SIZE)?;
                    self.cbp_buffer.clear();
                    self.cbp_count = 0;
                    self.cbp_encoding = None;
                }
                b"CBP0" => {
                    deferred_cbp.push((payload, CbpEncoding::Raw));
                }
                b"CBPZ" => {
                    deferred_cbp.push((payload, CbpEncoding::Compressed));
                }
                b"CPL0" => {
                    self.set_palette(payload);
                }
                b"CPLZ" => {
                    let decompressed = lcw::decompress(payload, 768)?;
                    self.set_palette(&decompressed);
                }
                b"VPT0" | b"VPTK" => {
                    vpt_data = Some(Cow::Borrowed(payload));
                }
                b"VPTZ" | b"VPTD" => {
                    let decompressed = lcw::decompress(payload, MAX_VPT_SIZE)?;
                    vpt_data = Some(Cow::Owned(decompressed));
                }
                _ => {}
            }

            let padded = chunk_size.saturating_add(chunk_size & 1);
            pos = payload_start.saturating_add(padded);
        }

        let result = if let Some(vpt) = vpt_data {
            Some(render(self, vpt.as_ref())?)
        } else {
            None
        };

        for (cbp_data, encoding) in deferred_cbp {
            self.accumulate_cbp(cbp_data, encoding)?;
        }

        Ok(result)
    }

    /// Processes a VQFR/VQFL container chunk's sub-chunks and allocates the
    /// next decoded frame when a VPT payload is present.
    fn decode_frame_chunk(&mut self, data: &[u8]) -> Result<Option<VqaFrame>, Error> {
        self.process_frame_chunk(data, |decoder, vpt| decoder.render_frame(vpt))
    }

    /// Processes a VQFR/VQFL container chunk and renders directly into `pixels`.
    fn decode_frame_chunk_into(&mut self, data: &[u8], pixels: &mut [u8]) -> Result<bool, Error> {
        Ok(self
            .process_frame_chunk(data, |decoder, vpt| {
                decoder.render_frame_into(vpt, pixels)?;
                Ok(())
            })?
            .is_some())
    }

    /// Sets the active codebook from a full CBF payload.
    fn copy_codebook(&mut self, data: &[u8]) -> Result<(), Error> {
        if data.len() > MAX_CODEBOOK_SIZE {
            return Err(Error::InvalidSize {
                value: data.len(),
                limit: MAX_CODEBOOK_SIZE,
                context: "VQA codebook",
            });
        }
        self.codebook.clear();
        self.codebook.extend_from_slice(data);
        self.cbp_buffer.clear();
        self.cbp_count = 0;
        self.cbp_encoding = None;
        Ok(())
    }

    fn replace_codebook(&mut self, data: Vec<u8>) -> Result<(), Error> {
        if data.len() > MAX_CODEBOOK_SIZE {
            return Err(Error::InvalidSize {
                value: data.len(),
                limit: MAX_CODEBOOK_SIZE,
                context: "VQA codebook",
            });
        }
        self.codebook = data;
        // Reset CBP accumulator when a full codebook arrives.
        self.cbp_buffer.clear();
        self.cbp_count = 0;
        self.cbp_encoding = None;
        Ok(())
    }

    /// Accumulates a partial codebook (CBP) chunk.
    ///
    /// After `groupsize` parts are collected, the accumulated data is either
    /// used directly (CBP0) or decompressed (CBPZ) and replaces the codebook.
    fn accumulate_cbp(&mut self, data: &[u8], encoding: CbpEncoding) -> Result<(), Error> {
        // V38: cap accumulated CBP buffer size.
        if self.cbp_buffer.len().saturating_add(data.len()) > MAX_CODEBOOK_SIZE {
            return Err(Error::InvalidSize {
                value: self.cbp_buffer.len().saturating_add(data.len()),
                limit: MAX_CODEBOOK_SIZE,
                context: "VQA CBP accumulator",
            });
        }
        // A codebook update cycle must stay in one encoding mode.  Falling
        // back across CBP0/CBPZ boundaries would silently reinterpret bytes.
        if let Some(existing) = self.cbp_encoding {
            if existing != encoding {
                return Err(Error::DecompressionError {
                    reason: "mixed CBP0 and CBPZ chunks in one codebook cycle",
                });
            }
        } else {
            self.cbp_encoding = Some(encoding);
        }
        self.cbp_buffer.extend_from_slice(data);
        self.cbp_count = self.cbp_count.saturating_add(1);

        let groupsize = self.header.groupsize.max(1);
        if self.cbp_count >= groupsize {
            // CBPZ is declared-compressed input.  If LCW decompression fails,
            // the frame data is malformed and must not silently fall back to
            // raw bytes.
            match self.cbp_encoding.unwrap_or(CbpEncoding::Raw) {
                CbpEncoding::Raw => {
                    // Swap buffers: cbp_buffer becomes the new codebook,
                    // and the old codebook buffer will be cleared and reused
                    // as the next CBP accumulator.
                    std::mem::swap(&mut self.codebook, &mut self.cbp_buffer);
                }
                CbpEncoding::Compressed => {
                    lcw::decompress_into(&self.cbp_buffer, &mut self.codebook, MAX_CODEBOOK_SIZE)?;
                }
            }
            self.cbp_buffer.clear();
            self.cbp_count = 0;
            self.cbp_encoding = None;
        }
        Ok(())
    }

    /// Sets the active palette from a CPL payload.
    ///
    /// VGA palettes use 6-bit values (0–63); we scale to 8-bit (0–255).
    fn set_palette(&mut self, data: &[u8]) {
        let copy_len = data.len().min(768);
        // Scale 6-bit VGA values to 8-bit: multiply by 4 (shift left 2)
        // then OR with the top 2 bits for accurate rounding.
        for i in 0..copy_len {
            let val = data.get(i).copied().unwrap_or(0);
            // Mask to 6 bits per VQA_INFO: "mask out the bits 6 and 7"
            let v6 = val & 0x3F;
            // Scale 0–63 → 0–255: (v6 << 2) | (v6 >> 4)
            let v8 = (v6 << 2) | (v6 >> 4);
            if let Some(slot) = self.palette.get_mut(i) {
                *slot = v8;
            }
        }
    }

    fn render_frame_into(&self, vpt: &[u8], pixels: &mut [u8]) -> Result<(), Error> {
        let geo = VqaRenderGeometry {
            width: self.header.width as usize,
            height: self.header.height as usize,
            block_w: (self.header.block_w as usize).max(1),
            block_h: (self.header.block_h as usize).max(1),
            blocks_x: self.blocks_x,
            blocks_y: self.blocks_y,
            block_size: self.block_size,
            fill_marker: self.fill_marker,
        };
        // Build a frequency-ordered compact codebook for this frame.
        // Hot entries are packed at the front so repeated accesses stay in
        // L1 cache.  Falls back to the original codebook when compaction
        // cannot proceed safely (all-fill frame, tiny codebook, etc.).
        if let Some((compact_cb, compact_vpt)) = build_compact_codebook(&geo, &self.codebook, vpt) {
            render_frame_pixels(&geo, &compact_cb, &compact_vpt, pixels)
        } else {
            render_frame_pixels(&geo, &self.codebook, vpt, pixels)
        }
    }

    fn render_frame(&self, vpt: &[u8]) -> Result<VqaFrame, Error> {
        let mut pixels =
            vec![0u8; (self.header.width as usize).saturating_mul(self.header.height as usize)];
        self.render_frame_into(vpt, &mut pixels)?;
        Ok(VqaFrame {
            pixels,
            palette: self.palette,
        })
    }

    #[inline]
    pub(crate) fn palette(&self) -> &[u8; 768] {
        &self.palette
    }

    pub(crate) fn apply_chunk(
        &mut self,
        fourcc: &[u8; 4],
        data: &[u8],
    ) -> Result<Option<VqaFrame>, Error> {
        match fourcc {
            b"VQFR" | b"VQFK" | b"VQFL" => self.decode_frame_chunk(data),
            b"CBF0" => {
                self.copy_codebook(data)?;
                Ok(None)
            }
            b"CBFZ" => {
                let decompressed = lcw::decompress(data, MAX_CODEBOOK_SIZE)?;
                self.replace_codebook(decompressed)?;
                Ok(None)
            }
            b"CBP0" => {
                self.accumulate_cbp(data, CbpEncoding::Raw)?;
                Ok(None)
            }
            b"CBPZ" => {
                self.accumulate_cbp(data, CbpEncoding::Compressed)?;
                Ok(None)
            }
            b"CPL0" => {
                self.set_palette(data);
                Ok(None)
            }
            b"CPLZ" => {
                if let Ok(decompressed) = lcw::decompress(data, 768) {
                    self.set_palette(&decompressed);
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn apply_chunk_into(
        &mut self,
        fourcc: &[u8; 4],
        data: &[u8],
        pixels: &mut [u8],
    ) -> Result<bool, Error> {
        match fourcc {
            b"VQFR" | b"VQFK" | b"VQFL" => self.decode_frame_chunk_into(data, pixels),
            b"CBF0" => {
                self.copy_codebook(data)?;
                Ok(false)
            }
            b"CBFZ" => {
                let decompressed = lcw::decompress(data, MAX_CODEBOOK_SIZE)?;
                self.replace_codebook(decompressed)?;
                Ok(false)
            }
            b"CBP0" => {
                self.accumulate_cbp(data, CbpEncoding::Raw)?;
                Ok(false)
            }
            b"CBPZ" => {
                self.accumulate_cbp(data, CbpEncoding::Compressed)?;
                Ok(false)
            }
            b"CPL0" => {
                self.set_palette(data);
                Ok(false)
            }
            b"CPLZ" => {
                if let Ok(decompressed) = lcw::decompress(data, 768) {
                    self.set_palette(&decompressed);
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }
}

// ─── Public API ──────────────────────────────────────────────────────────────

impl VqaFile<'_> {
    /// Decodes all video frames from the VQA container.
    ///
    /// Iterates chunks in file order, maintaining codebook/palette state,
    /// and produces one [`VqaFrame`] per VQFR/VQFL chunk that contains a
    /// VPT sub-chunk.  Each frame includes its own palette snapshot (palettes
    /// can change mid-stream via CPL chunks).
    ///
    /// Only version 2 (8-bit palette-indexed) VQA files are supported.
    ///
    /// # Errors
    ///
    /// - [`Error::DecompressionError`] if LCW decompression of CBFZ/VPTZ fails.
    /// - [`Error::UnexpectedEof`] if chunk data is truncated.
    /// - [`Error::InvalidSize`] if codebook exceeds V38 size caps.
    pub fn decode_frames(&self) -> Result<Vec<VqaFrame>, Error> {
        let mut decoder = VqaDecodeState::new(self.header.clone());
        // V38: cap pre-allocation to prevent DoS from crafted num_frames.
        // 8192 frames at 15 fps > 9 minutes — generous for any real VQA.
        const MAX_VQA_FRAMES: usize = 8192;
        let cap = (self.header.num_frames as usize).min(MAX_VQA_FRAMES);
        let mut frames = Vec::with_capacity(cap);

        for chunk in &self.chunks {
            if let Some(frame) = decoder.apply_chunk(&chunk.fourcc, chunk.data)? {
                frames.push(frame);
            }
        }

        Ok(frames)
    }

    /// Extracts and decodes the audio stream from the VQA container.
    ///
    /// Collects all SND0/SND1/SND2 chunks in file order and decodes them
    /// into a contiguous PCM sample buffer.  IMA ADPCM state is maintained
    /// across chunks (per the VQA spec).
    ///
    /// Returns `None` if the VQA has no audio (`freq == 0` or no SND chunks).
    ///
    /// # Errors
    ///
    /// - [`Error::DecompressionError`] if audio data is malformed.
    /// - [`Error::InvalidSize`] if total audio exceeds V38 cap.
    pub fn extract_audio(&self) -> Result<Option<VqaAudio>, Error> {
        if !self.header.has_audio() {
            return Ok(None);
        }

        let stereo = self.header.is_stereo();
        let mut all_samples: Vec<i16> = Vec::new();

        // SND2 IMA ADPCM state is maintained across chunk boundaries.
        let mut ima_l_sample: i32 = 0;
        let mut ima_l_index: usize = 0;
        let mut ima_r_sample: i32 = 0;
        let mut ima_r_index: usize = 0;
        // SND1 Westwood ADPCM predictor is maintained across chunk boundaries.
        let mut snd1_cur_sample: i16 = 0x80;

        for chunk in &self.chunks {
            let added = match &chunk.fourcc {
                b"SND0" => append_snd0(&mut all_samples, chunk.data, self.header.bits)?,
                // SND1: Westwood ADPCM, predictor state carried across chunks.
                b"SND1" => {
                    append_snd1_stateful(&mut all_samples, chunk.data, &mut snd1_cur_sample)?
                }
                // SND2: IMA ADPCM, state is maintained across chunks per VQA spec.
                b"SND2" => append_snd2_stateful(
                    &mut all_samples,
                    chunk.data,
                    stereo,
                    &mut ima_l_sample,
                    &mut ima_l_index,
                    &mut ima_r_sample,
                    &mut ima_r_index,
                )?,
                _ => continue,
            };
            if all_samples.len() > MAX_AUDIO_TOTAL {
                return Err(Error::InvalidSize {
                    value: all_samples.len(),
                    limit: MAX_AUDIO_TOTAL,
                    context: "VQA audio output",
                });
            }
            let _ = added;
        }

        if all_samples.is_empty() {
            return Ok(None);
        }

        Ok(Some(VqaAudio {
            samples: all_samples,
            sample_rate: if self.header.freq == 0 {
                22050
            } else {
                self.header.freq
            },
            channels: if self.header.channels == 0 {
                1
            } else {
                self.header.channels
            },
        }))
    }
}
