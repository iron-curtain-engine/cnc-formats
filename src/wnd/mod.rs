// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

//! Generals / Zero Hour WND UI layout parser (`.wnd`).
//!
//! WND files describe hierarchical UI window layouts for the SAGE engine.
//! Each window element has properties (type, name, rect, callbacks) and
//! may contain child windows.
//!
//! ## File Layout
//!
//! ```text
//! FILE_VERSION = 1;
//! STARTLAYOUTBLOCK
//!   LAYOUTINIT = ...;
//! ENDLAYOUTBLOCK
//! WINDOW
//!   WINDOWTYPE = USER;
//!   NAME = "Root";
//!   CHILD
//!   WINDOW
//!     NAME = "Child1";
//!   END
//!   CHILD
//!   WINDOW
//!     NAME = "Child2";
//!   END
//!   ENDALLCHILDREN
//! END
//! ```
//!
//! ## Parsing Strategy
//!
//! Properties are stored as raw key-value string pairs. Complex values
//! (e.g. `SCREENRECT` with sub-fields) are kept as-is for the engine
//! layer to interpret. The parser handles WINDOW/END block structure
//! with CHILD (per-child prefix) and ENDALLCHILDREN (group terminator).
//!
//! ## References
//!
//! Format source: Generals modding community documentation, OpenSAGE project source analysis.

use crate::error::Error;

// ── Constants ────────────────────────────────────────────────────────────────

/// Safety cap: maximum input size in bytes (16 MB).
///
/// WND files are text-based UI layouts; even heavily modded files are
/// well under 1 MB. 16 MB prevents unbounded allocation.
const MAX_INPUT_SIZE: usize = 16 * 1024 * 1024;

/// Safety cap: maximum WINDOW nesting depth.
///
/// Real WND files rarely exceed 5-6 levels of nesting. 64 levels is
/// generous while preventing stack overflow from deeply recursive input.
const MAX_DEPTH: usize = 64;

/// Safety cap: maximum total number of WINDOW elements in a file.
///
/// Prevents unbounded allocation from crafted input with thousands of
/// window definitions.
const MAX_WINDOWS: usize = 16_384;

// ── Types ────────────────────────────────────────────────────────────────────

/// A single window element in a WND UI layout.
///
/// Each window has an ordered list of properties (key-value string pairs)
/// and may contain child windows nested via CHILD/ENDCHILD blocks.
#[derive(Debug, Clone)]
pub struct WndWindow {
    /// Property key-value pairs in file order.
    pub properties: Vec<(String, String)>,
    /// Child windows (from CHILD/WINDOW…END/ENDALLCHILDREN blocks).
    pub children: Vec<WndWindow>,
}

impl WndWindow {
    /// Get a property value by key (case-insensitive).
    ///
    /// Returns the first matching property's value, or `None` if the
    /// key does not exist.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    /// Get the WINDOWTYPE property.
    pub fn window_type(&self) -> Option<&str> {
        self.get("WINDOWTYPE")
    }

    /// Get the NAME property.
    pub fn name(&self) -> Option<&str> {
        self.get("NAME")
    }
}

/// A parsed WND UI layout file.
///
/// Contains the file version (if present) and the top-level window
/// elements. Most WND files have a single top-level window, but the
/// parser supports multiple.
///
/// ## Example
///
/// ```
/// use cnc_formats::wnd::WndFile;
///
/// let input = b"FILE_VERSION = 1;\nWINDOW\n  WINDOWTYPE = USER;\n  NAME = \"Root\";\nEND\n";
/// let wnd = WndFile::parse(input).unwrap();
/// assert_eq!(wnd.version, Some(1));
/// assert_eq!(wnd.windows[0].name(), Some("\"Root\""));
/// ```
#[derive(Debug)]
pub struct WndFile {
    /// The FILE_VERSION value, if present at the top of the file.
    pub version: Option<u32>,
    /// Top-level window elements.
    pub windows: Vec<WndWindow>,
}

impl WndFile {
    /// Parses a WND file from a byte slice.
    ///
    /// The input is interpreted as UTF-8 (or ASCII). Invalid UTF-8
    /// produces an `InvalidMagic` error.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        // Reject oversized input.
        if data.len() > MAX_INPUT_SIZE {
            return Err(Error::InvalidSize {
                value: data.len(),
                limit: MAX_INPUT_SIZE,
                context: "WND file size",
            });
        }

        let text = std::str::from_utf8(data).map_err(|_| Error::InvalidMagic {
            context: "WND file (invalid UTF-8)",
        })?;

