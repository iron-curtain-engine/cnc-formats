// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(meg) = cnc_formats::meg::MegArchive::parse(data) {
        for entry in meg.entries() {
            let _ = entry.name.len();
        }
    }
});
