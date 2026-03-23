// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Red Alert 2 / Yuri's Revenge map file parser (`.map`).
//!
//! RA2 maps are INI-format text files with base64-encoded binary sections
//! for isometric terrain, overlays, and preview images.
//!
//! This module wraps [`crate::ini::IniFile`] and provides typed access
//! to standard RA2 map sections such as `[Basic]`, `[Map]`,
//! `[IsoMapPack5]`, `[OverlayPack]`, `[OverlayDataPack]`, and
//! `[PreviewPack]`.
//!
//! ## Format
//!
//! ```text
//! [Basic]
//! Name=map name
//! Author=author
//!
//! [Map]
//! Size=0,0,128,128
//! LocalSize=2,4,124,120
//! Theater=TEMPERATE
//!
//! [IsoMapPack5]
//! 1=base64data...
//! 2=base64data...
//!
//! [OverlayPack]
//! 1=base64data...
//!
//! [OverlayDataPack]
//! 1=base64data...
//!
//! [PreviewPack]
//! 1=base64data...
//!
//! [Waypoints]
//! 0=cell_number
//! ...
//! ```
//!
//! ## Clean-Room Implementation
//!
//! Implemented from publicly available community documentation of the
//! RA2/YR map format.  No EA-derived code is involved.

use crate::error::Error;
use crate::ini::IniFile;

// ── Constants ────────────────────────────────────────────────────────────────

/// Safety cap: maximum input size in bytes (32 MB).
///
/// RA2 map files with heavy modding can reach several megabytes due to
/// base64-encoded terrain and overlay packs.  32 MB is far beyond any
/// legitimate file.
const MAX_INPUT_SIZE: usize = 32 * 1024 * 1024;

// ── Types ────────────────────────────────────────────────────────────────────

/// Parsed `Size=X,Y,Width,Height` or `LocalSize=X,Y,Width,Height` value
/// from the `[Map]` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapSize {
    /// X origin (typically 0).
    pub x: u32,
    /// Y origin (typically 0).
    pub y: u32,
    /// Map width in cells.
    pub width: u32,
    /// Map height in cells.
    pub height: u32,
}

/// A parsed Red Alert 2 / Yuri's Revenge map file.
///
/// Wraps an [`IniFile`] and exposes typed accessors for the well-known
/// RA2 map sections.
#[derive(Debug)]
pub struct MapRa2File {
    ini: IniFile,
}

impl MapRa2File {
    /// Parses an RA2 map file from a byte slice.
    ///
    /// The input must be valid UTF-8 (or ASCII).  Returns
    /// [`Error::InvalidSize`] if the input exceeds `MAX_INPUT_SIZE`.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        if data.len() > MAX_INPUT_SIZE {
            return Err(Error::InvalidSize {
                value: data.len(),
                limit: MAX_INPUT_SIZE,
                context: "RA2 map file",
            });
        }
        let ini = IniFile::parse(data)?;
        Ok(Self { ini })
    }

    /// Returns a reference to the underlying [`IniFile`].
    #[inline]
    pub fn ini(&self) -> &IniFile {
        &self.ini
    }

    /// Returns the map name from `[Basic] Name`.
    #[inline]
    pub fn name(&self) -> Option<&str> {
        self.ini.get("Basic", "Name")
    }

    /// Returns the author from `[Basic] Author`.
    #[inline]
    pub fn author(&self) -> Option<&str> {
        self.ini.get("Basic", "Author")
    }

    /// Returns the theater from `[Map] Theater`.
    #[inline]
    pub fn theater(&self) -> Option<&str> {
        self.ini.get("Map", "Theater")
    }

    /// Parses `[Map] Size=X,Y,Width,Height` into a [`MapSize`].
    ///
    /// Returns `None` if the `[Map]` section or `Size` key is missing,
    /// or if the value cannot be parsed as four comma-separated integers.
    pub fn size(&self) -> Option<MapSize> {
        parse_map_size(self.ini.get("Map", "Size")?)
    }

    /// Parses `[Map] LocalSize=X,Y,Width,Height` into a [`MapSize`].
    ///
    /// Returns `None` if the `[Map]` section or `LocalSize` key is
    /// missing, or if the value cannot be parsed.
    pub fn local_size(&self) -> Option<MapSize> {
        parse_map_size(self.ini.get("Map", "LocalSize")?)
    }

    /// Concatenates all `[IsoMapPack5]` values in numeric key order.
    ///
    /// Returns `None` if the section is missing or empty.
    pub fn iso_map_pack_raw(&self) -> Option<String> {
        concat_pack_section(&self.ini, "IsoMapPack5")
    }

    /// Concatenates all `[OverlayPack]` values in numeric key order.
    ///
    /// Returns `None` if the section is missing or empty.
    pub fn overlay_pack_raw(&self) -> Option<String> {
        concat_pack_section(&self.ini, "OverlayPack")
    }

    /// Concatenates all `[OverlayDataPack]` values in numeric key order.
    ///
    /// Returns `None` if the section is missing or empty.
    pub fn overlay_data_pack_raw(&self) -> Option<String> {
        concat_pack_section(&self.ini, "OverlayDataPack")
    }

    /// Concatenates all `[PreviewPack]` values in numeric key order.
    ///
    /// Returns `None` if the section is missing or empty.
    pub fn preview_pack_raw(&self) -> Option<String> {
        concat_pack_section(&self.ini, "PreviewPack")
    }

    /// Returns `true` if the given section exists in the map file.
    #[inline]
    pub fn has_section(&self, name: &str) -> bool {
        self.ini.section(name).is_some()
    }

    /// Returns the number of entries in the `[Waypoints]` section.
    ///
    /// Returns `0` if the section is missing.
    #[inline]
    pub fn waypoint_count(&self) -> usize {
        self.ini.section("Waypoints").map_or(0, |s| s.len())
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Parses a `"X,Y,Width,Height"` string into a [`MapSize`].
fn parse_map_size(raw: &str) -> Option<MapSize> {
    let parts: Vec<&str> = raw.split(',').collect();
    if parts.len() != 4 {
        return None;
    }
    Some(MapSize {
        x: parts[0].trim().parse().ok()?,
        y: parts[1].trim().parse().ok()?,
        width: parts[2].trim().parse().ok()?,
        height: parts[3].trim().parse().ok()?,
    })
}

/// Concatenates all values from a section with numeric keys, sorted by
/// key as `u32`.
///
/// RA2 pack sections (`[IsoMapPack5]`, `[OverlayPack]`, etc.) store
/// base64 data split across numbered lines:
///
/// ```text
/// [IsoMapPack5]
/// 1=AAAA...
/// 2=BBBB...
/// ```
///
/// This function collects all entries, sorts them by their numeric key,
/// and concatenates the values into a single string.
fn concat_pack_section(ini: &IniFile, section_name: &str) -> Option<String> {
    let section = ini.section(section_name)?;
    let mut lines: Vec<(&str, &str)> = section.iter().collect();
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

#[cfg(test)]
mod tests;
