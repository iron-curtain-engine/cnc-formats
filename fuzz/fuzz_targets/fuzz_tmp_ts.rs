// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(tmp) = cnc_formats::tmp::TsTmpFile::parse(data) {
        // Walk tiles to touch parsed data.
        for tile in &tmp.tiles {
            let _ = tile.iso_pixels.len();
        }
    }
});
