// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! INI parser for classic C&C rules files (`.ini`).
//!
//! ## Format
//!
//! The original Westwood C&C games (Tiberian Dawn, Red Alert) use `.ini`
//! files for game rules, art definitions, AI configuration, and scenario
//! setup (`rules.ini`, `art.ini`, `ai.ini`, `scen*.ini`).
//!
//! ```text
//! ; This is a comment
//! [SectionName]
//! Key=Value
//! AnotherKey=AnotherValue
//!
//! [AnotherSection]
//! Key=Value
//! ```
//!
//! ## Behaviour
//!
//! - **Case-insensitive** section and key matching (matching original game
//!   behaviour — the Win32 `GetPrivateProfileString` API is case-insensitive).
//! - **Comments** start with `;` and extend to end of line.
//! - **Duplicate sections** are merged: keys from later occurrences override
//!   earlier ones (matching the original game's sequential-read behaviour).
//! - **Duplicate keys** within a section: last value wins.
//! - **Whitespace** around keys and values is trimmed.
//! - **Empty values** (`Key=` with nothing after `=`) are stored as empty strings.
//! - **Lines without `=`** outside a section header are silently ignored
//!   (matching the original game's permissive parsing).
//!
//! ## Clean-Room Implementation
//!
//! Implemented from the publicly documented `.ini` format used across all
//! Westwood games.  The format is a subset of the standard Windows INI
//! format — no EA-derived code is involved.
//!
//! ## References
//!
//! - Win32 `GetPrivateProfileString` API documentation (Microsoft)
//! - Community documentation from the C&C Modding Wiki
//! - Binary analysis of game `.ini` files extracted from `.mix` archives

use std::collections::HashMap;

use crate::error::Error;

// ── Constants ────────────────────────────────────────────────────────────────

/// V38 safety cap: maximum number of sections allowed per INI file.
///
/// Real-world C&C INI files contain at most ~2,000 sections (`rules.ini`).
/// 16,384 is generous while preventing a crafted file from consuming
/// excessive memory (16,384 sections × ~100 bytes overhead ≈ ~1.6 MB).
const MAX_SECTIONS: usize = 16_384;

/// V38 safety cap: maximum number of key-value pairs per section.
///
/// Real-world sections rarely exceed ~200 entries.  4,096 per section is
/// generous while bounding total allocation.
const MAX_KEYS_PER_SECTION: usize = 4_096;

/// V38 safety cap: maximum input size in bytes (16 MB).
///
/// The largest known C&C INI file (`rules.ini` from Yuri's Revenge mods)
/// is under 1 MB.  16 MB is far beyond any legitimate file.
const MAX_INPUT_SIZE: usize = 16 * 1024 * 1024;

// ── Types ────────────────────────────────────────────────────────────────────

/// A single section in an INI file, containing ordered key-value pairs.
///
/// Keys are case-insensitive for lookup but preserve their original casing.
#[derive(Debug, Clone)]
pub struct IniSection {
    /// The section name as it appeared in the file (original casing).
    name: String,
    /// Key-value pairs in insertion order (last-write-wins for duplicates).
    /// The HashMap maps lowercased keys to (original_key, value) pairs.
    entries: HashMap<String, (String, String)>,
    /// Insertion-order key list (lowercased) for deterministic iteration.
    order: Vec<String>,
}

impl IniSection {
    /// Returns the section name (original casing).
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Looks up a value by key (case-insensitive).
    ///
    /// Returns `None` if the key does not exist in this section.
    #[inline]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .get(&key.to_ascii_lowercase())
            .map(|(_, v)| v.as_str())
    }

    /// Returns an iterator over `(key, value)` pairs in insertion order.
    ///
    /// Keys are returned in their original casing.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.order.iter().filter_map(move |lower_key| {
            self.entries
                .get(lower_key)
                .map(|(k, v)| (k.as_str(), v.as_str()))
        })
    }

    /// Returns the number of key-value pairs in this section.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if this section has no key-value pairs.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            entries: HashMap::new(),
            order: Vec::new(),
        }
    }

    fn insert(&mut self, key: &str, value: &str) -> Result<(), Error> {
        // V38: bound the number of keys per section.
        let lower_key = key.to_ascii_lowercase();
        if !self.entries.contains_key(&lower_key) {
            if self.entries.len() >= MAX_KEYS_PER_SECTION {
                return Err(Error::InvalidSize {
                    value: self.entries.len().saturating_add(1),
                    limit: MAX_KEYS_PER_SECTION,
                    context: "INI keys per section",
                });
            }
            self.order.push(lower_key.clone());
        }
        self.entries
            .insert(lower_key, (key.to_string(), value.to_string()));
        Ok(())
    }
}