        Self::parse_str(text)
    }

    /// Parses a WND file from a string slice.
    pub fn parse_str(text: &str) -> Result<Self, Error> {
        if text.len() > MAX_INPUT_SIZE {
            return Err(Error::InvalidSize {
                value: text.len(),
                limit: MAX_INPUT_SIZE,
                context: "WND file size",
            });
        }

        let lines = preprocess(text);
        let mut pos = 0;
        let mut version: Option<u32> = None;
        let mut windows = Vec::new();
        let mut total_windows: usize = 0;

        // Look for FILE_VERSION at the top (before any WINDOW block).
        if let Some(line) = lines.get(pos) {
            if let Some(v) = parse_file_version(line) {
                version = Some(v);
                pos += 1;
            }
        }

        // Parse top-level WINDOW blocks.
        while pos < lines.len() {
            let line = lines.get(pos).copied().unwrap_or("").trim();
            if line.eq_ignore_ascii_case("WINDOW") {
                pos += 1;
                let window = parse_window(&lines, &mut pos, 0, &mut total_windows)?;
                windows.push(window);
            } else {
                // Skip unrecognized top-level lines (e.g. extra
                // FILE_VERSION-like lines or blank content).
                pos += 1;
            }
        }

        Ok(WndFile { version, windows })
    }

    /// Returns the total number of windows in the file, including all
    /// nested children.
    pub fn window_count(&self) -> usize {
        fn count(windows: &[WndWindow]) -> usize {
            windows.iter().map(|w| 1 + count(&w.children)).sum()
        }
        count(&self.windows)
    }
}

// ── Private helpers ──────────────────────────────────────────────────────────

/// Preprocesses WND text into non-empty, non-comment lines.
///
/// Lines that start with `;` (after trimming) are treated as comments and
/// excluded. All lines are trimmed of leading/trailing whitespace.
fn preprocess(text: &str) -> Vec<&str> {
    text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with(';'))
        .collect()
}

/// Tries to parse `FILE_VERSION = N;` from a line.
fn parse_file_version(line: &str) -> Option<u32> {
    let line = line.trim();
    // Strip trailing semicolon if present.
    let line = line.strip_suffix(';').unwrap_or(line).trim();
    // Split on `=`.
    let (key, value) = line.split_once('=')?;
    if !key.trim().eq_ignore_ascii_case("FILE_VERSION") {
        return None;
    }
    value.trim().parse::<u32>().ok()
}

/// Parses a WINDOW block (after the opening `WINDOW` keyword has been consumed).
///
/// Reads lines from `lines[*pos..]`, advancing `*pos` past the closing `END`.
fn parse_window(
    lines: &[&str],
    pos: &mut usize,
    depth: usize,
    total_windows: &mut usize,
) -> Result<WndWindow, Error> {
    // Depth check.
    if depth >= MAX_DEPTH {
        return Err(Error::InvalidSize {
            value: depth.saturating_add(1),
            limit: MAX_DEPTH,
            context: "WND nesting depth",
        });
    }

    // Window count check.
    *total_windows = total_windows.saturating_add(1);
    if *total_windows > MAX_WINDOWS {
        return Err(Error::InvalidSize {
            value: *total_windows,
            limit: MAX_WINDOWS,
            context: "WND window count",
        });
    }

    let mut properties = Vec::new();
    let mut children = Vec::new();
    // Set when a CHILD keyword is seen; the next WINDOW starts a child block.
    let mut awaiting_child_window = false;

    while *pos < lines.len() {
        let line = lines.get(*pos).copied().unwrap_or("");

        // END closes the current WINDOW block.
        if line.eq_ignore_ascii_case("END") {
            *pos += 1;
            return Ok(WndWindow {
                properties,
                children,
            });
        }

        // ENDALLCHILDREN signals the end of the children group; no-op in parsing
        // because children have already been accumulated one by one.
        if line.eq_ignore_ascii_case("ENDALLCHILDREN") {
            awaiting_child_window = false;
            *pos += 1;
            continue;
        }

        // ENDCHILD is not valid in real Generals WND files.
        if line.eq_ignore_ascii_case("ENDCHILD") {
            return Err(Error::InvalidMagic {
                context: "WND window block (unexpected ENDCHILD)",
            });
        }

        // CHILD precedes each individual child WINDOW.
        if line.eq_ignore_ascii_case("CHILD") {
            if awaiting_child_window {
                return Err(Error::InvalidMagic {
                    context: "WND window block (nested CHILD without WINDOW)",
                });
            }
            awaiting_child_window = true;
            *pos += 1;
            continue;
        }

        // WINDOW following a CHILD keyword starts a child block.
        if line.eq_ignore_ascii_case("WINDOW") {
            if !awaiting_child_window {
                return Err(Error::InvalidMagic {
                    context: "WND window block (nested WINDOW without CHILD)",
                });
            }
            awaiting_child_window = false;
            *pos += 1;
            let child = parse_window(lines, pos, depth + 1, total_windows)?;
            children.push(child);
            continue;
        }

        // Property line: KEY = VALUE;
        if let Some((key, value)) = parse_property(line) {
            properties.push((key.to_string(), value.to_string()));
        }
        // Unrecognized lines are silently skipped.

        *pos += 1;
    }

    // Reached end of input without finding END.
    Err(Error::InvalidMagic {
        context: "WND window block (missing END)",
    })
}

/// Parses a `KEY = VALUE;` property line.
///
/// Returns `(key, value)` with the trailing semicolon stripped from the
/// value. If no `=` is found, returns `None`.
fn parse_property(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    let value = value.trim();
    // Strip trailing semicolon.
    let value = value.strip_suffix(';').unwrap_or(value).trim();
    Some((key, value))
}

#[cfg(test)]
mod tests;
