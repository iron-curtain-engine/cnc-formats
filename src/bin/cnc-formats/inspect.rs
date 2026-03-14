// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! `inspect` subcommand — parse a file and print human-readable metadata.

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
        Format::Shp => inspect_shp(data),
        Format::Pal => inspect_pal(data),
        Format::Aud => inspect_aud(data),
        Format::Tmp => inspect_td_tmp(data),
        Format::TmpRa => inspect_ra_tmp(data),
        Format::Vqa => inspect_vqa(data),
        Format::Wsa => inspect_wsa(data),
        Format::Fnt => inspect_fnt(data),
        Format::Ini => inspect_ini(data),
        #[cfg(feature = "miniyaml")]
        Format::Miniyaml => inspect_miniyaml(data),
        #[cfg(feature = "convert")]
        Format::Avi => inspect_avi(data),
        #[cfg(feature = "midi")]
        Format::Mid => inspect_mid(data),
        #[cfg(feature = "adl")]
        Format::Adl => inspect_adl(data),
        #[cfg(feature = "xmi")]
        Format::Xmi => inspect_xmi(data),
        #[cfg(feature = "meg")]
        Format::Meg => inspect_meg(data),
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

fn inspect_ini(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let ini = cnc_formats::ini::IniFile::parse(data)?;
    println!("INI configuration");
    println!("  Sections: {}", ini.section_count());
    for section in ini.sections() {
        println!("  [{}] ({} keys)", section.name(), section.len());
    }
    Ok(())
}

#[cfg(feature = "convert")]
fn inspect_avi(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let avi = cnc_formats::convert::decode_avi(data)?;
    println!("AVI video (uncompressed)");
    println!("  Dimensions: {}×{}", avi.width, avi.height);
    println!("  Frames:     {}", avi.frames.len());
    println!("  FPS:        {}", avi.fps);
    if !avi.audio.is_empty() {
        println!(
            "  Audio:      {} samples, {}Hz, {} ch",
            avi.audio.len(),
            avi.sample_rate,
            avi.channels
        );
    } else {
        println!("  Audio:      none");
    }
    Ok(())
}

#[cfg(feature = "miniyaml")]
fn inspect_miniyaml(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let doc = cnc_formats::miniyaml::MiniYamlDoc::parse(data)?;
    let nodes = doc.nodes();
    fn count_nodes(nodes: &[cnc_formats::miniyaml::MiniYamlNode]) -> usize {
        let mut total = nodes.len();
        for n in nodes {
            total += count_nodes(n.children());
        }
        total
    }
    let total = count_nodes(nodes);
    println!("MiniYAML document");
    println!("  Root nodes:  {}", nodes.len());
    println!("  Total nodes: {total}");
    for node in nodes {
        let val = node.value().unwrap_or("");
        let children = node.children().len();
        if children > 0 {
            println!("  {}: {val} ({children} children)", node.key());
        } else {
            println!("  {}: {val}", node.key());
        }
    }
    Ok(())
}

#[cfg(feature = "midi")]
fn inspect_mid(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let mid = cnc_formats::mid::MidFile::parse(data)?;
    let format_name = match mid.format() {
        cnc_formats::mid::MidiFormat::SingleTrack => "Type 0 (single track)",
        cnc_formats::mid::MidiFormat::Parallel => "Type 1 (multi-track)",
        cnc_formats::mid::MidiFormat::Sequential => "Type 2 (multi-song)",
    };
    let timing_str = match mid.timing() {
        cnc_formats::mid::Timing::Metrical(tpb) => format!("{} ticks/beat", tpb.as_int()),
        cnc_formats::mid::Timing::Timecode(fps, sub) => format!("{fps:?} fps, {sub} sub"),
    };
    println!("MIDI file (Standard MIDI File)");
    println!("  Format:     {format_name}");
    println!("  Timing:     {timing_str}");
    println!("  Tracks:     {}", mid.track_count());
    println!("  Events:     {}", mid.event_count());
    println!("  Duration:   {:.2} s", mid.duration_secs());
    let channels = mid.channels_used();
    if !channels.is_empty() {
        let ch_list: Vec<String> = channels.iter().map(|c| c.to_string()).collect();
        println!("  Channels:   {}", ch_list.join(", "));
    }
    Ok(())
}

#[cfg(feature = "adl")]
fn inspect_adl(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let adl = cnc_formats::adl::AdlFile::parse(data)?;
    println!("ADL music (AdLib OPL2, Dune II)");
    println!("  Instruments:      {}", adl.instruments.len());
    println!("  Sub-songs:        {}", adl.subsongs.len());
    println!("  Register writes:  {}", adl.total_register_writes());
    match adl.estimated_duration_secs() {
        Some(duration) => println!("  Est. duration:    {:.2} s", duration),
        None => println!("  Est. duration:    unknown"),
    }
    for (i, subsong) in adl.subsongs.iter().enumerate() {
        match subsong.track_program() {
            Some(program) => {
                println!(
                    "  Sub-song {}: track={}, offset={}, data=opaque",
                    i, program.index, program.offset
                );
            }
            None => {
                let speed = subsong
                    .speed_ticks_per_step()
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                println!(
                    "  Sub-song {}: speed={}, channels={}, writes={}",
                    i,
                    speed,
                    subsong.channel_count(),
                    subsong.register_write_count()
                );
            }
        }
    }
    Ok(())
}

#[cfg(feature = "xmi")]
fn inspect_xmi(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let xmi = cnc_formats::xmi::XmiFile::parse(data)?;
    println!("XMI file (XMIDI / Miles Sound System)");
    println!("  Sequences:  {}", xmi.sequence_count());
    for (i, seq) in xmi.sequences.iter().enumerate() {
        let timbre_count = seq.timbres.len();
        let evnt_len = seq.event_data.len();
        println!("  Sequence {i}: {evnt_len} bytes EVNT, {timbre_count} timbres");
    }
    Ok(())
}

#[cfg(feature = "meg")]
fn inspect_meg(data: &[u8]) -> Result<(), cnc_formats::Error> {
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
