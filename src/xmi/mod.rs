// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! XMIDI parser and XMI→MID converter (`.xmi`).
//!
//! XMIDI (eXtended MIDI) is Miles Sound System's container format for game
//! music, used in Westwood titles from Legend of Kyrandia through Lands of
//! Lore.  An XMI file wraps one or more MIDI sequences inside an IFF
//! FORM:XDIR / FORM:XMID container with Miles-specific timing (IFTHEN
//! delay opcodes instead of standard MIDI delta-time).
//!
//! ## File Layout (IFF Structure)
//!
//! ```text
//! FORM:XDIR
//!   INFO  — u16 LE: number of sequences
//! CAT :XMID
//!   FORM:XMID  (repeated per sequence)
//!     TIMB — optional: instrument/timbre table
//!     RBRN — optional: branch point table
//!     EVNT — the MIDI event stream with IFTHEN timing
//! ```
//!
//! ## XMI Timing
//!
//! XMI uses a fixed 120 ticks-per-beat resolution.  Note-On events carry
//! an implicit duration (unlike standard MIDI where Note-Off is explicit).
//! The converter generates matching Note-Off events at the correct delta
//! offset.
//!
//! ## Conversion to Standard MIDI
//!
//! The `to_mid()` function produces a valid Type 0 SMF by:
//! 1. Stripping the IFF wrapper (FORM, CAT, chunk headers)
//! 2. Converting IFTHEN delay opcodes to standard MIDI variable-length
//!    delta-times
//! 3. Generating explicit Note-Off events from XMI note durations
//! 4. Adding an End-of-Track meta event
//!
//! ## References
//!
//! - Miles Sound System documentation
//! - AIL2 / Miles driver reverse engineering (community)
//! - [Shikadi Modding Wiki: XMI Format](https://moddingwiki.shikadi.net/wiki/XMI_Format)

use crate::error::Error;
use crate::read::{read_u16_le, read_u32_le, read_u8};

mod convert;

pub use convert::to_mid;

#[cfg(test)]
use convert::{read_vlq, write_vlq_to};

// ── Constants ────────────────────────────────────────────────────────────────

/// IFF "FORM" chunk identifier.
const FORM_TAG: &[u8; 4] = b"FORM";

/// IFF "CAT " chunk identifier (note trailing space).
const CAT_TAG: &[u8; 4] = b"CAT ";

/// XMID form type identifier.
const XMID_TAG: &[u8; 4] = b"XMID";

/// XDIR form type identifier.
const XDIR_TAG: &[u8; 4] = b"XDIR";

/// EVNT chunk identifier (contains the MIDI event stream).
const EVNT_TAG: &[u8; 4] = b"EVNT";

/// TIMB chunk identifier (instrument/timbre table).
const TIMB_TAG: &[u8; 4] = b"TIMB";

/// INFO chunk identifier (sequence count).
const INFO_TAG: &[u8; 4] = b"INFO";

/// XMI fixed timing: 120 ticks per quarter note.
///
/// This is the standard XMI resolution — all XMI files use this value.
pub const XMI_TICKS_PER_BEAT: u16 = 120;

/// Maximum number of sequences in an XMI file.
///
/// V38: bounds check for the sequence count field.  Real XMI files contain
/// at most a few dozen sequences.
const MAX_SEQUENCES: usize = 256;

/// Maximum event data size per sequence.
///
/// V38: prevents unbounded allocation from malformed EVNT chunk sizes.
const MAX_EVNT_SIZE: usize = 16 * 1024 * 1024; // 16 MiB

// ── Types ────────────────────────────────────────────────────────────────────

/// A timbre (instrument patch) entry from the TIMB chunk.
///
/// Each entry specifies a General MIDI patch and bank that the sequence
/// requires for correct playback.  Miles Sound System uses this to
/// preload instrument samples before playback begins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmiTimbre {
    /// MIDI patch number (0–127).
    pub patch: u8,
    /// MIDI bank number (0–127).
    pub bank: u8,
}

/// A single XMI sequence extracted from the IFF container.
///
/// Contains the raw EVNT data (MIDI events with IFTHEN timing) and an
/// optional timbre table.  Use [`to_mid()`] to convert to standard SMF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmiSequence<'a> {
    /// Optional timbre (instrument) table from the TIMB chunk.
    pub timbres: Vec<XmiTimbre>,
    /// Raw EVNT chunk data: MIDI events with XMI IFTHEN timing.
    pub event_data: &'a [u8],
}

/// A parsed XMI file containing one or more MIDI sequences.
///
/// The parser extracts the IFF structure, validates chunk boundaries, and
/// provides access to individual sequences.  Each sequence can be converted
/// to standard MIDI via [`to_mid()`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmiFile<'a> {
    /// The sequences contained in this XMI file.
    pub sequences: Vec<XmiSequence<'a>>,
}

