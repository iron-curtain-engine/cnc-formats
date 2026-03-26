// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! `inspect` subcommand — parse a file and print human-readable metadata.

mod extra;
mod extra2;

use super::{print_format_hint, read_file, resolve_format, Format};

// ── inspect ──────────────────────────────────────────────────────────────────

/// Parse the file and print a human-readable metadata summary.
pub(crate) fn cmd_inspect(path: &str, explicit: Option<Format>) -> i32 {
    let fmt = resolve_format(path, explicit);
    let data = read_file(path);
    match inspect_data(&data, &fmt) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Error parsing {path}: {e}");
            print_format_hint(path);
            1
        }
    }
}

/// Parse and print metadata for the given format.
fn inspect_data(data: &[u8], fmt: &Format) -> Result<(), cnc_formats::Error> {
    match fmt {
        Format::Mix => inspect_mix(data),
        Format::Big => inspect_big(data),
        Format::Shp => inspect_shp(data),
        Format::Pal => inspect_pal(data),
        Format::Aud => inspect_aud(data),
        Format::Lut => inspect_lut(data),
        Format::Dip => inspect_dip(data),
        Format::Tmp => inspect_td_tmp(data),
        Format::TmpRa => inspect_ra_tmp(data),
        Format::Vqa => inspect_vqa(data),
        Format::Vqp => inspect_vqp(data),
        Format::Wsa => inspect_wsa(data),
        Format::Fnt => inspect_fnt(data),
        Format::Eng => inspect_eng(data),
        Format::Ini => inspect_ini(data),
        Format::Vxl => extra::inspect_vxl(data),
        Format::Hva => extra::inspect_hva(data),
        Format::ShpTs => extra::inspect_shp_ts(data),
        Format::Csf => extra::inspect_csf(data),
        Format::Cps => extra::inspect_cps(data),
        Format::W3d => extra::inspect_w3d(data),
        Format::TmpTs => extra::inspect_ts_tmp(data),
        Format::Voc => extra::inspect_voc(data),
        Format::Pak => extra::inspect_pak(data),
        Format::ShpD2 => extra2::inspect_shp_d2(data),
        Format::Icn => extra2::inspect_icn(data),
        Format::D2Map => extra2::inspect_d2_map(data),
        Format::BinTd => extra2::inspect_bin_td(data),
        Format::Mpr => extra2::inspect_mpr(data),
        Format::BagIdx => extra2::inspect_bag_idx(data),
        Format::MapRa2 => extra2::inspect_map_ra2(data),
        Format::Wnd => extra2::inspect_wnd(data),
        Format::SageStr => extra2::inspect_sage_str(data),
        Format::MapSage => extra2::inspect_map_sage(data),
        Format::Apt => extra2::inspect_apt(data),
        Format::Dds => extra2::inspect_dds(data),
        Format::Tga => extra2::inspect_tga(data),
        #[cfg(feature = "miniyaml")]
        Format::Miniyaml => extra::inspect_miniyaml(data),
        #[cfg(feature = "convert")]
        Format::Avi => extra::inspect_avi(data),
        #[cfg(feature = "midi")]
        Format::Mid => extra::inspect_mid(data),
        #[cfg(feature = "adl")]
        Format::Adl => extra::inspect_adl(data),
        #[cfg(feature = "xmi")]
        Format::Xmi => extra::inspect_xmi(data),
        #[cfg(feature = "meg")]
        Format::Meg => extra2::inspect_meg(data),
    }
}

fn inspect_mix(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let archive = cnc_formats::mix::MixArchive::parse(data)?;
    let entries = archive.entries();
    println!("MIX archive");
    println!("  Entries:    {}", entries.len());
    let total_size: u64 = entries.iter().map(|e| u64::from(e.size)).sum();
    println!("  Total data: {} bytes", total_size);
    println!();
    println!("  {:>10}  {:>10}  {:>10}", "CRC", "Offset", "Size");
    for entry in entries {
        println!(
            "  0x{:08X}  {:>10}  {:>10}",
            entry.crc.to_raw(),
            entry.offset,
            entry.size
        );
    }
    Ok(())
}

