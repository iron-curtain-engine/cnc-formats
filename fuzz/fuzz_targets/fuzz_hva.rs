// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(hva) = cnc_formats::hva::HvaFile::parse(data) {
        // Touch parsed data to ensure no lazy panics.
        let _ = hva.section_names.len();
        let _ = hva.transforms.len();
    }
});
