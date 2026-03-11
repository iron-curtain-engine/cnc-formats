// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(shp) = cnc_formats::shp::ShpFile::parse(data) {
        let pixel_count = shp.frame_pixel_count();
        for frame in &shp.frames {
            // Attempt decompression; errors are fine.
            let _ = frame.pixels(pixel_count);
        }
    }
});
