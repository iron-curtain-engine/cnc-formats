use std::sync::OnceLock;

pub(crate) fn csf_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_csf_bytes).as_slice()
}

pub(crate) fn cps_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_cps_bytes).as_slice()
}

pub(crate) fn shp_ts_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_shp_ts_bytes).as_slice()
}

pub(crate) fn vxl_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_vxl_bytes).as_slice()
}

pub(crate) fn hva_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_hva_bytes).as_slice()
}

pub(crate) fn w3d_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_w3d_bytes).as_slice()
}

// ── CSF ──────────────────────────────────────────────────────────────────────

fn build_csf_bytes() -> Vec<u8> {
    let num_labels = 32u32;

    let mut data = Vec::new();
    // Header
    data.extend_from_slice(b" FSC"); // magic
    data.extend_from_slice(&3u32.to_le_bytes()); // version
    data.extend_from_slice(&num_labels.to_le_bytes()); // num_labels
    data.extend_from_slice(&num_labels.to_le_bytes()); // num_strings (1 per label)
    data.extend_from_slice(&0u32.to_le_bytes()); // unused
    data.extend_from_slice(&0u32.to_le_bytes()); // language ID

    for i in 0..num_labels {
        let lbl_name = format!("GUI:Label{i}");
        let value = format!("String value {i}");

        data.extend_from_slice(b" LBL"); // label magic
        data.extend_from_slice(&1u32.to_le_bytes()); // 1 string per label
        data.extend_from_slice(&(lbl_name.len() as u32).to_le_bytes());
        data.extend_from_slice(lbl_name.as_bytes());

        data.extend_from_slice(b" STR");
        let val_utf16: Vec<u16> = value.encode_utf16().collect();
        data.extend_from_slice(&(val_utf16.len() as u32).to_le_bytes());

        // CSF strings are bitwise-inverted UTF-16LE
        for ch in val_utf16 {
            let bytes = ch.to_le_bytes();
            data.push(!bytes[0]);
            data.push(!bytes[1]);
        }
    }

    data
}

// ── CPS ──────────────────────────────────────────────────────────────────────

fn build_cps_bytes() -> Vec<u8> {
    // 320×200 full-screen image with LCW compression
    let pixel_count = 320usize * 200;
    let mut pixels = Vec::with_capacity(pixel_count);
    for i in 0..pixel_count {
        pixels.push(((i * 7 + i / 320 * 3) & 0xFF) as u8);
    }

    let compressed = cnc_formats::lcw::compress(&pixels);
    let total = 10 + compressed.len();
    let file_size = (total.saturating_sub(2)) as u16;

    let mut buf = Vec::with_capacity(total);
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(&4u16.to_le_bytes()); // COMPRESSION_LCW
    buf.extend_from_slice(&(pixel_count as u16).to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes()); // palette_size = 0
    buf.extend_from_slice(&0u16.to_le_bytes()); // unknown
    buf.extend_from_slice(&compressed);
    buf
}

// ── SHP_TS ───────────────────────────────────────────────────────────────────

fn build_shp_ts_bytes() -> Vec<u8> {
    let width: u16 = 48;
    let height: u16 = 32;
    let num_frames: u16 = 8;
    let area = width as usize * height as usize;
    let headers_size = 8 + num_frames as usize * 24; // FILE_HEADER + FRAME_HEADER per frame
    let total = headers_size + num_frames as usize * area;
    let mut buf = Vec::with_capacity(total);

    // File header
    buf.extend_from_slice(&0u16.to_le_bytes()); // zero marker
    buf.extend_from_slice(&width.to_le_bytes());
    buf.extend_from_slice(&height.to_le_bytes());
    buf.extend_from_slice(&num_frames.to_le_bytes());

    // Frame headers
    for i in 0..num_frames as usize {
        let offset = (headers_size + i * area) as u32;
        buf.extend_from_slice(&0u16.to_le_bytes()); // x
        buf.extend_from_slice(&0u16.to_le_bytes()); // y
        buf.extend_from_slice(&width.to_le_bytes()); // cx
        buf.extend_from_slice(&height.to_le_bytes()); // cy
        buf.push(0); // compression = none
        buf.extend_from_slice(&[0u8; 3]); // padding
        buf.extend_from_slice(&0u32.to_le_bytes()); // unknown1
        buf.extend_from_slice(&0u32.to_le_bytes()); // unknown2
        buf.extend_from_slice(&offset.to_le_bytes()); // file_offset
    }

    // Frame pixel data
    for frame in 0..num_frames as usize {
        for pixel in 0..area {
            buf.push(((frame * 17 + pixel * 3) & 0xFF) as u8);
        }
    }

    buf
}

