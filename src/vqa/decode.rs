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

use super::snd::{decode_snd0, decode_snd1, decode_snd2};
use super::{VqaFile, VqaHeader};

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
struct VqaDecoder<'a> {
    header: &'a VqaHeader,
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

impl<'a> VqaDecoder<'a> {
    /// Creates a new decoder from a parsed VQA header.
    fn new(header: &'a VqaHeader) -> Self {
        let block_w = (header.block_w as usize).max(1);
        let block_h = (header.block_h as usize).max(1);
        let block_size = block_w.saturating_mul(block_h);
        let width = header.width as usize;
        let height = header.height as usize;
        let blocks_x = if block_w > 0 { width / block_w } else { 0 };
        let blocks_y = if block_h > 0 { height / block_h } else { 0 };

        // Determine fill marker: 0xFF for hi-res (block_h == 4), 0x0F otherwise.
        // Per VQA_INFO.TXT: "If BlockW=2 -> 0x0f is used, if BlockH=4 -> 0xff is used"
        let fill_marker = if block_h == 4 { 0xFF } else { 0x0F };

        // Default VGA palette: all black.
        let palette = [0u8; 768];

        VqaDecoder {
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

    /// Processes a VQFR/VQFL container chunk's sub-chunks.
    ///
    /// The VQFR chunk itself contains nested sub-chunks (CBF0/CBFZ, CBP0/CBPZ,
    /// CPL0/CPLZ, VPT0/VPTZ).  This method iterates them and returns a
    /// decoded frame if a VPT chunk is found.
    fn decode_frame_chunk(&mut self, data: &[u8]) -> Result<Option<VqaFrame>, Error> {
        let mut pos: usize = 0;
        let mut vpt_data: Option<Vec<u8>> = None;
        // Deferred CBP chunks: accumulate AFTER rendering the current frame
        // so the new codebook takes effect at the start of the next group,
        // not during the current frame.
        let mut deferred_cbp: Vec<(Vec<u8>, CbpEncoding)> = Vec::new();

        // Iterate sub-chunks within the VQFR/VQFL payload.
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

            // Process sub-chunk based on FourCC.
            // Last byte '0' = uncompressed, 'Z' = LCW-compressed.
            match fourcc {
                // ── Full codebook ────────────────────────────────────
                b"CBF0" => {
                    self.set_codebook(payload.to_vec())?;
                }
                b"CBFZ" => {
                    let decompressed = lcw::decompress(payload, MAX_CODEBOOK_SIZE)?;
                    self.set_codebook(decompressed)?;
                }
                // ── Partial codebook (deferred until after VPT render) ─
                b"CBP0" => {
                    deferred_cbp.push((payload.to_vec(), CbpEncoding::Raw));
                }
                b"CBPZ" => {
                    deferred_cbp.push((payload.to_vec(), CbpEncoding::Compressed));
                }
                // ── Palette ──────────────────────────────────────────
                b"CPL0" => {
                    self.set_palette(payload);
                }
                b"CPLZ" => {
                    let decompressed = lcw::decompress(payload, 768)?;
                    self.set_palette(&decompressed);
                }
                // ── Vector Pointer Table ─────────────────────────────
                b"VPT0" => {
                    vpt_data = Some(payload.to_vec());
                }
                b"VPTZ" => {
                    let decompressed = lcw::decompress(payload, MAX_VPT_SIZE)?;
                    vpt_data = Some(decompressed);
                }
                _ => {
                    // Unknown sub-chunks: skip silently (permissive parsing).
                }
            }

            // Advance to next sub-chunk (padded to even boundary).
            let padded = chunk_size.saturating_add(chunk_size & 1);
            pos = payload_start.saturating_add(padded);
        }

        // Render the frame FIRST (using the current codebook), THEN apply
        // any deferred CBP parts.  Per VQA_INFO, the accumulated CBP
        // codebook takes effect at the start of the next group, not during
        // the frame that provides the last part.
        let result = if let Some(vpt) = vpt_data {
            let frame = self.render_frame(&vpt)?;
            Some(frame)
        } else {
            None
        };

        // Now accumulate deferred CBP chunks (may complete and swap codebook).
        for (cbp_data, encoding) in deferred_cbp {
            self.accumulate_cbp(&cbp_data, encoding)?;
        }

        Ok(result)
    }

    /// Sets the active codebook from a full CBF payload.
    fn set_codebook(&mut self, data: Vec<u8>) -> Result<(), Error> {
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
            let new_codebook = match self.cbp_encoding.unwrap_or(CbpEncoding::Raw) {
                CbpEncoding::Raw => self.cbp_buffer.clone(),
                CbpEncoding::Compressed => lcw::decompress(&self.cbp_buffer, MAX_CODEBOOK_SIZE)?,
            };
            self.codebook = new_codebook;
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

    /// Renders a frame from the Vector Pointer Table.
    ///
    /// Version 2 layout: the VPT is split into two halves.
    ///
    /// - lo_val = vpt\[by * blocks_x + bx\]
    /// - hi_val = vpt\[blocks_x * blocks_y + by * blocks_x + bx\]
    ///
    /// If hi_val == fill_marker → solid fill with color lo_val.
    /// Otherwise → copy codebook block #(hi_val * 256 + lo_val).
    fn render_frame(&self, vpt: &[u8]) -> Result<VqaFrame, Error> {
        let width = self.header.width as usize;
        let height = self.header.height as usize;
        let block_w = (self.header.block_w as usize).max(1);
        let block_h = (self.header.block_h as usize).max(1);
        let total_blocks = self.blocks_x.saturating_mul(self.blocks_y);

        // VPT must be at least 2 × total_blocks bytes.
        let needed = total_blocks.saturating_mul(2);
        if vpt.len() < needed {
            return Err(Error::UnexpectedEof {
                needed,
                available: vpt.len(),
            });
        }

        let mut pixels = vec![0u8; width.saturating_mul(height)];

        for by in 0..self.blocks_y {
            for bx in 0..self.blocks_x {
                let idx = by.saturating_mul(self.blocks_x).saturating_add(bx);
                let lo_val = vpt.get(idx).copied().unwrap_or(0);
                let hi_val = vpt
                    .get(total_blocks.saturating_add(idx))
                    .copied()
                    .unwrap_or(0);

                let block_x = bx.saturating_mul(block_w);
                let block_y = by.saturating_mul(block_h);

                if hi_val == self.fill_marker {
                    // Solid fill: every pixel in this block = lo_val.
                    for row in 0..block_h {
                        let y = block_y.saturating_add(row);
                        if y >= height {
                            break;
                        }
                        for col in 0..block_w {
                            let x = block_x.saturating_add(col);
                            if x >= width {
                                break;
                            }
                            let dst = y.saturating_mul(width).saturating_add(x);
                            if let Some(px) = pixels.get_mut(dst) {
                                *px = lo_val;
                            }
                        }
                    }
                } else {
                    // Codebook lookup: block index = hi_val * 256 + lo_val.
                    let block_index = (hi_val as usize)
                        .saturating_mul(256)
                        .saturating_add(lo_val as usize);
                    let cb_offset = block_index.saturating_mul(self.block_size);

                    for row in 0..block_h {
                        let y = block_y.saturating_add(row);
                        if y >= height {
                            break;
                        }
                        for col in 0..block_w {
                            let x = block_x.saturating_add(col);
                            if x >= width {
                                break;
                            }
                            let src_off = cb_offset
                                .saturating_add(row.saturating_mul(block_w).saturating_add(col));
                            let pixel = self.codebook.get(src_off).copied().unwrap_or(0);
                            let dst = y.saturating_mul(width).saturating_add(x);
                            if let Some(px) = pixels.get_mut(dst) {
                                *px = pixel;
                            }
                        }
                    }
                }
            }
        }

        Ok(VqaFrame {
            pixels,
            palette: self.palette,
        })
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
        let mut decoder = VqaDecoder::new(&self.header);
        // V38: cap pre-allocation to prevent DoS from crafted num_frames.
        // 8192 frames at 15 fps > 9 minutes — generous for any real VQA.
        const MAX_VQA_FRAMES: usize = 8192;
        let cap = (self.header.num_frames as usize).min(MAX_VQA_FRAMES);
        let mut frames = Vec::with_capacity(cap);

        for chunk in &self.chunks {
            match &chunk.fourcc {
                b"VQFR" | b"VQFL" => {
                    if let Some(frame) = decoder.decode_frame_chunk(chunk.data)? {
                        frames.push(frame);
                    }
                }
                // Top-level CBF/CBP/CPL chunks (outside VQFR) can exist in
                // some VQA variants.  Process them to update decoder state.
                b"CBF0" => {
                    decoder.set_codebook(chunk.data.to_vec())?;
                }
                b"CBFZ" => {
                    let decompressed = lcw::decompress(chunk.data, MAX_CODEBOOK_SIZE)?;
                    decoder.set_codebook(decompressed)?;
                }
                b"CBP0" => {
                    decoder.accumulate_cbp(chunk.data, CbpEncoding::Raw)?;
                }
                b"CBPZ" => {
                    decoder.accumulate_cbp(chunk.data, CbpEncoding::Compressed)?;
                }
                b"CPL0" => {
                    decoder.set_palette(chunk.data);
                }
                b"CPLZ" => {
                    if let Ok(decompressed) = lcw::decompress(chunk.data, 768) {
                        decoder.set_palette(&decompressed);
                    }
                }
                _ => {
                    // Other chunk types (SND*, FINF, etc.) handled elsewhere.
                }
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

        // IMA ADPCM state must be maintained across SND2 chunks.
        let mut left_sample: i32 = 0;
        let mut left_index: usize = 0;
        let mut right_sample: i32 = 0;
        let mut right_index: usize = 0;

        for chunk in &self.chunks {
            match &chunk.fourcc {
                b"SND0" => {
                    // Raw PCM data.
                    let samples = decode_snd0(chunk.data, self.header.bits)?;
                    if all_samples.len().saturating_add(samples.len()) > MAX_AUDIO_TOTAL {
                        return Err(Error::InvalidSize {
                            value: all_samples.len().saturating_add(samples.len()),
                            limit: MAX_AUDIO_TOTAL,
                            context: "VQA audio output",
                        });
                    }
                    all_samples.extend_from_slice(&samples);
                }
                b"SND1" => {
                    // Westwood ADPCM (8-bit, unsigned).
                    let samples = decode_snd1(chunk.data)?;
                    if all_samples.len().saturating_add(samples.len()) > MAX_AUDIO_TOTAL {
                        return Err(Error::InvalidSize {
                            value: all_samples.len().saturating_add(samples.len()),
                            limit: MAX_AUDIO_TOTAL,
                            context: "VQA audio output",
                        });
                    }
                    all_samples.extend_from_slice(&samples);
                }
                b"SND2" => {
                    // IMA ADPCM (16-bit).  State carried across chunks.
                    let samples = decode_snd2(
                        chunk.data,
                        stereo,
                        &mut left_sample,
                        &mut left_index,
                        &mut right_sample,
                        &mut right_index,
                    )?;
                    if all_samples.len().saturating_add(samples.len()) > MAX_AUDIO_TOTAL {
                        return Err(Error::InvalidSize {
                            value: all_samples.len().saturating_add(samples.len()),
                            limit: MAX_AUDIO_TOTAL,
                            context: "VQA audio output",
                        });
                    }
                    all_samples.extend_from_slice(&samples);
                }
                _ => {}
            }
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
