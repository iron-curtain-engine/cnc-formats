// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! MiniYAML parser for OpenRA-format configuration files.
//!
//! ## Format
//!
//! MiniYAML is the configuration format used by OpenRA and its community mods.
//! It is a simplified YAML-like format with tab-based indentation:
//!
//! ```text
//! # This is a comment
//! RootNode:
//!     ChildKey: ChildValue          (indented by one tab)
//!     AnotherChild:                 (indented by one tab)
//!         GrandChild: DeepValue     (indented by two tabs)
//! Inherits: @parent
//! ```
//!
//! Note: indentation shown as spaces for readability; actual MiniYAML files
//! use **tab characters** exclusively.  The parser rejects space indentation.
//!
//! ## Key differences from standard YAML
//!
//! - **Tab indentation only** (not spaces) — each tab level is one nesting depth.
//! - **`Key: Value`** syntax with a colon-space separator (or bare `Key:` for
//!   nodes with only children and no value).
//! - **`#` comments** — everything from the first unescaped `#` to end of line
//!   is ignored.  Use `\#` to embed a literal `#` in a value.
//! - **`Inherits: @parent`** — inheritance declarations reference named templates.
//! - **`^` removal markers** — `^NodeName` removes an inherited child node.
//! - **`-Key:` removal prefix** — removes a key during merge (engine-level).
//! - **No multi-line strings, no anchors, no flow syntax** — MiniYAML is
//!   strictly line-oriented.
//!
//! ## Known Limitations
//!
//! - **Tab-only indentation:** OpenRA's C# parser accepts both tabs and spaces
//!   (treating 4 spaces ≈ 1 tab).  This parser is tab-only.  Files that use
//!   space indentation are rejected with an error.
//! - **Guarded whitespace:** OpenRA uses leading/trailing `\` to preserve
//!   significant whitespace in values.  This is not currently implemented.
//!
//! ## Clean-Room Implementation
//!
//! Implemented from public OpenRA documentation and community format descriptions.
//! No code derived from OpenRA's GPL-licensed C# MiniYAML parser.
//!
//! ## References
//!
//! - OpenRA wiki: MiniYAML format documentation
//! - OpenRA mod SDK examples
//! - D025 (Runtime MiniYAML Loading) in the Iron Curtain design docs

use crate::error::Error;

// ── Constants ────────────────────────────────────────────────────────────────

/// V38 safety cap: maximum nesting depth.
///
/// Real-world MiniYAML files rarely exceed 5–6 levels.  64 is generous.
const MAX_DEPTH: usize = 64;

/// V38 safety cap: maximum number of nodes in a single file.
///
/// Real-world OpenRA mod files contain at most ~10,000 nodes.
/// 100,000 is generous while bounding total memory.
const MAX_NODES: usize = 100_000;

/// V38 safety cap: maximum input size in bytes (16 MB).
const MAX_INPUT_SIZE: usize = 16 * 1024 * 1024;

// ── Types ────────────────────────────────────────────────────────────────────

/// A single node in a MiniYAML document tree.
///
/// Each node has a key, an optional value, and zero or more child nodes.
/// The tree structure mirrors the indentation in the source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiniYamlNode {
    /// The node key (left side of `Key: Value`, or `Key:` for value-less nodes).
    key: String,
    /// The optional value (right side of `Key: Value`; `None` if bare `Key:`).
    value: Option<String>,
    /// Child nodes (one indentation level deeper).
    children: Vec<MiniYamlNode>,
}

impl MiniYamlNode {
    /// Returns the node key.
    #[inline]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Returns the node value, if present.
    #[inline]
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }

    /// Returns a slice of child nodes.
    #[inline]
    pub fn children(&self) -> &[MiniYamlNode] {
        &self.children
    }

    /// Looks up a direct child node by key (case-sensitive, matching OpenRA).
    pub fn child(&self, key: &str) -> Option<&MiniYamlNode> {
        self.children.iter().find(|c| c.key == key)
    }
}

/// A parsed MiniYAML document — a forest of top-level nodes.
///
/// ## Example
///
/// ```
/// use cnc_formats::miniyaml::MiniYamlDoc;
///
/// let input = "RootNode:\n\tChildKey: ChildValue\nOther: 42\n";
/// let doc = MiniYamlDoc::parse_str(input).unwrap();
/// assert_eq!(doc.nodes().len(), 2);
/// assert_eq!(doc.node("Other").unwrap().value(), Some("42"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiniYamlDoc {
    /// Top-level nodes (zero indentation).
    nodes: Vec<MiniYamlNode>,
}

impl MiniYamlDoc {
    /// Parses a MiniYAML document from a byte slice.
    ///
    /// The input is interpreted as UTF-8.  Invalid UTF-8 produces an
    /// `InvalidMagic` error.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        // V38: reject oversized input.
        if data.len() > MAX_INPUT_SIZE {
            return Err(Error::InvalidSize {
                value: data.len(),
                limit: MAX_INPUT_SIZE,
                context: "MiniYAML file size",
            });
        }

