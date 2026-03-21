// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! AdLib music parser (`.adl`) — Dune II (1992) soundtrack format.
//!
//! Dune II ADL files are Westwood music containers for Yamaha YM3812 / OPL2
//! playback.  The header indexes multiple sub-songs, track programs, and
//! instrument patches.  The track programs ultimately drive sequential OPL2
//! register writes at replay time, but the file stores indexed Westwood
//! music bytecode rather than a single flat write stream.
//!
//! ## File Layout
//!
//! An ADL file stores multiple sub-songs in a header-indexed structure.
//! The first 120 bytes map sub-song slots to track programs, followed by
//! fixed-size pointer tables for track data and instrument patches.
//!
//! ```text
//! [u8 × 120]   — primary sub-song table (track program indexes, `0xFF` unused)
//! [u16 × 250]  — track program offsets, relative to byte 120
//! [u16 × 250]  — instrument patch offsets (offsets to instrument patch data), relative to byte 120
//! [track data] — Westwood music bytecode
//! [patch data] — 11-byte OPL2 instrument register sets
//! ```
//!
//! ## Instrument Patch Format
//!
//! Each instrument is defined by 11 bytes of OPL2 register values:
//!
//! | Offset | Register      | Purpose                                     |
//! |--------|---------------|---------------------------------------------|
//! | 0      | 0x20 (mod)    | Tremolo / Vibrato / Sustain / KSR / Multi   |
//! | 1      | 0x23 (car)    | Tremolo / Vibrato / Sustain / KSR / Multi   |
//! | 2      | 0x40 (mod)    | Key Scale Level / Output Level              |
//! | 3      | 0x43 (car)    | Key Scale Level / Output Level              |
//! | 4      | 0x60 (mod)    | Attack Rate / Decay Rate                    |
//! | 5      | 0x63 (car)    | Attack Rate / Decay Rate                    |
//! | 6      | 0x80 (mod)    | Sustain Level / Release Rate                |
//! | 7      | 0x83 (car)    | Sustain Level / Release Rate                |
//! | 8      | 0xE0 (mod)    | Waveform Select                             |
//! | 9      | 0xE3 (car)    | Waveform Select                             |
//! | 10     | 0xC0          | Feedback / Connection                       |
//!
//! ## Rendering
//!
//! ADL→WAV rendering requires OPL2 chip emulation.  The only viable pure
//! Rust emulator (`opl-emu`) is GPL-3.0, so rendering lives in `ic-cnc-content`,
//! not here.  This module provides the parser only.
//!
//! ## References
//!
//! - DOSBox source code (OPL emulation and ADL replay logic)
//! - AdPlug project (ADL format documentation and player implementations)
//! - [Dune II music format](https://moddingwiki.shikadi.net/wiki/Westwood_ADL_Format)

use crate::error::Error;
use crate::read::{read_u16_le, read_u8};
use std::fmt;
use std::num::NonZeroU16;

mod dune2;

// ── Constants ────────────────────────────────────────────────────────────────

/// Number of bytes per OPL2 instrument patch definition.
///
/// 11 bytes: 5 register pairs (modulator + carrier) for the characteristic
/// registers (0x20, 0x40, 0x60, 0x80, 0xE0) plus one feedback/connection
/// byte (0xC0).
pub const INSTRUMENT_SIZE: usize = 11;

/// Maximum number of instruments in an ADL file.
///
/// V38: prevents allocation of unreasonably large instrument tables from
/// malformed headers.  Real Dune II ADL files use ≤64 instruments.
const MAX_INSTRUMENTS: usize = 256;

/// Maximum number of sub-songs in an ADL file.
///
/// V38: bounds check for the sub-song count.  Real Dune II ADL files
/// contain ≤50 sub-songs.
const MAX_SUBSONGS: usize = 256;

/// Maximum number of register writes per channel.
///
/// V38: prevents unbounded parsing of register write sequences.
const MAX_REGISTER_WRITES: usize = 1_000_000;

/// Dune II ADL primary sub-song index table length in bytes.
const DUNE2_SUBSONG_INDEX_COUNT: usize = 120;

/// Number of track and instrument pointer entries in the Dune II tables.
const DUNE2_POINTER_COUNT: usize = 250;

/// Size in bytes of one Dune II `u16` pointer table.
const DUNE2_POINTER_TABLE_BYTES: usize = DUNE2_POINTER_COUNT * 2;