// ── VXL ──────────────────────────────────────────────────────────────────────

fn build_vxl_bytes() -> Vec<u8> {
    let limb_count: u32 = 4;
    let body_bytes: Vec<u8> = (0..256).map(|i| (i & 0xFF) as u8).collect();
    let tailer_count = limb_count;
    let body_size = body_bytes.len() as u32;

    let mut buf = Vec::new();

    // Header (804 bytes)
    buf.extend_from_slice(b"Voxel Animation\0"); // magic (16 bytes)
    buf.extend_from_slice(&1u32.to_le_bytes()); // palette_count
    buf.extend_from_slice(&limb_count.to_le_bytes());
    buf.extend_from_slice(&tailer_count.to_le_bytes());
    buf.extend_from_slice(&body_size.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes()); // start_palette_remap
    buf.extend_from_slice(&0u16.to_le_bytes()); // end_palette_remap
                                                // Palette: 256 × RGB (768 bytes)
    for i in 0..256u16 {
        buf.push((i & 0xFF) as u8);
        buf.push(0);
        buf.push(0);
    }
    // Pad to exactly 804 bytes
    while buf.len() < 804 {
        buf.push(0);
    }

    // Limb headers (28 bytes each)
    for i in 0..limb_count {
        let mut name = [0u8; 16];
        let s = format!("limb_{i}");
        let bytes = s.as_bytes();
        let n = bytes.len().min(15);
        name[..n].copy_from_slice(&bytes[..n]);
        buf.extend_from_slice(&name);
        buf.extend_from_slice(&i.to_le_bytes()); // limb_number
        buf.extend_from_slice(&0u32.to_le_bytes()); // unknown1
        buf.extend_from_slice(&0u32.to_le_bytes()); // unknown2
    }

    // Body data
    buf.extend_from_slice(&body_bytes);

    // Limb tailers (92 bytes each)
    for _ in 0..tailer_count {
        buf.extend_from_slice(&0u32.to_le_bytes()); // span_start_offset
        buf.extend_from_slice(&0u32.to_le_bytes()); // span_end_offset
        buf.extend_from_slice(&0u32.to_le_bytes()); // span_data_offset
        buf.extend_from_slice(&1.0f32.to_le_bytes()); // det
                                                      // Transform: 3×4 identity
        let identity: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        for val in &identity {
            buf.extend_from_slice(&val.to_le_bytes());
        }
        // min_bounds
        for _ in 0..3 {
            buf.extend_from_slice(&(-1.0f32).to_le_bytes());
        }
        // max_bounds
        for _ in 0..3 {
            buf.extend_from_slice(&1.0f32.to_le_bytes());
        }
        buf.push(2); // size_x
        buf.push(2); // size_y
        buf.push(2); // size_z
        buf.push(2); // normals_mode
    }

    buf
}

// ── HVA ──────────────────────────────────────────────────────────────────────

fn build_hva_bytes() -> Vec<u8> {
    let num_sections: u32 = 8;
    let num_frames: u32 = 24;
    let mut buf = Vec::new();

    // Header: filename (16 bytes)
    buf.extend_from_slice(b"bench.hva\0\0\0\0\0\0\0");
    buf.extend_from_slice(&num_frames.to_le_bytes());
    buf.extend_from_slice(&num_sections.to_le_bytes());

    // Section names (16 bytes each)
    for i in 0..num_sections {
        let mut name = [0u8; 16];
        let s = format!("bone_{i}");
        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(15);
        name[..copy_len].copy_from_slice(&bytes[..copy_len]);
        buf.extend_from_slice(&name);
    }

    // Transform matrices: 3×4 identity per frame×section
    for _ in 0..num_frames {
        for _ in 0..num_sections {
            let identity: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
            for val in &identity {
                buf.extend_from_slice(&val.to_le_bytes());
            }
        }
    }

    buf
}