        let text = std::str::from_utf8(data).map_err(|_| Error::InvalidMagic {
            context: "MiniYAML file (invalid UTF-8)",
        })?;

        Self::parse_str(text)
    }

    /// Parses a MiniYAML document from a string slice.
    pub fn parse_str(text: &str) -> Result<Self, Error> {
        // V38: reject oversized input.
        if text.len() > MAX_INPUT_SIZE {
            return Err(Error::InvalidSize {
                value: text.len(),
                limit: MAX_INPUT_SIZE,
                context: "MiniYAML file size",
            });
        }

        // Parse all lines into (indent_level, key, value) triples.
        let mut entries: Vec<(usize, String, Option<String>)> = Vec::new();

        for line in text.lines() {
            // Strip comments: everything from the first unescaped `#` onward.
            // MiniYAML uses `\#` as an escape for literal `#` in values.
            let line = match find_unescaped_hash(line) {
                Some(pos) => line.get(..pos).unwrap_or(line),
                None => line,
            };

            // Count leading tabs (indentation level).
            let indent = line.bytes().take_while(|&b| b == b'\t').count();

            let content = line.get(indent..).unwrap_or("").trim();
            if content.is_empty() {
                continue;
            }

            // Reject space indentation — MiniYAML uses tabs only.
            // Space-indented content would be silently misparsed (all lines
            // treated as depth 0) which is worse than a clear error.
            // Only checked on lines with non-empty content — blank lines
            // consisting solely of whitespace are harmless and skipped above.
            if indent == 0 && line.starts_with(' ') {
                return Err(Error::InvalidMagic {
                    context: "MiniYAML: space indentation detected (use tabs)",
                });
            }

            // V38: reject excessive nesting.
            if indent >= MAX_DEPTH {
                return Err(Error::InvalidSize {
                    value: indent,
                    limit: MAX_DEPTH,
                    context: "MiniYAML nesting depth",
                });
            }

            // V38: bound total node count.
            if entries.len() >= MAX_NODES {
                return Err(Error::InvalidSize {
                    value: entries.len().saturating_add(1),
                    limit: MAX_NODES,
                    context: "MiniYAML node count",
                });
            }

            // Parse `Key: Value` or bare `Key:` or `Key` (no colon).
            // After comment stripping, unescape `\#` → `#` in values.
            let (key, value) = if let Some(colon_pos) = content.find(':') {
                let k = content.get(..colon_pos).unwrap_or("").trim();
                let v = content.get(colon_pos + 1..).unwrap_or("").trim();
                if v.is_empty() {
                    (k.to_string(), None)
                } else {
                    (k.to_string(), Some(unescape_hash(v)))
                }
            } else {
                // Bare key without colon (e.g., removal markers like `^NodeName`).
                (content.to_string(), None)
            };

            entries.push((indent, key, value));
        }

        // Build the tree from the flat (indent, key, value) list.
        // Entries are consumed (via `mem::take`) to avoid cloning every string.
        let len = entries.len();
        let nodes = build_tree(&mut entries, 0, 0, len)?;
        Ok(MiniYamlDoc { nodes })
    }

    /// Returns the top-level nodes.
    #[inline]
    pub fn nodes(&self) -> &[MiniYamlNode] {
        &self.nodes
    }

    /// Looks up a top-level node by key.
    pub fn node(&self, key: &str) -> Option<&MiniYamlNode> {
        self.nodes.iter().find(|n| n.key == key)
    }
}

/// Finds the byte position of the first unescaped `#` in a line.
///
/// MiniYAML uses `\#` as an escape sequence for literal `#` in values.
/// Returns the position of the first `#` not immediately preceded by `\`.
fn find_unescaped_hash(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    (0..bytes.len()).find(|&i| {
        bytes.get(i) == Some(&b'#') && (i == 0 || bytes.get(i.wrapping_sub(1)) != Some(&b'\\'))
    })
}

/// Unescapes `\#` → `#` in a MiniYAML value string.
///
/// After comment stripping, any `\#` remaining in the value represents
/// a literal `#` character.  The backslash escape is removed.
///
/// Avoids the `replace` allocation when no `\#` is present, which is
/// the common case — most values contain no escaped hashes.
#[inline]
fn unescape_hash(value: &str) -> String {
    if value.contains("\\#") {
        value.replace("\\#", "#")
    } else {
        value.to_string()
    }
}