/// Start of the Dune II track pointer table.
const DUNE2_TRACK_POINTERS_START: usize = DUNE2_SUBSONG_INDEX_COUNT;

/// Start of the Dune II instrument pointer table.
const DUNE2_INSTRUMENT_POINTERS_START: usize =
    DUNE2_TRACK_POINTERS_START + DUNE2_POINTER_TABLE_BYTES;

/// First valid Dune II data pointer, relative to byte 120.
///
/// The two 250-entry pointer tables occupy 1000 bytes after the 120-byte
/// primary sub-song table, so real track/instrument data starts at 0x03E8.
const DUNE2_DATA_START_REL: usize = DUNE2_POINTER_TABLE_BYTES * 2;

/// Absolute byte offset of the first Dune II track/program payload.
const DUNE2_DATA_START_ABS: usize = DUNE2_SUBSONG_INDEX_COUNT + DUNE2_DATA_START_REL;

/// Sentinel marking an unused primary sub-song slot.
const DUNE2_UNUSED_SUBSONG: u8 = 0xFF;

// ── Types ────────────────────────────────────────────────────────────────────

/// A single OPL2 instrument patch: 11 register values defining the sound
/// of one FM synthesis voice.
///
/// The register layout matches the Yamaha YM3812 operator register set.
/// Bytes 0–1 map to register 0x20 (modulator, carrier), bytes 2–3 to 0x40,
/// and so on.  Byte 10 is the feedback/connection byte (register 0xC0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdlInstrument {
    /// Raw 11-byte OPL2 register values.
    pub registers: [u8; INSTRUMENT_SIZE],
}

/// A single register write in an ADL music data stream.
///
/// Represents one step of the OPL2 replay sequence: write `value` to
/// OPL2 register `register`.  The timing between writes is determined by
/// the sub-song's speed setting and the replay driver's tick rate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdlRegisterWrite {
    /// OPL2 register address (0x00–0xFF).
    pub register: u8,
    /// Value to write to the register.
    pub value: u8,
}

/// Index into the Dune II track-program table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AdlTrackIndex(u8);

impl AdlTrackIndex {
    /// Wraps a raw Dune II track-program index.
    #[inline]
    pub const fn from_raw(value: u8) -> Self {
        Self(value)
    }

    /// Returns the underlying track-program index.
    #[inline]
    pub const fn to_raw(self) -> u8 {
        self.0
    }
}

impl fmt::Display for AdlTrackIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Relative offset into the Dune II post-header data area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AdlDataOffset(u16);

impl AdlDataOffset {
    /// Wraps a raw relative offset.
    #[inline]
    pub const fn from_raw(value: u16) -> Self {
        Self(value)
    }

    /// Returns the underlying relative offset.
    #[inline]
    pub const fn to_raw(self) -> u16 {
        self.0
    }
}

impl fmt::Display for AdlDataOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:04X}", self.0)
    }
}

/// Reference to one indexed Dune II track program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AdlTrackProgramRef {
    /// Entry in the 250-slot track-program table.
    pub index: AdlTrackIndex,
    /// Relative offset from byte 120 into the post-header data area.
    pub offset: AdlDataOffset,
}

/// Sub-song payload representation.
///
/// Flat ADL streams expose decoded register writes directly.  Dune II
/// containers expose a validated indexed track-program reference and keep
/// the underlying Westwood bytecode opaque at this layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdlSubSongData {
    /// Fully decoded register-write channels.
    DecodedChannels {
        /// Register writes grouped by logical OPL2 channel.
        channels: Vec<Vec<AdlRegisterWrite>>,
    },
    /// Validated indexed Dune II track program.
    IndexedTrackProgram {
        /// Which Dune II track-program table entry this sub-song uses.
        program: AdlTrackProgramRef,
    },
}

/// A sub-song within the ADL file.
///
/// Dune II uses multiple sub-songs per ADL file — different music cues
/// (combat, map screen, briefing) are stored as separate sub-songs indexed
/// through the header tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdlSubSong {
    /// Replay speed (ticks per step) when the source format stores one.
    ///
    /// Flat ADL replay streams store an explicit speed word and malformed
    /// inputs may store 0, so the parser clamps that path to a minimum of 1.
    /// Dune II container files do not expose a separate top-level speed
    /// field in the indexed header, so container-backed sub-songs use `None`.
    pub speed: Option<NonZeroU16>,
    /// Decoded register writes or a validated indexed track-program reference.
    pub data: AdlSubSongData,
}