impl<'a> XmiFile<'a> {
    /// Parses an XMI file from a byte slice.
    ///
    /// Extracts the IFF FORM:XDIR / CAT:XMID structure and returns the
    /// contained sequences with their EVNT data and optional TIMB tables.
    ///
    /// # Errors
    ///
    /// - [`Error::InvalidMagic`] — missing FORM or XMID identifiers
    /// - [`Error::UnexpectedEof`] — data is truncated
    /// - [`Error::InvalidSize`] — sequence count or chunk size exceeds limits
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        // V38: minimum IFF FORM header is 12 bytes (4 tag + 4 size + 4 type).
        if data.len() < 12 {
            return Err(Error::UnexpectedEof {
                needed: 12,
                available: data.len(),
            });
        }

        // Read the outer container — can be FORM:XDIR or FORM:XMID or
        // CAT:XMID.  Some XMI files start directly with FORM:XMID for
        // single-sequence files.
        let tag = data.get(..4).ok_or(Error::UnexpectedEof {
            needed: 4,
            available: data.len(),
        })?;

        if tag == FORM_TAG {
            let form_type = data.get(8..12).ok_or(Error::UnexpectedEof {
                needed: 12,
                available: data.len(),
            })?;
            if form_type == XDIR_TAG {
                // Multi-sequence: FORM:XDIR followed by CAT:XMID.
                Self::parse_xdir(data)
            } else if form_type == XMID_TAG {
                // Single-sequence: FORM:XMID directly.
                let seq = Self::parse_xmid_form(data, 0)?;
                Ok(XmiFile {
                    sequences: vec![seq],
                })
            } else {
                Err(Error::InvalidMagic { context: "XMI" })
            }
        } else if tag == CAT_TAG {
            // Some files start directly with CAT:XMID (no XDIR/INFO header).
            Self::parse_cat_xmid(data, 0, None)
        } else {
            Err(Error::InvalidMagic { context: "XMI" })
        }
    }

    /// Parses the FORM:XDIR header, reads the INFO sequence count, and
    /// then parses the CAT:XMID body.
    fn parse_xdir(data: &'a [u8]) -> Result<Self, Error> {
        // Some legacy XMI files store an incorrect outer FORM size, so we
        // do not trust the XDIR boundary.  Instead, walk the adjacent IFF
        // chunk headers structurally: INFO lives in the XDIR body and the
        // next top-level sibling is CAT:XMID.
        let mut declared_count = None;
        let mut pos = 12usize;

        while pos.saturating_add(8) <= data.len() {
            let chunk_tag = data
                .get(pos..pos.saturating_add(4))
                .ok_or(Error::UnexpectedEof {
                    needed: pos.saturating_add(4),
                    available: data.len(),
                })?;
            let chunk_size = read_u32_le(data, pos.saturating_add(4))
                .map(u32::from_be)
                .unwrap_or(0) as usize;
            let payload_start = pos.saturating_add(8);
            let payload_end = payload_start.saturating_add(chunk_size);

            if chunk_tag == CAT_TAG {
                return Self::parse_cat_xmid(data, pos, declared_count);
            }

            let payload = data
                .get(payload_start..payload_end)
                .ok_or(Error::InvalidOffset {
                    offset: payload_end,
                    bound: data.len(),
                })?;
            if chunk_tag == INFO_TAG {
                // INFO chunk: u16 LE declared sequence count.
                declared_count = read_u16_le(payload, 0).ok();
            }

            let padded_size = if chunk_size % 2 == 1 {
                chunk_size.saturating_add(1)
            } else {
                chunk_size
            };
            let next = payload_start.saturating_add(padded_size);
            if next <= pos {
                break;
            }
            pos = next;
        }

        Err(Error::InvalidMagic {
            context: "XMI: no CAT chunk found",
        })
    }

    /// Parses a `CAT :XMID` container and extracts all `FORM:XMID` sequences.
    ///
    /// `declared_count` is the sequence count from the INFO chunk (if an
    /// XDIR header was present).  When provided, the parser uses it as an
    /// upper bound on the number of sequences to extract, cross-validating
    /// the INFO declaration against the actual FORM:XMID chunks found.
    fn parse_cat_xmid(
        data: &'a [u8],
        offset: usize,
        declared_count: Option<u16>,
    ) -> Result<Self, Error> {
        // Verify CAT tag.
        let tag = data
            .get(offset..offset.saturating_add(4))
            .ok_or(Error::UnexpectedEof {
                needed: offset.saturating_add(4),
                available: data.len(),
            })?;
        if tag != CAT_TAG {
            return Err(Error::InvalidMagic {
                context: "XMI: expected CAT",
            });
        }

        // CAT type should be XMID.
        let type_offset = offset.saturating_add(8);
        let cat_type =
            data.get(type_offset..type_offset.saturating_add(4))
                .ok_or(Error::UnexpectedEof {
                    needed: type_offset.saturating_add(4),
                    available: data.len(),
                })?;
        if cat_type != XMID_TAG {
            return Err(Error::InvalidMagic {
                context: "XMI: expected XMID in CAT",
            });
        }

        // Scan for FORM:XMID chunks inside the CAT.
        // V38: use the INFO-declared count (if available) as the sequence
        // cap, falling back to MAX_SEQUENCES for files without an XDIR header.
        let max_seq = declared_count
            .map(|c| (c as usize).min(MAX_SEQUENCES))
            .unwrap_or(MAX_SEQUENCES);
        let mut sequences = Vec::new();
        let mut pos = offset.saturating_add(12); // skip CAT header

        while pos.saturating_add(12) <= data.len() && sequences.len() < max_seq {
            let chunk_tag = match data.get(pos..pos.saturating_add(4)) {
                Some(t) => t,
                None => break,
            };
            if chunk_tag == FORM_TAG {
                let form_type = match data.get(pos.saturating_add(8)..pos.saturating_add(12)) {
                    Some(t) => t,
                    None => break,
                };
                if form_type == XMID_TAG {
                    let seq = Self::parse_xmid_form(data, pos)?;
                    sequences.push(seq);
                }
            }
            // Read chunk size (IFF big-endian u32) to advance.
            let chunk_size = read_u32_le(data, pos.saturating_add(4))
                .map(u32::from_be)
                .unwrap_or(0) as usize;
            // Advance past this chunk: 8 (tag + size) + body.
            // IFF chunks are padded to even size.
            let body = if chunk_size % 2 == 1 {
                chunk_size.saturating_add(1)
            } else {
                chunk_size
            };
            let next = pos.saturating_add(8).saturating_add(body);
            if next <= pos {
                break; // V38: forward progress guard.
            }
            pos = next;
        }

        if sequences.is_empty() {
            return Err(Error::InvalidMagic {
                context: "XMI: no FORM:XMID sequences found",
            });
        }

        Ok(XmiFile { sequences })
    }

    /// Parses a single `FORM:XMID` chunk starting at `offset`.
    fn parse_xmid_form(data: &'a [u8], offset: usize) -> Result<XmiSequence<'a>, Error> {
        // FORM:XMID header: 12 bytes.
        let body_start = offset.saturating_add(12);
        let form_size = read_u32_le(data, offset.saturating_add(4))
            .map(u32::from_be)
            .unwrap_or(0) as usize;
        let form_end = offset
            .saturating_add(8)
            .saturating_add(form_size)
            .min(data.len());

        let mut timbres = Vec::new();
        let mut event_data: &'a [u8] = &[];
        let mut pos = body_start;

        // Scan chunks within FORM:XMID.
        while pos.saturating_add(8) <= form_end {
            let chunk_tag = match data.get(pos..pos.saturating_add(4)) {
                Some(t) => t,
                None => break,
            };
            let chunk_size = read_u32_le(data, pos.saturating_add(4))
                .map(u32::from_be)
                .unwrap_or(0) as usize;
            let chunk_body_start = pos.saturating_add(8);
            let chunk_body_end = chunk_body_start.saturating_add(chunk_size).min(data.len());

            if chunk_tag == TIMB_TAG {
                timbres = Self::parse_timb(data, chunk_body_start, chunk_size);
            } else if chunk_tag == EVNT_TAG {
                // V38: cap EVNT size.
                if chunk_size > MAX_EVNT_SIZE {
                    return Err(Error::InvalidSize {
                        value: chunk_size,
                        limit: MAX_EVNT_SIZE,
                        context: "XMI EVNT chunk",
                    });
                }
                event_data = data.get(chunk_body_start..chunk_body_end).unwrap_or(&[]);
            }

            // Advance past chunk (with even-byte padding).
            let padded_size = if chunk_size % 2 == 1 {
                chunk_size.saturating_add(1)
            } else {
                chunk_size
            };
            let next = pos.saturating_add(8).saturating_add(padded_size);
            if next <= pos {
                break; // V38: forward progress guard.
            }
            pos = next;
        }

        Ok(XmiSequence {
            timbres,
            event_data,
        })
    }

    /// Parses a TIMB chunk into a list of timbre entries.
    fn parse_timb(data: &[u8], offset: usize, size: usize) -> Vec<XmiTimbre> {
        // TIMB chunk: u16 LE count, then (patch, bank) pairs.
        if size < 2 {
            return Vec::new();
        }
        let count = read_u16_le(data, offset).unwrap_or(0) as usize;
        let mut timbres = Vec::with_capacity(count.min(128));
        let mut pos = offset.saturating_add(2);
        for _ in 0..count {
            let patch = match read_u8(data, pos) {
                Ok(v) => v,
                Err(_) => break,
            };
            pos = pos.saturating_add(1);
            let bank = match read_u8(data, pos) {
                Ok(v) => v,
                Err(_) => break,
            };
            pos = pos.saturating_add(1);
            timbres.push(XmiTimbre { patch, bank });
        }
        timbres
    }

    /// Returns the number of sequences in this XMI file.
    #[inline]
    pub fn sequence_count(&self) -> usize {
        self.sequences.len()
    }
}

#[cfg(test)]
mod tests;
