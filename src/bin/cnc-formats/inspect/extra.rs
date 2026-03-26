// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Additional format inspectors (first half) — split from `inspect.rs` to keep
//! file sizes within the RAG/LLM friendliness target of ≤600 lines.
//!
//! Handles: AVI, VXL, HVA, SHP-TS, CSF, CPS, W3D, TS-TMP, MiniYAML, MIDI,
//! ADL, XMI, VOC, PAK.
//!
//! See [`super::extra2`] for the second half (SHP-D2 through MEG).

#[cfg(feature = "convert")]
pub(super) fn inspect_avi(data: &[u8]) -> Result<(), cnc_formats::Error> {
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

pub(super) fn inspect_vxl(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let vxl = cnc_formats::vxl::VxlFile::parse(data)?;
    let h = &vxl.header;
    println!("VXL voxel model");
    println!("  Limbs:       {}", h.limb_count);
    println!("  Tailers:     {}", h.tailer_count);
    println!("  Body size:   {} bytes", h.body_size);
    println!("  Palette:     {} entries", h.palette.len());
    for (i, lh) in vxl.limb_headers.iter().enumerate() {
        println!("  Limb {:>2}: \"{}\"", i, lh.name_str());
    }
    for (i, lt) in vxl.limb_tailers.iter().enumerate() {
        println!(
            "  Tailer {:>2}: {}x{}x{} (normals mode {})",
            i, lt.size_x, lt.size_y, lt.size_z, lt.normals_mode
        );
    }
    Ok(())
}

pub(super) fn inspect_hva(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let hva = cnc_formats::hva::HvaFile::parse(data)?;
    let h = &hva.header;
    let filename = String::from_utf8_lossy(
        h.filename
            .get(..h.filename.iter().position(|&b| b == 0).unwrap_or(16))
            .unwrap_or(&h.filename),
    );
    println!("HVA voxel animation");
    println!("  Filename:    {filename}");
    println!("  Frames:      {}", h.num_frames);
    println!("  Sections:    {}", h.num_sections);
    println!("  Transforms:  {}", hva.transforms.len());
    for (i, name) in hva.section_names.iter().enumerate() {
        let name_str = String::from_utf8_lossy(
            name.get(..name.iter().position(|&b| b == 0).unwrap_or(16))
                .unwrap_or(name),
        );
        println!("  Section {:>2}: \"{name_str}\"", i);
    }
    Ok(())
}

pub(super) fn inspect_shp_ts(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let shp = cnc_formats::shp_ts::ShpTsFile::parse(data)?;
    let h = &shp.header;
    println!("SHP sprite (Tiberian Sun / Red Alert 2)");
    println!("  Dimensions: {}x{}", h.width, h.height);
    println!("  Frames:     {}", h.num_frames);
    for (i, frame) in shp.frames.iter().enumerate() {
        let comp = match frame.header.compression {
            0 => "raw",
            1 => "scanline RLE",
            2 => "scanline RLE v2",
            3 => "LCW",
            _ => "unknown",
        };
        println!(
            "  Frame {:>4}: crop({}x{} @ {},{}), {comp}",
            i, frame.header.cx, frame.header.cy, frame.header.x, frame.header.y
        );
    }
    Ok(())
}

pub(super) fn inspect_csf(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let csf = cnc_formats::csf::CsfFile::parse(data)?;
    println!("CSF string table");
    println!("  Version:    {}", csf.version);
    println!("  Language:   {}", csf.language);
    println!("  Labels:     {}", csf.labels.len());
    for (label, strings) in csf.labels.iter().take(8) {
        let first = strings.first();
        let preview = match first {
            Some(s) if s.value.len() > 60 => {
                format!("{}...", s.value.get(..60).unwrap_or(&s.value))
            }
            Some(s) => s.value.clone(),
            None => "<empty>".to_string(),
        };
        let count_tag = if strings.len() > 1 {
            format!(" [{} variants]", strings.len())
        } else {
            String::new()
        };
        println!("  {label}: \"{preview}\"{count_tag}");
    }
    if csf.labels.len() > 8 {
        println!("  ... ({} more labels)", csf.labels.len() - 8);
    }
    Ok(())
}

pub(super) fn inspect_cps(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let cps = cnc_formats::cps::CpsFile::parse(data)?;
    let h = &cps.header;
    let comp = match h.compression {
        cnc_formats::cps::COMPRESSION_LCW => "LCW",
        cnc_formats::cps::COMPRESSION_NONE => "none",
        _ => "unknown",
    };
    println!("CPS compressed screen picture");
    println!(
        "  Dimensions:   {}x{}",
        cnc_formats::cps::CPS_WIDTH,
        cnc_formats::cps::CPS_HEIGHT
    );
    println!("  Buffer size:  {} bytes", h.buffer_size);
    println!("  Compression:  {comp} ({})", h.compression);
    println!(
        "  Palette:      {}",
        if cps.palette.is_some() {
            "embedded"
        } else {
            "external"
        }
    );
    println!("  Pixels:       {} bytes", cps.pixels.len());
    Ok(())
}

pub(super) fn inspect_w3d(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let w3d = cnc_formats::w3d::W3dFile::parse(data)?;
    println!("W3D 3D model (Westwood/SAGE)");
    println!("  Top-level chunks: {}", w3d.chunks.len());
    println!("  Meshes:           {}", w3d.meshes().len());
    for (i, chunk) in w3d.chunks.iter().enumerate() {
        let kind = if chunk.is_container() {
            "container"
        } else {
            "leaf"
        };
        println!(
            "  Chunk {:>2}: type 0x{:04X}, {} bytes ({}), {} children",
            i,
            chunk.chunk_type,
            chunk.data.len(),
            kind,
            chunk.children.len()
        );
    }
    Ok(())
}

pub(super) fn inspect_ts_tmp(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let tmp = cnc_formats::tmp::TsTmpFile::parse(data)?;
    let h = &tmp.header;
    let present = tmp.tiles.iter().filter(|t| t.is_some()).count();
    let grid = (h.tiles_x as usize).saturating_mul(h.tiles_y as usize);
    println!("TMP terrain (Tiberian Sun / Red Alert 2 isometric)");
    println!("  Tile size: {}x{} px", h.tile_width, h.tile_height);
    println!("  Grid:      {}x{} ({} cells)", h.tiles_x, h.tiles_y, grid);
    println!("  Present:   {} tiles", present);
    for tile in tmp.tiles.iter().flatten() {
        let extras = if tile.extra_pixels.is_some() {
            " +extra"
        } else {
            ""
        };
        let zdata = if tile.z_data.is_some() { " +Z" } else { "" };
        println!(
            "  Tile ({},{}): h={}, terrain={}, ramp={}{extras}{zdata}",
            tile.col, tile.row, tile.header.height, tile.header.terrain_type, tile.header.ramp_type
        );
    }
    Ok(())
}

#[cfg(feature = "miniyaml")]
pub(super) fn inspect_miniyaml(data: &[u8]) -> Result<(), cnc_formats::Error> {
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
pub(super) fn inspect_mid(data: &[u8]) -> Result<(), cnc_formats::Error> {
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
pub(super) fn inspect_adl(data: &[u8]) -> Result<(), cnc_formats::Error> {
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
pub(super) fn inspect_xmi(data: &[u8]) -> Result<(), cnc_formats::Error> {
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

pub(super) fn inspect_voc(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let voc = cnc_formats::voc::VocFile::parse(data)?;
    let (major, minor) = voc.version();
    println!("VOC audio (Creative Voice File)");
    println!("  Version:  {major}.{minor}");
    println!("  Blocks:   {}", voc.blocks().len());
    for (i, block) in voc.blocks().iter().enumerate() {
        let kind = match block.block_type {
            0 => "terminator",
            1 => "sound data",
            2 => "sound continue",
            3 => "silence",
            4 => "marker",
            5 => "text",
            6 => "repeat start",
            7 => "repeat end",
            8 => "extended",
            9 => "new sound",
            _ => "unknown",
        };
        println!("  Block {:>3}: type {} ({kind})", i, block.block_type);
    }
    Ok(())
}

pub(super) fn inspect_pak(data: &[u8]) -> Result<(), cnc_formats::Error> {
    let archive = cnc_formats::pak::PakArchive::parse(data)?;
    let entries = archive.entries();
    println!("PAK archive (Dune II)");
    println!("  Entries: {}", entries.len());
    let total: usize = entries.iter().map(|e| e.size).sum();
    println!("  Total:   {} bytes", total);
    println!();
    println!("  {:>10}  {:>10}  Name", "Offset", "Size");
    for entry in entries {
        println!("  {:>10}  {:>10}  {}", entry.offset, entry.size, entry.name);
    }
    Ok(())
}
