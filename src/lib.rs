// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! # cnc-formats — Clean-Room C&C Binary Format Parsers
//!
//! This crate provides parsers for binary file formats used by
//! Command & Conquer games (Red Alert, Tiberian Dawn, and related titles):
//!
//! - **`.mix`** — Archive containers holding game assets
//! - **`.shp`** — Sprite image frames
//! - **`.pal`** — 256-color palettes
//! - **`.aud`** — Audio samples (IMA ADPCM / Westwood ADPCM)
//! - **`.vqa`** — Full-motion video
//! - **MiniYAML** — Configuration format used by OpenRA
//!
//! ## Clean-Room Design
//!
//! All parsing is implemented from publicly available format documentation
//! and binary analysis. This crate contains **no EA-derived code**, which
//! is why it can be licensed under MIT/Apache-2.0.
//!
//! EA GPL-derived parsing logic lives in the `ra-formats` crate (GPL v3)
//! in the Iron Curtain engine repository.
//!
//! ## `no_std` Support
//!
//! This crate aims to support `#![no_std]` environments where possible.
//! Allocator-dependent features are gated behind the `alloc` feature.
//!
//! ## Design Authority
//!
//! Format specifications and architectural decisions are documented in the
//! [Iron Curtain Design Documentation](https://github.com/iron-curtain-engine/iron-curtain-design-docs).
//!
//! Related decisions: D076 (standalone crate extraction), D003 (YAML format).

#![warn(missing_docs)]

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder() {
        // Format parsing tests will be added in M1/G1 against the RA1 test corpus.
        // Each format module (.mix, .shp, .pal, .aud, .vqa) will have its own
        // test suite validating against known-good binary files.
    }
}
