// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! TD / RA1 map package parser (`.mpr`, `.ini`).
//!
//! MPR files are INI-format text with embedded base64-encoded binary
//! sections for terrain (`[MapPack]`) and overlays (`[OverlayPack]`).
//!
//! This module wraps `crate::ini::IniFile` and provides typed access
//! to standard map sections.
//!
//! ## File Layout
//!
//! ```text
//! [Basic]
//! Name=mission name
//! Player=GoodGuy
//!
//! [Map]
//! Theater=TEMPERATE
//! X=1
//! Y=4
//! Width=62
//! Height=56
//!
//! [MapPack]
//! 1=base64data...
//! 2=base64data...
//!
//! [OverlayPack]
//! 1=base64data...
//!
//! [Waypoints]
//! 0=cell_number
//! ...
//! ```

use crate::error::Error;
use crate::ini::IniFile;

/// Safety cap: maximum input size in bytes (16 MB).
const MAX_INPUT_SIZE: usize = 16 * 1024 * 1024;

/// Parsed map bounds from the `[Map]` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapBounds {
    /// X origin of the playable area.
    pub x: u32,
    /// Y origin of the playable area.
    pub y: u32,
    /// Width of the playable area in cells.
    pub width: u32,
    /// Height of the playable area in cells.
    pub height: u32,
}

/// A parsed MPR / mission-package file.
///
/// Wraps an [`IniFile`] and provides typed accessors for the standard
/// map sections (`[Basic]`, `[Map]`, `[MapPack]`, `[OverlayPack]`).
#[derive(Debug)]
pub struct MprFile {
    ini: IniFile,
}

impl MprFile {
    /// Parses an MPR file from a byte slice.
    ///
    /// The input is interpreted as INI-format text.  Returns an error if
    /// the input exceeds `MAX_INPUT_SIZE` or is not valid UTF-8.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        if data.len() > MAX_INPUT_SIZE {
            return Err(Error::InvalidSize {
                value: data.len(),
                limit: MAX_INPUT_SIZE,
                context: "MPR file",
            });
        }
        let ini = IniFile::parse(data)?;
        Ok(Self { ini })
    }

    /// Returns the underlying INI file for full access.
    pub fn ini(&self) -> &IniFile {
        &self.ini
    }

    /// Get the map name from `[Basic] Name=`.
    pub fn name(&self) -> Option<&str> {
        self.ini.get("Basic", "Name")
    }

    /// Get the theater from `[Map] Theater=`.
    pub fn theater(&self) -> Option<&str> {
        self.ini.get("Map", "Theater")
    }

    /// Parse `[Map]` section for bounds (`X`, `Y`, `Width`, `Height`).
    ///
    /// Returns `None` if the section or any of the four keys is missing
    /// or cannot be parsed as a `u32`.
    pub fn bounds(&self) -> Option<MapBounds> {
        let x = self.ini.get("Map", "X")?.parse::<u32>().ok()?;
        let y = self.ini.get("Map", "Y")?.parse::<u32>().ok()?;
        let w = self.ini.get("Map", "Width")?.parse::<u32>().ok()?;
        let h = self.ini.get("Map", "Height")?.parse::<u32>().ok()?;
        Some(MapBounds {
            x,
            y,
            width: w,
            height: h,
        })
    }

    /// Concatenate all `[MapPack]` values in numeric key order into a
    /// single base64 string.
    ///
    /// Returns `None` if the section does not exist or is empty.
    pub fn map_pack_raw(&self) -> Option<String> {
        Self::concat_pack_section(&self.ini, "MapPack")
    }

    /// Concatenate all `[OverlayPack]` values in numeric key order into
    /// a single base64 string.
    ///
    /// Returns `None` if the section does not exist or is empty.
    pub fn overlay_pack_raw(&self) -> Option<String> {
        Self::concat_pack_section(&self.ini, "OverlayPack")
    }

    /// Check whether a section exists (e.g. `"Terrain"`, `"Units"`, etc.).
    pub fn has_section(&self, name: &str) -> bool {
        self.ini.section(name).is_some()
    }

    // ── Internal helpers ────────────────────────────────────────────────

    /// Concatenates all values from a numbered-key section in numeric
    /// key order.
    fn concat_pack_section(ini: &IniFile, section_name: &str) -> Option<String> {
        let section = ini.section(section_name)?;
        let mut lines: Vec<(&str, &str)> = section.iter().collect();
        // Sort by numeric key so line 1 comes before line 2, etc.
        lines.sort_by(|a, b| {
            a.0.parse::<u32>()
                .unwrap_or(u32::MAX)
                .cmp(&b.0.parse::<u32>().unwrap_or(u32::MAX))
        });
        let combined: String = lines.iter().map(|(_, v)| *v).collect();
        if combined.is_empty() {
            None
        } else {
            Some(combined)
        }
    }
}

#[cfg(test)]
mod tests;
