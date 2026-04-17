// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! # cnc-formats — Clean-Room C&C Binary Format Parsers
//!
//! This crate provides parsers for binary file formats used by
//! Command & Conquer games (Red Alert, Tiberian Dawn, and related titles):
//!
//! | Module          | Format         | Description                              |
//! |-----------------|----------------|------------------------------------------|
//! | [`mix`]         | `.mix`         | Flat archive with CRC-based file lookup  |
//! | [`pal`]         | `.pal`         | 256-color 6-bit VGA palette              |
//! | [`shp`]         | `.shp`         | Keyframe sprite animation frames         |
//! | [`aud`]         | `.aud`         | Westwood IMA ADPCM audio                 |
//! | [`big`]         | `.big`         | EA BIG archive with stored filenames     |
//! | [`dip`]         | `.dip`         | Westwood setup/installer data            |
//! | [`eng`]         | `.eng`         | Westwood language string tables          |
//! | [`iso9660`]     | `.iso`         | ISO 9660 CD-ROM filesystem image         |
//! | [`lcw`]         | —              | LCW decompression used by SHP/VQA/WSA    |
//! | [`lut`]         | `.lut`         | Red Alert Chrono Vortex lookup tables    |
//! | [`tmp`]         | `.tmp`         | Terrain tile sets (TD + RA + TS/RA2 iso) |
//! | [`vqa`]         | `.vqa`         | VQ video container (IFF chunk-based)     |
//! | [`vqp`]         | `.vqp`         | VQA palette interpolation sidecar tables |
//! | [`wsa`]         | `.wsa`         | LCW + XOR-delta animation                |
//! | [`fnt`]         | `.fnt`         | Bitmap fonts (variable character count)   |
//! | [`cps`]         | `.cps`         | Compressed full-screen images            |
//! | [`csf`]         | `.csf`         | Compiled string tables (TS/RA2/Generals) |
//! | [`shp_ts`]      | `.shp`         | TS/RA2 sprite frames (scanline RLE)      |
//! | [`vxl`]         | `.vxl`         | Voxel models (TS/RA2)                    |
//! | [`hva`]         | `.hva`         | Hierarchical voxel animation (TS/RA2)    |
//! | [`w3d`]         | `.w3d`         | Westwood 3D meshes (Generals/SAGE)       |
//! | [`ini`]         | `.ini`         | Classic C&C rules file parser            |
//! | [`apt`]         | `.apt`         | Generals GUI animation (SAGE)            |
//! | [`bag_idx`]     | `.bag`+`.idx`  | RA2 audio archive pair                   |
//! | [`bin_td`]      | `.bin`         | TD/RA1 terrain grid                      |
//! | [`d2_map`]      | scenario       | Dune II scenario/mission                 |
//! | [`dds`]         | `.dds`         | DirectDraw Surface texture headers       |
//! | [`icn`]         | `.icn`+`.map`  | Dune II tile graphics                    |
//! | [`map_ra2`]     | `.map`         | RA2 map files (INI-based)                |
//! | [`map_sage`]    | `.map`         | Generals map files (binary chunks)       |
//! | [`mpr`]         | `.mpr`         | TD/RA1 map packages (INI-based)          |
//! | [`pak`]         | `.pak`         | Dune II PAK archive                      |
//! | [`sage_str`]    | `.str`         | Generals string tables (SAGE)            |
//! | [`shp_d2`]      | `.shp`         | Dune II sprites (Format80/LCW)           |
//! | [`tga`]         | `.tga`         | Truevision TGA image headers             |
//! | [`voc`]         | `.voc`         | Creative Voice File (Dune II audio)      |
//! | [`wnd`]         | `.wnd`         | Generals UI layout (SAGE)                |
//!
//! Sniff-only (detected by magic bytes, no parser module):
//!
//! | Format          | Magic      | Description                              |
//! |-----------------|------------|------------------------------------------|
//! | JPEG            | `FF D8 FF` | Standard JPEG image (Generals textures)  |
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

/// Generals / Zero Hour APT GUI animation parser (`.apt`).
pub mod apt;
pub mod aud;
/// Red Alert 2 / Yuri's Revenge audio archive (`.bag` + `.idx`).
pub mod bag_idx;
pub mod big;
/// Tiberian Dawn / Red Alert 1 terrain grid (`.bin`).
pub mod bin_td;
#[cfg(feature = "convert")]
pub mod convert;
/// Compressed Screen Picture images (TD/RA1/Dune II title screens).
pub mod cps;
pub mod csf;
/// Dune II scenario/mission file parser.
pub mod d2_map;
/// DirectDraw Surface texture header parser (`.dds`).
pub mod dds;
pub mod dip;
pub mod eng;
pub mod error;
pub mod fnt;
/// Hierarchical Voxel Animation transforms (TS/RA2).
pub mod hva;
/// Dune II icon/tile graphics (`.icn` + `ICON.MAP`).
pub mod icn;
pub mod ini;
/// ISO 9660 / ECMA-119 CD-ROM filesystem image parser (`.iso`).
pub mod iso9660;
pub mod lcw;
pub mod lut;
/// Red Alert 2 / Yuri's Revenge map file parser (`.map`).
pub mod map_ra2;
/// Generals / Zero Hour binary map parser (`.map`, SAGE engine).
pub mod map_sage;
pub mod mix;
#[cfg(feature = "encrypted-mix")]
pub mod mix_crypt;
/// TD / RA1 map package parser (`.mpr`).
pub mod mpr;
/// Dune II PAK archive (`.pak`).
pub mod pak;
pub mod pal;
pub(crate) mod read;
/// Generals / Zero Hour SAGE string table (`.str`).
pub mod sage_str;
pub mod shp;
/// Dune II SHP sprites (Format80/LCW, distinct from TD/RA1 [`shp`]).
pub mod shp_d2;
/// TS/RA2 SHP sprites (scanline RLE, distinct from TD/RA1 [`shp`]).
pub mod shp_ts;
/// Format detection by content inspection (magic-byte sniffing).
pub mod sniff;
pub(crate) mod stream_io;
/// Truevision TGA image header parser (`.tga`).
pub mod tga;
pub mod tmp;
/// Creative Voice File audio container (`.voc`, Dune II).
pub mod voc;
pub mod vqa;
pub mod vqp;
/// Voxel models for TS/RA2 3D units.
pub mod vxl;
/// Westwood 3D chunk-based mesh format (Generals/SAGE engine).
pub mod w3d;
/// Generals / Zero Hour WND UI layout parser (`.wnd`).
pub mod wnd;
pub mod wsa;
/// Format40 XOR-delta decoder shared by SHP and WSA.
pub mod xor_delta;

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
