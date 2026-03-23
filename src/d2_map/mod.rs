// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Dune II scenario/mission parser.
//!
//! Scenario files describe Dune II mission parameters: victory/loss
//! conditions, map seed (terrain is procedurally generated), time limits,
//! and object placements.
//!
//! ## Header Layout
//!
//! ```text
//! [Header]         16 bytes (flags, seed, limits, camera, scale, house)
//! [Placements]     variable (structures, units, reinforcements)
//! ```
//!
//! ## References
//!
//! Format source: Dune Legacy project documentation, CnC-Tools wiki.

use crate::error::Error;
use crate::read::read_u16_le;

/// Size of the fixed scenario header in bytes.
const HEADER_SIZE: usize = 16;

/// Maximum valid map scale value (inclusive).
const MAX_MAP_SCALE: u16 = 2;

/// A Dune II house (faction).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum D2House {
    /// House Harkonnen (index 0).
    Harkonnen,
    /// House Atreides (index 1).
    Atreides,
    /// House Ordos (index 2).
    Ordos,
    /// Fremen allies (index 3).
    Fremen,
    /// Emperor's Sardaukar (index 4).
    Sardaukar,
    /// Mercenary faction (index 5).
    Mercenary,
}

/// Fixed-size header found at the start of every Dune II scenario file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct D2ScenarioHeader {
    /// Conditions that cause mission failure.
    pub lose_flags: u16,
    /// Conditions that cause mission victory.
    pub win_flags: u16,
    /// Seed for procedural terrain generation.
    pub map_seed: u16,
    /// Time limit in game ticks (0 = no limit).
    pub time_limit: u16,
    /// Initial camera X position.
    pub cursor_x: u16,
    /// Initial camera Y position.
    pub cursor_y: u16,
    /// Map scale factor (0, 1, or 2).
    pub map_scale: u16,
    /// Player's house index (see [`D2House`]).
    pub active_house: u16,
}

/// A parsed Dune II scenario file.
///
/// Holds the fixed header and a borrowed slice of the remaining
/// placement data (structures, units, reinforcements).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct D2Scenario<'input> {
    /// The 16-byte fixed header.
    pub header: D2ScenarioHeader,
    /// Raw bytes following the header (placement records).
    placement_data: &'input [u8],
}

impl<'input> D2Scenario<'input> {
    /// Parses a Dune II scenario from a byte slice.
    ///
    /// The input must contain at least `HEADER_SIZE` (16) bytes.  Any bytes
    /// beyond the header are retained as raw placement data accessible via
    /// [`placement_data`](Self::placement_data).
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        if data.len() < HEADER_SIZE {
            return Err(Error::UnexpectedEof {
                needed: HEADER_SIZE,
                available: data.len(),
            });
        }

        let lose_flags = read_u16_le(data, 0)?;
        let win_flags = read_u16_le(data, 2)?;
        let map_seed = read_u16_le(data, 4)?;
        let time_limit = read_u16_le(data, 6)?;
        let cursor_x = read_u16_le(data, 8)?;
        let cursor_y = read_u16_le(data, 10)?;
        let map_scale = read_u16_le(data, 12)?;
        let active_house = read_u16_le(data, 14)?;

        if map_scale > MAX_MAP_SCALE {
            return Err(Error::InvalidSize {
                value: map_scale as usize,
                limit: MAX_MAP_SCALE as usize,
                context: "D2 scenario map scale",
            });
        }

        let header = D2ScenarioHeader {
            lose_flags,
            win_flags,
            map_seed,
            time_limit,
            cursor_x,
            cursor_y,
            map_scale,
            active_house,
        };

        Ok(Self {
            header,
            placement_data: data.get(HEADER_SIZE..).ok_or(Error::UnexpectedEof {
                needed: HEADER_SIZE,
                available: data.len(),
            })?,
        })
    }

    /// Returns the player's house as a [`D2House`] enum, or `None` if the
    /// `active_house` value does not map to a known faction.
    pub fn house(&self) -> Option<D2House> {
        match self.header.active_house {
            0 => Some(D2House::Harkonnen),
            1 => Some(D2House::Atreides),
            2 => Some(D2House::Ordos),
            3 => Some(D2House::Fremen),
            4 => Some(D2House::Sardaukar),
            5 => Some(D2House::Mercenary),
            _ => None,
        }
    }

    /// Returns the raw placement data following the fixed header.
    ///
    /// This slice contains structure, unit, and reinforcement records whose
    /// exact layout varies by game version.
    pub fn placement_data(&self) -> &[u8] {
        self.placement_data
    }
}

#[cfg(test)]
mod tests;
