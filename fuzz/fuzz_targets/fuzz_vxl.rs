// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025-present Iron Curtain contributors

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(vxl) = cnc_formats::vxl::VxlFile::parse(data) {
        // Touch parsed data to ensure no lazy panics.
        let _ = vxl.limb_headers.len();
        let _ = vxl.limb_tailers.len();
        let _ = vxl.body_data.len();
    }
});
