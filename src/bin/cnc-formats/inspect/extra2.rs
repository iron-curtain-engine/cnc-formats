// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Additional format inspectors (second half) — split from `extra.rs` to keep
//! file sizes within the RAG/LLM friendliness target of ≤600 lines.
//!
//! Handles: SHP-D2, ICN, D2 Map, BIN-TD, MPR, BAG-IDX, RA2 Map, WND,
//! SAGE-STR, SAGE Map, APT, DDS, TGA, MEG.
//!
//! See [`super::extra`] for the first half (AVI through PAK).

pub(super) fn inspect_shp_d2(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let shp = cnc_formats::shp_d2::ShpD2File::parse(data)?;
    println!("SHP sprite (Dune II, Format80/LCW)");
    println!("  Frames: {}", shp.frame_count());
    for (i, frame) in shp.frames().iter().enumerate() {
        let remap = if frame.remap.is_some() { " +remap" } else { "" };
        println!(
            "  Frame {:>3}: {}x{} ({} px){remap}",
            i,
            frame.width,
            frame.height,
            frame.pixels.len()
        );
    }
    Ok(())
}

pub(super) fn inspect_icn(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let icn = cnc_formats::icn::IcnFile::parse(data, 16, 16)?;
    println!("ICN tile graphics (Dune II)");
    println!(
        "  Tile size:  {}x{} px",
        icn.tile_width(),
        icn.tile_height()
    );
    println!("  Tiles:      {}", icn.tile_count());
    Ok(())
}

pub(super) fn inspect_d2_map(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let scn = cnc_formats::d2_map::D2Scenario::parse(data)?;
    let h = &scn.header;
    let house = scn
        .house()
        .map(|h| format!("{h:?}"))
        .unwrap_or_else(|| format!("unknown ({})", h.active_house));
    println!("Dune II scenario");
    println!("  Map seed:    0x{:04X}", h.map_seed);
    println!("  Map scale:   {}", h.map_scale);
    println!("  Cursor:      ({}, {})", h.cursor_x, h.cursor_y);
    println!("  House:       {house}");
    println!("  Win flags:   0x{:04X}", h.win_flags);
    println!("  Lose flags:  0x{:04X}", h.lose_flags);
    println!("  Time limit:  {}", h.time_limit);
    println!("  Placement:   {} bytes", scn.placement_data().len());
    Ok(())
}

pub(super) fn inspect_bin_td(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let map = cnc_formats::bin_td::BinMap::parse(data, 64, 64)?;
    let non_empty = map
        .cells()
        .iter()
        .filter(|c| c.template_type != 0 || c.template_icon != 0)
        .count();
    println!("BIN terrain grid (TD/RA1)");
    println!("  Dimensions: {}x{}", map.width(), map.height());
    println!("  Cells:      {}", map.cells().len());
    println!("  Non-empty:  {non_empty}");
    Ok(())
}

pub(super) fn inspect_mpr(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let mpr = cnc_formats::mpr::MprFile::parse(data)?;
    println!("MPR map package (TD/RA1)");
    if let Some(name) = mpr.name() {
        println!("  Name:     {name}");
    }
    if let Some(theater) = mpr.theater() {
        println!("  Theater:  {theater}");
    }
    if let Some(bounds) = mpr.bounds() {
        println!(
            "  Bounds:   ({}, {}) {}x{}",
            bounds.x, bounds.y, bounds.width, bounds.height
        );
    }
    println!(
        "  MapPack:  {}",
        if mpr.map_pack_raw().is_some() {
            "present"
        } else {
            "absent"
        }
    );
    println!(
        "  Overlay:  {}",
        if mpr.overlay_pack_raw().is_some() {
            "present"
        } else {
            "absent"
        }
    );
    let ini = mpr.ini();
    println!("  Sections: {}", ini.section_count());
    for section in ini.sections() {
        println!("    [{}] ({} keys)", section.name(), section.len());
    }
    Ok(())
}

pub(super) fn inspect_bag_idx(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let idx = cnc_formats::bag_idx::IdxFile::parse(data)?;
    let entries = idx.entries();
    println!("IDX audio index (RA2)");
    println!("  Entries: {}", entries.len());
    let total: u64 = entries.iter().map(|e| u64::from(e.size)).sum();
    println!("  Total:   {} bytes", total);
    println!();
    println!("  {:>10}  {:>10}  {:>6}  Name", "Offset", "Size", "Rate");
    for entry in entries.iter().take(20) {
        println!(
            "  {:>10}  {:>10}  {:>6}  {}",
            entry.offset, entry.size, entry.sample_rate, entry.name
        );
    }
    if entries.len() > 20 {
        println!("  ... ({} more entries)", entries.len() - 20);
    }
    Ok(())
}

