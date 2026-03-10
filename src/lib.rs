// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! # cnc-formats — Clean-Room C&C Binary Format Parsers
//!
//! This crate provides parsers for binary file formats used by
//! Command & Conquer games (Red Alert, Tiberian Dawn, and related titles):
//!
//! | Module          | Format | Description                              |
//! |-----------------|--------|------------------------------------------|
//! | [`mix`]         | `.mix` | Flat archive with CRC-based file lookup  |
//! | [`pal`]         | `.pal` | 256-color 6-bit VGA palette              |
//! | [`shp`]         | `.shp` | Keyframe sprite animation frames         |
//! | [`aud`]         | `.aud` | Westwood IMA ADPCM audio                 |
//! | [`lcw`]         | —      | LCW decompression used by SHP/VQA/TMP    |
//!
//! ## Clean-Room Design
//!
//! All parsing is implemented from publicly available format documentation
//! and binary analysis.  This crate contains **no EA-derived code**, which
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

pub mod aud;
pub mod error;
pub mod lcw;
pub mod mix;
pub mod pal;
pub mod shp;

pub use error::Error;