/// A parsed INI file with case-insensitive section and key lookup.
///
/// ## Example
///
/// ```
/// use cnc_formats::ini::IniFile;
///
/// let input = b"[General]\nName=test\nSpeed=5\n";
/// let ini = IniFile::parse(input).unwrap();
/// assert_eq!(ini.get("general", "name"), Some("test"));
/// assert_eq!(ini.get("General", "Speed"), Some("5"));
/// ```
#[derive(Debug, Clone)]
pub struct IniFile {
    /// Sections keyed by lowercased name, mapped to (insertion_index, section).
    sections: HashMap<String, (usize, IniSection)>,
    /// Insertion-order section name list (lowercased) for deterministic iteration.
    section_order: Vec<String>,
}

impl IniFile {
    /// Parses an INI file from a byte slice.
    ///
    /// The input is interpreted as UTF-8 (or ASCII, which is a subset).
    /// Invalid UTF-8 sequences produce an `InvalidMagic` error.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        // V38: reject oversized input.
        if data.len() > MAX_INPUT_SIZE {
            return Err(Error::InvalidSize {
                value: data.len(),
                limit: MAX_INPUT_SIZE,
                context: "INI file size",
            });
        }

        let text = std::str::from_utf8(data).map_err(|_| Error::InvalidMagic {
            context: "INI file (invalid UTF-8)",
        })?;

        Self::parse_str(text)
    }

    /// Parses an INI file from a string slice.
    pub fn parse_str(text: &str) -> Result<Self, Error> {
        // V38: reject oversized input.
        if text.len() > MAX_INPUT_SIZE {
            return Err(Error::InvalidSize {
                value: text.len(),
                limit: MAX_INPUT_SIZE,
                context: "INI file size",
            });
        }

        let mut file = IniFile {
            sections: HashMap::new(),
            section_order: Vec::new(),
        };

        let mut current_section: Option<String> = None;

        for line in text.lines() {
            // Strip comments: everything from the first `;` onward is ignored.
            let line = match line.find(';') {
                Some(pos) => &line[..pos],
                None => line,
            };
            let line = line.trim();

            if line.is_empty() {
                continue;
            }

            // Section header: [SectionName]
            if line.starts_with('[') {
                if let Some(end) = line.find(']') {
                    let name = line[1..end].trim();
                    let lower_name = name.to_ascii_lowercase();

                    if !file.sections.contains_key(&lower_name) {
                        // V38: bound the number of sections.
                        if file.sections.len() >= MAX_SECTIONS {
                            return Err(Error::InvalidSize {
                                value: file.sections.len().saturating_add(1),
                                limit: MAX_SECTIONS,
                                context: "INI section count",
                            });
                        }
                        let idx = file.section_order.len();
                        file.section_order.push(lower_name.clone());
                        file.sections
                            .insert(lower_name.clone(), (idx, IniSection::new(name)));
                    }
                    current_section = Some(lower_name);
                }
                continue;
            }

            // Key=Value pair (only valid inside a section).
            if let Some(eq_pos) = line.find('=') {
                if let Some(ref section_key) = current_section {
                    let key = line[..eq_pos].trim();
                    let value = line[eq_pos + 1..].trim();

                    if !key.is_empty() {
                        if let Some((_, section)) = file.sections.get_mut(section_key) {
                            section.insert(key, value)?;
                        }
                    }
                }
            }
            // Lines without `=` outside a section header are silently ignored.
        }

        Ok(file)
    }

    /// Looks up a value by section and key (both case-insensitive).
    ///
    /// Returns `None` if the section or key does not exist.
    #[inline]
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .get(&section.to_ascii_lowercase())
            .and_then(|(_, s)| s.get(key))
    }

    /// Returns a reference to a section by name (case-insensitive).
    ///
    /// Returns `None` if the section does not exist.
    #[inline]
    pub fn section(&self, name: &str) -> Option<&IniSection> {
        self.sections
            .get(&name.to_ascii_lowercase())
            .map(|(_, s)| s)
    }

    /// Returns an iterator over all sections in insertion order.
    pub fn sections(&self) -> impl Iterator<Item = &IniSection> {
        self.section_order
            .iter()
            .filter_map(move |key| self.sections.get(key).map(|(_, s)| s))
    }

    /// Returns the number of sections.
    #[inline]
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }
}

#[cfg(test)]
mod tests;