pub(super) fn inspect_map_ra2(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let map = cnc_formats::map_ra2::MapRa2File::parse(data)?;
    println!("RA2 map file");
    if let Some(name) = map.name() {
        println!("  Name:     {name}");
    }
    if let Some(author) = map.author() {
        println!("  Author:   {author}");
    }
    if let Some(theater) = map.theater() {
        println!("  Theater:  {theater}");
    }
    if let Some(size) = map.size() {
        println!(
            "  Size:     ({}, {}) {}x{}",
            size.x, size.y, size.width, size.height
        );
    }
    if let Some(local) = map.local_size() {
        println!(
            "  Local:    ({}, {}) {}x{}",
            local.x, local.y, local.width, local.height
        );
    }
    println!(
        "  IsoMap:   {}",
        if map.iso_map_pack_raw().is_some() {
            "present"
        } else {
            "absent"
        }
    );
    println!("  Waypoints: {}", map.waypoint_count());
    let ini = map.ini();
    println!("  Sections:  {}", ini.section_count());
    Ok(())
}

pub(super) fn inspect_wnd(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let wnd = cnc_formats::wnd::WndFile::parse(data)?;
    println!("WND UI layout (Generals/SAGE)");
    if let Some(v) = wnd.version {
        println!("  Version:  {v}");
    }
    println!("  Windows:  {} (top-level)", wnd.windows.len());
    println!("  Total:    {}", wnd.window_count());
    for win in &wnd.windows {
        let name = win.name().unwrap_or("(unnamed)");
        let wtype = win.window_type().unwrap_or("(unknown)");
        println!("  [{wtype}] {name} ({} children)", win.children.len());
    }
    Ok(())
}

pub(super) fn inspect_sage_str(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let strs = cnc_formats::sage_str::StrFile::parse(data)?;
    println!("STR string table (Generals/SAGE)");
    println!("  Entries: {}", strs.len());
    for entry in strs.entries().iter().take(8) {
        let preview = if entry.value.len() > 60 {
            format!("{}...", entry.value.get(..60).unwrap_or(&entry.value))
        } else {
            entry.value.clone()
        };
        println!("  {}: \"{}\"", entry.id, preview);
    }
    if strs.len() > 8 {
        println!("  ... ({} more entries)", strs.len() - 8);
    }
    Ok(())
}

pub(super) fn inspect_map_sage(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let map = cnc_formats::map_sage::MapSageFile::parse(data)?;
    println!("SAGE map (Generals binary)");
    println!("  Chunks: {}", map.chunk_count());
    for chunk in map.chunks() {
        println!(
            "  [{}] v{}, {} bytes",
            chunk.name,
            chunk.version,
            chunk.data.len()
        );
    }
    Ok(())
}

pub(super) fn inspect_apt(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let apt = cnc_formats::apt::AptFile::parse(data)?;
    println!("APT GUI animation (Generals/SAGE)");
    println!("  Data offset: {}", apt.apt_data_offset());
    println!("  Entries:     {}", apt.entry_count());
    for (i, entry) in apt.entries().iter().enumerate() {
        println!(
            "  Entry {:>2}: offset={}, fields=[{}, {}, {}, {}]",
            i,
            entry.entry_offset,
            entry.fields[0],
            entry.fields[1],
            entry.fields[2],
            entry.fields[3]
        );
    }
    Ok(())
}

pub(super) fn inspect_dds(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let dds = cnc_formats::dds::DdsFile::parse(data)?;
    let compressed = if dds.is_compressed() {
        format!("yes ({})", dds.four_cc_str().unwrap_or("?"))
    } else {
        "no".to_string()
    };
    println!("DDS texture (DirectDraw Surface)");
    println!("  Dimensions:  {}x{}", dds.width, dds.height);
    if dds.depth > 1 {
        println!("  Depth:       {}", dds.depth);
    }
    println!("  Mipmaps:     {}", dds.mip_map_count);
    println!("  Compressed:  {compressed}");
    println!("  Bit count:   {}", dds.pixel_format.rgb_bit_count);
    println!(
        "  DX10:        {}",
        if dds.has_dx10() { "yes" } else { "no" }
    );
    println!("  Pixel data:  {} bytes", dds.pixel_data().len());
    Ok(())
}

pub(super) fn inspect_tga(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let tga = cnc_formats::tga::TgaFile::parse(data)?;
    let h = &tga.header;
    let rle = if tga.is_rle() { " (RLE)" } else { "" };
    println!("TGA image (Truevision)");
    println!("  Dimensions:  {}x{}", h.width, h.height);
    println!("  Pixel depth: {}-bit{rle}", h.pixel_depth);
    println!("  Image type:  {:?}", h.image_type);
    println!(
        "  Color map:   {}",
        if tga.has_color_map() {
            format!("{} entries", h.color_map_length)
        } else {
            "none".to_string()
        }
    );
    println!("  Origin:      ({}, {})", h.x_origin, h.y_origin);
    println!(
        "  Footer:      {}",
        if tga.has_footer() { "TGA 2.0" } else { "none" }
    );
    println!("  Image data:  {} bytes", tga.image_data().len());
    Ok(())
}

#[cfg(feature = "meg")]
pub(super) fn inspect_meg(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let archive = cnc_formats::meg::MegArchive::parse(data)?;
    let entries = archive.entries();
    println!("MEG archive (Petroglyph)");
    println!("  Entries:    {}", entries.len());
    let total_size: u64 = entries.iter().map(|e| e.size).sum();
    println!("  Total data: {} bytes", total_size);
    println!();
    println!("  {:>10}  {:>10}  Name", "Offset", "Size");
    for entry in entries {
        println!("  {:>10}  {:>10}  {}", entry.offset, entry.size, entry.name);
    }
    Ok(())
}
