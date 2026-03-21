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
//! | [`big`]         | `.big` | EA BIG archive with stored filenames     |
//! | [`dip`]         | `.dip` | Westwood setup/installer data            |
//! | [`eng`]         | `.eng` | Westwood language string tables          |
//! | [`lcw`]         | —      | LCW decompression used by SHP/VQA/WSA    |
//! | [`lut`]         | `.lut` | Red Alert Chrono Vortex lookup tables    |
//! | [`tmp`]         | `.tmp` | Terrain tile sets (TD + RA + TS/RA2 iso) |
//! | [`vqa`]         | `.vqa` | VQ video container (IFF chunk-based)     |
//! | [`vqp`]         | `.vqp` | VQA palette interpolation sidecar tables |
//! | [`wsa`]         | `.wsa` | LCW + XOR-delta animation                |
//! | [`fnt`]         | `.fnt` | Bitmap fonts (variable character count)   |
//! | [`cps`]         | `.cps` | Compressed full-screen images            |
//! | [`csf`]         | `.csf` | Compiled string tables (TS/RA2/Generals) |
//! | [`shp_ts`]      | `.shp` | TS/RA2 sprite frames (scanline RLE)      |
//! | [`vxl`]         | `.vxl` | Voxel models (TS/RA2)                    |
//! | [`hva`]         | `.hva` | Hierarchical voxel animation (TS/RA2)    |
//! | [`w3d`]         | `.w3d` | Westwood 3D meshes (Generals/SAGE)       |
//! | [`ini`]         | `.ini` | Classic C&C rules file parser            |
//!
//! Feature-gated:
//!
//! | Module          | Feature    | Format   | Description                        |
//! |-----------------|------------|----------|------------------------------------|
//! | [`miniyaml`]    | `miniyaml` | MiniYAML | OpenRA configuration file parser   |
//! | [`mid`]         | `midi`     | `.mid`   | Standard MIDI file parser/writer   |
//! | [`adl`]         | `adl`      | `.adl`   | AdLib OPL2 music parser (Dune II)  |
//! | [`xmi`]         | `xmi`      | `.xmi`   | XMIDI parser + XMI→MID converter   |
//! | [`transcribe`]  | `transcribe`| —       | PCM→MIDI transcription (YIN pitch) |
//! | [`meg`]         | `meg`      | `.meg`   | Petroglyph MEG/PGM archive parser  |
//!
//! ## Clean-Room Design
//!
//! All parsing is implemented from publicly available format documentation
//! and binary analysis.  This crate contains **no EA-derived code**, which
//! is why it can be licensed under MIT/Apache-2.0.
//!
//! EA GPL-derived parsing logic lives in the `ic-cnc-content` crate (GPL v3)
//! in the Iron Curtain engine repository.
//!
//! ## Design Authority
//!
//! Format specifications and architectural decisions are documented in the
//! [Iron Curtain Design Documentation](https://github.com/iron-curtain-engine/iron-curtain-design-docs).
//!
//! Related decisions: D076 (standalone crate extraction), D003 (YAML format).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

// ── Public modules ───────────────────────────────────────────────────────────
// Each module is a self-contained parser for one file format.  Modules depend
// only on `error` and `lcw`; there are no circular dependencies.

pub mod aud;
pub mod big;
#[cfg(feature = "convert")]
pub mod convert;
/// Compressed Screen Picture images (TD/RA1/Dune II title screens).
pub mod cps;
pub mod csf;
pub mod dip;
pub mod eng;
pub mod error;
pub mod fnt;
/// Hierarchical Voxel Animation transforms (TS/RA2).
pub mod hva;
pub mod ini;
pub mod lcw;
pub mod lut;
pub mod mix;
#[cfg(feature = "encrypted-mix")]
pub mod mix_crypt;
pub mod pal;
pub(crate) mod read;
pub mod shp;
/// TS/RA2 SHP sprites (scanline RLE, distinct from TD/RA1 [`shp`]).
pub mod shp_ts;
/// Format detection by content inspection (magic-byte sniffing).
pub mod sniff;
pub(crate) mod stream_io;
pub mod tmp;
pub mod vqa;
pub mod vqp;
/// Voxel models for TS/RA2 3D units.
pub mod vxl;
/// Westwood 3D chunk-based mesh format (Generals/SAGE engine).
pub mod w3d;
pub mod wsa;

#[cfg(feature = "miniyaml")]
pub mod miniyaml;

#[cfg(feature = "midi")]
pub mod mid;

#[cfg(feature = "adl")]
pub mod adl;

#[cfg(feature = "xmi")]
pub mod xmi;

#[cfg(feature = "transcribe")]
pub mod transcribe;

#[cfg(feature = "meg")]
pub mod meg;

// Re-export `Error` at the crate root so callers can write `cnc_formats::Error`
// without descending into the `error` module.
pub use error::Error;
