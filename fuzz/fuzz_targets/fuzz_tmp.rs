// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Try both TD and RA variants; errors are fine.
    let _ = cnc_formats::tmp::TdTmpFile::parse(data);
    let _ = cnc_formats::tmp::RaTmpFile::parse(data);
});
