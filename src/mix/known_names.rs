// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

//! Built-in filename database for MIX CRC resolution.
//!
//! Contains known filenames from Tiberian Dawn and Red Alert 1,
//! sourced from the XCC community database
//! (<https://github.com/askeladdk/xcc_gmdb_creator>).

/// One candidate filename per line.
pub(crate) const TD_RA1_FILENAME_CANDIDATES: &str = include_str!("known_names_td_ra1.txt");
