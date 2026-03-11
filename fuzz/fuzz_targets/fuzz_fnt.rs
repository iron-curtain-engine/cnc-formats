// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(fnt) = cnc_formats::fnt::FntFile::parse(data) {
        // Exercise pixel queries on every glyph.
        for glyph in &fnt.glyphs {
            let _ = glyph.pixel(0, 0);
        }
    }
});