impl AdlSubSong {
    /// Returns the known replay speed, if the source format stores one.
    #[inline]
    pub fn speed_ticks_per_step(&self) -> Option<u16> {
        self.speed.map(NonZeroU16::get)
    }

    /// Returns the number of decoded OPL2 channels carried by this sub-song.
    #[inline]
    pub fn channel_count(&self) -> usize {
        match &self.data {
            AdlSubSongData::DecodedChannels { channels } => channels.len(),
            AdlSubSongData::IndexedTrackProgram { .. } => 0,
        }
    }

    /// Returns the number of decoded register writes carried by this sub-song.
    #[inline]
    pub fn register_write_count(&self) -> usize {
        match &self.data {
            AdlSubSongData::DecodedChannels { channels } => channels.iter().map(|c| c.len()).sum(),
            AdlSubSongData::IndexedTrackProgram { .. } => 0,
        }
    }

    /// Returns decoded channels when this sub-song stores explicit writes.
    #[inline]
    pub fn decoded_channels(&self) -> Option<&[Vec<AdlRegisterWrite>]> {
        match &self.data {
            AdlSubSongData::DecodedChannels { channels } => Some(channels),
            AdlSubSongData::IndexedTrackProgram { .. } => None,
        }
    }

    /// Returns the indexed Dune II track program when the payload is opaque.
    #[inline]
    pub fn track_program(&self) -> Option<AdlTrackProgramRef> {
        match &self.data {
            AdlSubSongData::DecodedChannels { .. } => None,
            AdlSubSongData::IndexedTrackProgram { program } => Some(*program),
        }
    }
}

/// A parsed ADL file containing instrument definitions and sub-songs.
///
/// The parser extracts the instrument table and sub-song structure from
/// the binary data.  Flat replay streams keep their decoded register writes;
/// Dune II containers preserve indexed sub-song boundaries even though the
/// track payload itself is opaque Westwood music bytecode at this layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdlFile {
    /// Instrument patch definitions (OPL2 register sets).
    pub instruments: Vec<AdlInstrument>,
    /// Sub-songs with decoded channel register writes when available.
    pub subsongs: Vec<AdlSubSong>,
}

impl AdlFile {
    /// Parses an ADL file from a byte slice.
    ///
    /// Extracts instrument patches and sub-song structures.  For documented
    /// Dune II container files the parser validates the indexed song/track
    /// layout and exposes one `AdlSubSong` per primary table entry.  For
    /// tiny synthetic or raw replay streams, it falls back to the crate's
    /// flat instrument-table-plus-write-stream parser.
    ///
    /// # Errors
    ///
    /// - [`Error::UnexpectedEof`] — data is truncated
    /// - [`Error::InvalidSize`] — instrument or sub-song count exceeds limits
    /// - [`Error::InvalidOffset`] — a Dune II pointer leaves the input buffer
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        // V38: minimum viable ADL file: at least 2 bytes for first offset.
        if data.len() < 2 {
            return Err(Error::UnexpectedEof {
                needed: 2,
                available: data.len(),
            });
        }

        // Dune II ADL files use a fixed 120-byte primary sub-song table
        // followed by two 250-entry pointer tables.  Detect that container
        // shape first so real soundtrack files expose multiple sub-songs.
        if dune2::looks_like_dune2_container(data) {
            return dune2::parse_dune2_container(data);
        }

        parse_flat_stream(data)
    }

    /// Returns the total number of register writes across all sub-songs.
    pub fn total_register_writes(&self) -> usize {
        self.subsongs
            .iter()
            .map(AdlSubSong::register_write_count)
            .sum()
    }

    /// Estimates the duration in seconds based on register write count and
    /// sub-song speed.
    ///
    /// This is a rough estimate using 560 Hz as the base tick rate (common
    /// in AdLib drivers).  Higher speed values mean slower playback.
    ///
    /// Returns `None` when any sub-song is still an opaque indexed Dune II
    /// track program, because this layer does not know how many decoded
    /// writes or timing ticks the hidden Westwood bytecode will produce.
    pub fn estimated_duration_secs(&self) -> Option<f64> {
        // AdLib driver base tick rate: ~560 Hz (18.2 Hz timer × 31 divider
        // is one common configuration; exact rate varies by game).
        const BASE_TICK_HZ: f64 = 560.0;

        if self
            .subsongs
            .iter()
            .any(|subsong| matches!(&subsong.data, AdlSubSongData::IndexedTrackProgram { .. }))
        {
            return None;
        }

        let total_writes = self.total_register_writes();
        if total_writes == 0 {
            return Some(0.0);
        }

        let speed = self
            .subsongs
            .first()
            .and_then(AdlSubSong::speed_ticks_per_step)?;
        // Each register-write pair is processed every (speed) ticks.
        Some((total_writes as f64) * (f64::from(speed)) / BASE_TICK_HZ)
    }
}

