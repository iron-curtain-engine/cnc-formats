// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Generals / Zero Hour string table parser (`.str`).
//!
//! STR files are text-based localization tables used by the SAGE engine.
//! Each entry is a triplet: an identifier line, a quoted value line, and
//! an `END` keyword.
//!
//! ## File Layout
//!
//! ```text
//! ; comment
//! IDENTIFIER
//! "Localized string"
//! END
//! ```

use crate::error::Error;

/// Maximum number of string entries permitted in a single STR file.
const MAX_ENTRIES: usize = 100_000;

/// Maximum input size in bytes (16 MiB).
const MAX_INPUT_SIZE: usize = 16 * 1024 * 1024;

/// One entry in a SAGE string table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrEntry {
    /// The string identifier (e.g. `GUI:SomeButton`).
    pub id: String,
    /// The localized string value (quotes stripped).
    pub value: String,
}

/// A parsed SAGE engine `.str` string table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrFile {
    entries: Vec<StrEntry>,
}

impl StrFile {
    /// Parses a SAGE `.str` string table from a byte slice.
    ///
    /// The input must be valid UTF-8. Comment lines (starting with `;`) and
    /// blank lines are skipped. Each string entry consists of three lines:
    /// an identifier, a double-quoted value, and the keyword `END`.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        if data.len() > MAX_INPUT_SIZE {
            return Err(Error::InvalidSize {
                value: data.len(),
                limit: MAX_INPUT_SIZE,
                context: "STR input size",
            });
        }

        let text = std::str::from_utf8(data).map_err(|_| Error::InvalidMagic {
            context: "STR file encoding (expected UTF-8)",
        })?;

        let mut lines = text.lines().peekable();
        let mut entries = Vec::new();

        while let Some(line) = lines.next() {
            let trimmed = line.trim();

            // Skip blank lines and comment lines.
            if trimmed.is_empty() || trimmed.starts_with(';') {
                continue;
            }

            // This line should be the identifier.
            let id = trimmed.to_string();

            // Read the quoted value line.
            let value_line = loop {
                let raw = lines.next().ok_or(Error::UnexpectedEof {
                    needed: 1,
                    available: 0,
                })?;
                let t = raw.trim();
                if t.is_empty() || t.starts_with(';') {
                    continue;
                }
                break t;
            };

            if !value_line.starts_with('"') || !value_line.ends_with('"') || value_line.len() < 2 {
                return Err(Error::InvalidMagic {
                    context: "STR string entry (value must be double-quoted)",
                });
            }
            let value = value_line[1..value_line.len() - 1].to_string();

            // Read the END keyword.
            let end_line = loop {
                let raw = lines.next().ok_or(Error::UnexpectedEof {
                    needed: 1,
                    available: 0,
                })?;
                let t = raw.trim();
                if t.is_empty() || t.starts_with(';') {
                    continue;
                }
                break t;
            };

            if !end_line.eq_ignore_ascii_case("END") {
                return Err(Error::InvalidMagic {
                    context: "STR expected END",
                });
            }

            if entries.len() >= MAX_ENTRIES {
                return Err(Error::InvalidSize {
                    value: entries.len() + 1,
                    limit: MAX_ENTRIES,
                    context: "STR entry count",
                });
            }

            entries.push(StrEntry { id, value });
        }

        Ok(Self { entries })
    }

    /// Returns a slice of all parsed entries.
    pub fn entries(&self) -> &[StrEntry] {
        &self.entries
    }

    /// Looks up a string value by identifier (case-insensitive).
    pub fn get(&self, id: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.id.eq_ignore_ascii_case(id))
            .map(|e| e.value.as_str())
    }

    /// Returns the number of entries in the string table.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the string table contains no entries.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests;