/// Recursively builds a tree of nodes from the flat entry list.
///
/// `expected_indent` is the indentation level of nodes at this depth.
/// `start..end` is the range of entries to process.
///
/// Entries are consumed via `std::mem::take` — each entry's key and value
/// strings are moved into the resulting `MiniYamlNode` without cloning.
/// This halves the per-node heap allocations compared to a borrow + clone
/// approach, since every entry is visited exactly once during tree construction.
fn build_tree(
    entries: &mut [(usize, String, Option<String>)],
    expected_indent: usize,
    start: usize,
    end: usize,
) -> Result<Vec<MiniYamlNode>, Error> {
    let mut nodes = Vec::new();
    let mut i = start;

    while i < end {
        let indent = entries.get(i).map_or(0, |e| e.0);

        // Skip entries at deeper indentation than expected (orphaned children
        // of a skipped parent — shouldn't happen in well-formed input, but
        // be resilient).
        if indent > expected_indent {
            i += 1;
            continue;
        }

        // Skip entries at shallower indentation (belongs to a parent scope).
        if indent < expected_indent {
            break;
        }

        // Find the range of children (entries at indent + 1 immediately following).
        let child_start = i + 1;
        let mut child_end = child_start;
        while child_end < end
            && entries
                .get(child_end)
                .is_some_and(|e| e.0 > expected_indent)
        {
            child_end += 1;
        }

        let children = if child_start < child_end {
            build_tree(entries, expected_indent + 1, child_start, child_end)?
        } else {
            Vec::new()
        };

        // Move strings out of the entry instead of cloning — zero extra allocs.
        // Safety: `i < end <= entries.len()` is maintained by the while-loop guard.
        let (key, value) = entries
            .get_mut(i)
            .map(|e| (std::mem::take(&mut e.1), std::mem::take(&mut e.2)))
            .unwrap_or_default();

        nodes.push(MiniYamlNode {
            key,
            value,
            children,
        });

        i = child_end;
    }

    Ok(nodes)
}

/// Converts a MiniYAML document to standard YAML.
///
/// The output is valid YAML that can be parsed by `serde_yaml` or any
/// standard YAML parser.  Indentation uses two spaces per level (YAML
/// convention).
///
/// ## Conversion rules
///
/// - `Key: Value` → `Key: Value` (identical in YAML)
/// - `Key:` with children → `Key:` followed by indented children
/// - `Inherits: @parent` → preserved as-is (the IC engine interprets this)
/// - `^RemoveNode` → `^RemoveNode:` (preserved as a key for engine processing)
/// - `#` comments are stripped during parsing and not preserved in output
pub fn to_yaml(doc: &MiniYamlDoc) -> String {
    let mut output = String::new();
    for node in &doc.nodes {
        write_yaml_node(&mut output, node, 0);
    }
    output
}

/// Writes a single node and its children as YAML at the given indent level.
///
/// Uses direct `push_str` / `push` calls instead of `format!` to avoid
/// allocating temporary `String`s for every node.  The indent prefix is
/// written by pushing `"  "` N times instead of `"  ".repeat(indent)`
/// which would allocate a new `String` per call.
fn write_yaml_node(output: &mut String, node: &MiniYamlNode, indent: usize) {
    // Write indent prefix directly — no heap alloc.
    for _ in 0..indent {
        output.push_str("  ");
    }

    // Write key.
    output.push_str(&node.key);

    // Write value (or bare colon) then newline.
    match &node.value {
        Some(v) => {
            output.push_str(": ");
            if needs_yaml_quoting(v) {
                output.push('\'');
                // yaml_escape only allocates when the value contains `'`,
                // which is rare.  For the common case this is just a push_str.
                if v.contains('\'') {
                    output.push_str(&v.replace('\'', "''"));
                } else {
                    output.push_str(v);
                }
                output.push('\'');
            } else {
                output.push_str(v);
            }
            output.push('\n');
        }
        None => {
            output.push_str(":\n");
        }
    }

    // Recurse into children.
    for child in &node.children {
        write_yaml_node(output, child, indent + 1);
    }
}

/// Checks whether a YAML value string needs quoting.
///
/// Values that start with special YAML characters, contain comment markers,
/// or could be misinterpreted as YAML types (booleans, nulls) are quoted.
/// This prevents data corruption when the output is consumed by a standard
/// YAML parser.
fn needs_yaml_quoting(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }

    // Values starting with YAML-special characters.
    let first = match value.as_bytes().first() {
        Some(&b) => b,
        None => return true,
    };
    if matches!(
        first,
        b'@' | b'^'
            | b'*'
            | b'&'
            | b'!'
            | b'{'
            | b'['
            | b'\''
            | b'"'
            | b'|'
            | b'>'
            | b'%'
            | b'?'
            | b'-'
            | b'#'
    ) {
        return true;
    }

    // Values containing `#` anywhere — a YAML parser would treat `#` as
    // the start of an inline comment, silently truncating the value.
    // This is critical after `\#` → `#` unescaping from MiniYAML input.
    if value.contains('#') {
        return true;
    }

    // Values containing `: ` (colon-space) could be misinterpreted as
    // YAML mapping keys in flow context.
    if value.contains(": ") {
        return true;
    }

    // Values that YAML would interpret as boolean or null.
    let lower = value.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~"
    )
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_validation;
