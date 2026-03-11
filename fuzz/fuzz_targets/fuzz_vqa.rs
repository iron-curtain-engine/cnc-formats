// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(vqa) = cnc_formats::vqa::VqaFile::parse(data) {
        // Exercise frame index (populated by parse() from FINF chunk).
        let _ = vqa.frame_index.as_ref().map(|fi| fi.len());
        // Walk all parsed chunks to touch borrowed data.
        for chunk in &vqa.chunks {
            let _ = chunk.data.len();
        }
    }
});
