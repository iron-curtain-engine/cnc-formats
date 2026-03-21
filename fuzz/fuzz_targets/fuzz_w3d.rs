// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(w3d) = cnc_formats::w3d::W3dFile::parse(data) {
        // Walk all chunks to touch parsed data.
        for chunk in &w3d.chunks {
            let _ = chunk.data.len();
        }
    }
});