/// Parses the crate's narrow flat replay-stream ADL layout.
fn parse_flat_stream(data: &[u8]) -> Result<AdlFile, Error> {
    // ── Instrument table ─────────────────────────────────────────────
    //
    // This fallback layout is used by synthetic fixtures and raw replay
    // streams: a short `u16` instrument pointer table followed by one flat
    // stream of `(register, value)` pairs.
    let mut instrument_offsets: Vec<u16> = Vec::new();
    let mut pos: usize = 0;

    // Read u16 offsets.  The first offset that equals zero or points
    // to data before the end of the offset table signals the end.
    // We limit to MAX_INSTRUMENTS to prevent V38 unbounded reads.
    loop {
        if instrument_offsets.len() >= MAX_INSTRUMENTS {
            break;
        }
        let offset = match read_u16_le(data, pos) {
            Ok(v) => v,
            Err(_) => break,
        };
        // Heuristic: if offset is 0 or points within the offset table
        // area we've already read, stop.  Also stop if offset is beyond
        // file length.
        if offset == 0 || (offset as usize) < pos.saturating_add(2) {
            // Include this offset if it points to valid instrument data.
            if offset != 0 && (offset as usize).saturating_add(INSTRUMENT_SIZE) <= data.len() {
                instrument_offsets.push(offset);
            }
            // Always advance past the consumed u16 (sentinel or self-ref).
            pos = pos.saturating_add(2);
            break;
        }
        instrument_offsets.push(offset);
        pos = pos.saturating_add(2);
    }

    // Parse instrument patches from the collected offsets.
    let mut instruments = Vec::with_capacity(instrument_offsets.len());
    for &off in &instrument_offsets {
        let start = off as usize;
        let end = start.saturating_add(INSTRUMENT_SIZE);
        let patch_data = data.get(start..end).ok_or(Error::UnexpectedEof {
            needed: end,
            available: data.len(),
        })?;
        let mut registers = [0u8; INSTRUMENT_SIZE];
        registers.copy_from_slice(patch_data);
        instruments.push(AdlInstrument { registers });
    }

    // ── Flat replay stream ───────────────────────────────────────────
    //
    // Advance past instrument patch data and decode the remaining bytes as
    // one raw register-write stream.  This is intentionally narrow and only
    // exists for simple fixtures that do not use the full Dune II header.
    let mut subsongs = Vec::new();
    let instr_data_end = instrument_offsets
        .iter()
        .map(|&off| (off as usize).saturating_add(INSTRUMENT_SIZE))
        .max()
        .unwrap_or(pos);
    pos = pos.max(instr_data_end);

    if pos < data.len() && subsongs.len() < MAX_SUBSONGS {
        // V38: clamp speed to >=1 to prevent division-by-zero in callers.
        let speed = clamped_speed(read_u16_le(data, pos).unwrap_or(0));
        pos = pos.saturating_add(2);

        let mut writes = Vec::new();
        let mut write_count: usize = 0;
        while pos.saturating_add(1) < data.len() && write_count < MAX_REGISTER_WRITES {
            let reg = match read_u8(data, pos) {
                Ok(v) => v,
                Err(_) => break,
            };
            pos = pos.saturating_add(1);
            let val = match read_u8(data, pos) {
                Ok(v) => v,
                Err(_) => break,
            };
            pos = pos.saturating_add(1);
            writes.push(AdlRegisterWrite {
                register: reg,
                value: val,
            });
            write_count = write_count.saturating_add(1);
        }
        subsongs.push(AdlSubSong {
            speed: Some(speed),
            data: AdlSubSongData::DecodedChannels {
                channels: vec![writes],
            },
        });
    }

    Ok(AdlFile {
        instruments,
        subsongs,
    })
}

/// Clamps malformed zero speeds to 1 while preserving a non-zero type.
fn clamped_speed(raw: u16) -> NonZeroU16 {
    match NonZeroU16::new(raw) {
        Some(value) => value,
        None => match NonZeroU16::new(1) {
            Some(value) => value,
            None => unreachable!("1 is non-zero"),
        },
    }
}

#[cfg(test)]
mod tests;