// ── W3D ──────────────────────────────────────────────────────────────────────

fn build_w3d_bytes() -> Vec<u8> {
    let mut buf = Vec::new();

    // Mesh Header chunk (leaf, 116 bytes)
    let mut mesh_header_data = vec![0u8; 116];
    mesh_header_data[0..4].copy_from_slice(&3u32.to_le_bytes()); // version = 3
    mesh_header_data[8..16].copy_from_slice(b"BenchMsh");
    mesh_header_data[24..32].copy_from_slice(b"BenchCnt");
    mesh_header_data[40..44].copy_from_slice(&2u32.to_le_bytes()); // num_tris = 2
    mesh_header_data[44..48].copy_from_slice(&4u32.to_le_bytes()); // num_vertices = 4
    mesh_header_data[48..52].copy_from_slice(&1u32.to_le_bytes()); // num_materials = 1

    let mut mesh_header_chunk = Vec::new();
    write_w3d_chunk_header(&mut mesh_header_chunk, 0x0000_001F, 116, false);
    mesh_header_chunk.extend_from_slice(&mesh_header_data);

    // Vertices chunk (leaf, 4 × 12 = 48 bytes)
    let vertices: [(f32, f32, f32); 4] = [
        (0.0, 0.0, 0.0),
        (1.0, 0.0, 0.0),
        (1.0, 1.0, 0.0),
        (0.0, 1.0, 0.0),
    ];
    let mut vertex_data = Vec::new();
    for (x, y, z) in &vertices {
        vertex_data.extend_from_slice(&x.to_le_bytes());
        vertex_data.extend_from_slice(&y.to_le_bytes());
        vertex_data.extend_from_slice(&z.to_le_bytes());
    }
    let mut vertex_chunk = Vec::new();
    write_w3d_chunk_header(
        &mut vertex_chunk,
        0x0000_0002,
        vertex_data.len() as u32,
        false,
    );
    vertex_chunk.extend_from_slice(&vertex_data);

    // Triangles chunk (leaf, 2 × 32 = 64 bytes)
    let mut tri_data = Vec::new();
    for (v0, v1, v2) in [(0u32, 1u32, 2u32), (0u32, 2u32, 3u32)] {
        tri_data.extend_from_slice(&v0.to_le_bytes());
        tri_data.extend_from_slice(&v1.to_le_bytes());
        tri_data.extend_from_slice(&v2.to_le_bytes());
        tri_data.extend_from_slice(&0u32.to_le_bytes()); // attributes
        tri_data.extend_from_slice(&0.0f32.to_le_bytes()); // nx
        tri_data.extend_from_slice(&0.0f32.to_le_bytes()); // ny
        tri_data.extend_from_slice(&1.0f32.to_le_bytes()); // nz
        tri_data.extend_from_slice(&0.0f32.to_le_bytes()); // d
    }
    let mut tri_chunk = Vec::new();
    write_w3d_chunk_header(&mut tri_chunk, 0x0000_0020, tri_data.len() as u32, false);
    tri_chunk.extend_from_slice(&tri_data);

    // Mesh container chunk
    let mesh_payload_size = mesh_header_chunk.len() + vertex_chunk.len() + tri_chunk.len();
    write_w3d_chunk_header(&mut buf, 0x0000_0000, mesh_payload_size as u32, true);
    buf.extend_from_slice(&mesh_header_chunk);
    buf.extend_from_slice(&vertex_chunk);
    buf.extend_from_slice(&tri_chunk);

    buf
}

fn write_w3d_chunk_header(buf: &mut Vec<u8>, chunk_type: u32, size: u32, has_children: bool) {
    buf.extend_from_slice(&chunk_type.to_le_bytes());
    let size_field = if has_children {
        size | 0x8000_0000
    } else {
        size
    };
    buf.extend_from_slice(&size_field.to_le_bytes());
}