fn inspect_big(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let archive = cnc_formats::big::BigArchive::parse(data)?;
    let entries = archive.entries();
    let version = match archive.version() {
        cnc_formats::big::BigVersion::BigF => "BIGF",
        cnc_formats::big::BigVersion::Big4 => "BIG4",
    };
    println!("BIG archive ({version})");
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

fn inspect_shp(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let shp = cnc_formats::shp::ShpFile::parse(data)?;
    let h = &shp.header;
    println!("SHP sprite");
    println!("  Frames:     {}", h.frame_count);
    println!("  Dimensions: {}x{}", h.width, h.height);
    println!("  Largest:    {} bytes", h.largest_frame_size);
    println!(
        "  Palette:    {}",
        if h.has_embedded_palette() {
            "embedded"
        } else {
            "external"
        }
    );
    for (i, frame) in shp.frames.iter().enumerate() {
        let kind = match frame.format {
            cnc_formats::shp::ShpFrameFormat::Lcw => "LCW",
            cnc_formats::shp::ShpFrameFormat::XorLcw => "XOR+LCW",
            cnc_formats::shp::ShpFrameFormat::XorPrev => "XOR+Prev",
        };
        println!("  Frame {:>4}: {} bytes ({})", i, frame.data.len(), kind);
    }
    Ok(())
}

fn inspect_pal(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let pal = cnc_formats::pal::Palette::parse(data)?;
    println!("PAL palette (256 colors, 6-bit VGA)");
    // Show the first 16 colors as a sample.
    for (i, c) in pal.colors.iter().enumerate().take(16) {
        let rgb8 = c.to_rgb8();
        println!(
            "  [{:>3}] VGA({:>2},{:>2},{:>2})  RGB8({:>3},{:>3},{:>3})",
            i,
            c.r,
            c.g,
            c.b,
            rgb8.first().copied().unwrap_or(0),
            rgb8.get(1).copied().unwrap_or(0),
            rgb8.get(2).copied().unwrap_or(0),
        );
    }
    if pal.colors.len() > 16 {
        println!("  ... ({} more colors)", pal.colors.len() - 16);
    }
    Ok(())
}

fn inspect_aud(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let aud = cnc_formats::aud::AudFile::parse(data)?;
    let h = &aud.header;
    let channels = if h.is_stereo() { "stereo" } else { "mono" };
    let bits = if h.is_16bit() { 16 } else { 8 };
    println!("AUD audio");
    println!("  Sample rate:       {} Hz", h.sample_rate);
    println!("  Channels:          {channels}");
    println!("  Bit depth:         {bits}-bit");
    println!("  Compression ID:    {}", h.compression);
    println!("  Compressed size:   {} bytes", h.compressed_size);
    println!("  Uncompressed size: {} bytes", h.uncompressed_size);
    let ratio = if h.compressed_size > 0 {
        h.uncompressed_size as f64 / h.compressed_size as f64
    } else {
        0.0
    };
    println!("  Compression ratio: {ratio:.2}x");
    Ok(())
}

fn inspect_lut(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let lut = cnc_formats::lut::LutFile::parse(data)?;
    let mut min_value = u8::MAX;
    let mut max_value = 0u8;
    for entry in &lut.entries {
        if entry.value < min_value {
            min_value = entry.value;
        }
        if entry.value > max_value {
            max_value = entry.value;
        }
    }

    println!("LUT lookup table");
    println!("  Entries:     {}", lut.entry_count());
    println!("  Value range: {}..{}", min_value, max_value);
    if let Some(first) = lut.entries.first() {
        println!(
            "  First entry: x={}, y={}, value={}",
            first.x, first.y, first.value
        );
    }
    Ok(())
}

fn inspect_dip(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let dip = cnc_formats::dip::DipFile::parse(data)?;
    match dip {
        cnc_formats::dip::DipFile::StringTable(strings) => {
            println!("DIP installer string table");
            println!("  Strings:    {}", strings.string_count());
            println!("  Data start: {} bytes", strings.data_start);

            for string in strings.strings.iter().take(8) {
                let preview = string.as_lossy_str();
                let preview = if preview.is_empty() {
                    "<empty>"
                } else {
                    preview.as_ref()
                };
                println!("  [{:>4}] {}", string.index, preview);
            }

            if strings.string_count() > 8 {
                println!("  ... ({} more strings)", strings.string_count() - 8);
            }
        }
        cnc_formats::dip::DipFile::Segmented(segmented) => {
            println!("DIP installer segmented data");
            println!("  Sections:    {}", segmented.section_count);
            println!("  Header size: {} bytes", segmented.header_size);
            for section in &segmented.sections {
                println!(
                    "  Section {:>2}: {}..{} ({} bytes)",
                    section.index,
                    section.start,
                    section.end,
                    section.data.len()
                );
            }
            if segmented.trailer.len() == 2 {
                if let Some(bytes) = segmented.trailer.get(..2) {
                    let mut trailer_bytes = [0u8; 2];
                    trailer_bytes.copy_from_slice(bytes);
                    let trailer = u16::from_le_bytes(trailer_bytes);
                    println!("  Trailer:     0x{trailer:04X}");
                }
            }
        }
    }

    Ok(())
}

fn inspect_td_tmp(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let tmp = cnc_formats::tmp::TdTmpFile::parse(data)?;
    let h = &tmp.header;
    println!("TMP terrain (Tiberian Dawn)");
    println!("  Icon size:  {}x{} px", h.icon_width, h.icon_height);
    println!("  Count:      {}", h.count);
    println!("  Map data:   {} bytes", tmp.map_data.len());
    Ok(())
}

fn inspect_ra_tmp(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let tmp = cnc_formats::tmp::RaTmpFile::parse(data)?;
    let h = &tmp.header;
    let cols = h.cols();
    let rows = h.rows();
    let present = tmp.tiles.iter().filter(|t| t.is_some()).count();
    println!("TMP terrain (Red Alert)");
    println!("  Image size: {}x{} px", h.image_width, h.image_height);
    println!("  Tile size:  {}x{} px", h.tile_width, h.tile_height);
    println!("  Grid:       {}x{} ({} cells)", cols, rows, cols * rows);
    println!("  Present:    {} tiles", present);
    Ok(())
}

fn inspect_vqa(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let vqa = cnc_formats::vqa::VqaFile::parse(data)?;
    let h = &vqa.header;
    println!("VQA video");
    println!("  Version:    {}", h.version);
    println!("  Dimensions: {}x{}", h.width, h.height);
    println!("  Frames:     {}", h.num_frames);
    println!("  Block size: {}x{}", h.block_w, h.block_h);
    println!(
        "  Codebook:   {} entries, group of {}",
        h.cb_entries, h.groupsize
    );
    println!("  FPS:        {}", h.fps);
    if h.has_audio() {
        let ch = if h.is_stereo() { "stereo" } else { "mono" };
        println!("  Audio:      {} Hz, {}-bit, {ch}", h.freq, h.bits);
    } else {
        println!("  Audio:      none");
    }
    println!("  Chunks:     {}", vqa.chunks.len());
    // Show chunk type summary.
    let mut chunk_types: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for chunk in &vqa.chunks {
        let tag = String::from_utf8_lossy(&chunk.fourcc).to_string();
        *chunk_types.entry(tag).or_insert(0) += 1;
    }
    for (tag, count) in &chunk_types {
        println!("    {tag}: {count}");
    }
    Ok(())
}

fn inspect_vqp(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let vqp = cnc_formats::vqp::VqpFile::parse(data)?;
    println!("VQP palette interpolation tables");
    println!("  Tables:      {}", vqp.num_tables);
    println!(
        "  Table size:  {} bytes packed",
        cnc_formats::vqp::VQP_TABLE_SIZE
    );
    println!("  Expanded:    {} bytes per table", 256usize * 256usize);

    if let Some(first) = vqp.tables.first() {
        println!("  Sample[0,0]: {}", first.get(0, 0));
        println!("  Sample[1,0]: {}", first.get(1, 0));
        println!("  Sample[1,1]: {}", first.get(1, 1));
    }

    Ok(())
}

fn inspect_wsa(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let wsa = cnc_formats::wsa::WsaFile::parse(data)?;
    let h = &wsa.header;
    println!("WSA animation");
    println!("  Frames:     {}", h.num_frames);
    println!("  Dimensions: {}x{}", h.width, h.height);
    println!("  Position:   ({}, {})", h.x, h.y);
    println!("  Largest Δ:  {} bytes", h.largest_frame_size);
    println!(
        "  Palette:    {}",
        if h.has_embedded_palette() {
            "embedded"
        } else {
            "external"
        }
    );
    println!(
        "  Looping:    {}",
        if wsa.has_loop_frame { "yes" } else { "no" }
    );
    for (i, frame) in wsa.frames.iter().enumerate() {
        println!("  Frame {:>4}: {} bytes (LCW)", i, frame.data.len());
    }
    Ok(())
}

fn inspect_fnt(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let fnt = cnc_formats::fnt::FntFile::parse(data)?;
    let h = &fnt.header;
    let non_empty = fnt.glyphs.iter().filter(|g| g.width > 0).count();
    println!("FNT bitmap font");
    println!("  Characters:  {}", h.num_chars);
    println!("  Max height:  {} px", h.max_height);
    println!("  Max width:   {} px", h.max_width);
    println!("  Glyphs:      {} ({} non-empty)", h.num_chars, non_empty);
    Ok(())
}

fn inspect_eng(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let eng = cnc_formats::eng::EngFile::parse(data)?;
    println!("ENG string table");
    println!("  Strings:    {}", eng.string_count());
    println!("  Data start: {} bytes", eng.data_start);

    for string in eng.strings.iter().take(8) {
        let preview = string.as_lossy_str();
        let preview = if preview.is_empty() {
            "<empty>"
        } else {
            preview.as_ref()
        };
        println!("  [{:>4}] {}", string.index, preview);
    }

    if eng.string_count() > 8 {
        println!("  ... ({} more strings)", eng.string_count() - 8);
    }

    Ok(())
}

fn inspect_ini(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let ini = cnc_formats::ini::IniFile::parse(data)?;
    println!("INI configuration");
    println!("  Sections: {}", ini.section_count());
    for section in ini.sections() {
        println!("  [{}] ({} keys)", section.name(), section.len());
    }
    Ok(())
}
