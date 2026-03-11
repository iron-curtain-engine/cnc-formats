// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(archive) = cnc_formats::mix::MixArchive::parse(data) {
        // Exercise lookup with a few common filenames.
        let _ = archive.get("CONQUER.MIX");
        let _ = archive.get("TEST.DAT");
        for entry in archive.entries() {
            let _ = archive.get_by_crc(entry.crc);
        }
    }
});
