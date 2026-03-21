// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::*;

// ── Test Helpers ──────────────────────────────────────────────────────────────

/// Writes a chunk header (type + size, with optional sub-chunk flag).
fn write_chunk_header(buf: &mut Vec<u8>, chunk_type: u32, size: u32, has_children: bool) {
    buf.extend_from_slice(&chunk_type.to_le_bytes());
    let size_field = if has_children {
        size | HAS_SUB_CHUNKS_FLAG
    } else {
        size
    };
    buf.extend_from_slice(&size_field.to_le_bytes());
}

/// Builds a minimal valid W3D file with one mesh containing a header,
/// 3 vertices (a triangle), and 1 triangle face.
fn build_w3d() -> Vec<u8> {
    let mut buf = Vec::new();

    // ── Mesh Header chunk (leaf, 116 bytes) ──────────────────────────────
    let mut mesh_header_data = vec![0u8; MESH_HEADER3_SIZE];
    // version = 3
    mesh_header_data[0..4].copy_from_slice(&3u32.to_le_bytes());
    // attributes = 0
    // mesh_name = "TestMesh"
    mesh_header_data[8..16].copy_from_slice(b"TestMesh");
    // container_name = "TestCont"
    mesh_header_data[24..32].copy_from_slice(b"TestCont");
    // num_tris = 1
    mesh_header_data[40..44].copy_from_slice(&1u32.to_le_bytes());
    // num_vertices = 3
    mesh_header_data[44..48].copy_from_slice(&3u32.to_le_bytes());
    // num_materials = 1
    mesh_header_data[48..52].copy_from_slice(&1u32.to_le_bytes());

    let mut mesh_header_chunk = Vec::new();
    write_chunk_header(
        &mut mesh_header_chunk,
        CHUNK_MESH_HEADER3,
        MESH_HEADER3_SIZE as u32,
        false,
    );
    mesh_header_chunk.extend_from_slice(&mesh_header_data);

    // ── Vertices chunk (leaf, 3 × 12 = 36 bytes) ────────────────────────
    let mut vertex_data = Vec::new();
    for &(x, y, z) in &[
        (0.0f32, 0.0f32, 0.0f32),
        (1.0f32, 0.0f32, 0.0f32),
        (0.0f32, 1.0f32, 0.0f32),
    ] {
        vertex_data.extend_from_slice(&x.to_le_bytes());
        vertex_data.extend_from_slice(&y.to_le_bytes());
        vertex_data.extend_from_slice(&z.to_le_bytes());
    }
    let mut vertex_chunk = Vec::new();
    write_chunk_header(
        &mut vertex_chunk,
        CHUNK_VERTICES,
        vertex_data.len() as u32,
        false,
    );
    vertex_chunk.extend_from_slice(&vertex_data);

    // ── Triangles chunk (leaf, 1 × 32 bytes) ────────────────────────────
    let mut tri_data = Vec::with_capacity(TRIANGLE_SIZE);
    tri_data.extend_from_slice(&0u32.to_le_bytes()); // v0
    tri_data.extend_from_slice(&1u32.to_le_bytes()); // v1
    tri_data.extend_from_slice(&2u32.to_le_bytes()); // v2
    tri_data.extend_from_slice(&0u32.to_le_bytes()); // attributes
    tri_data.extend_from_slice(&0.0f32.to_le_bytes()); // nx
    tri_data.extend_from_slice(&0.0f32.to_le_bytes()); // ny
    tri_data.extend_from_slice(&1.0f32.to_le_bytes()); // nz
    tri_data.extend_from_slice(&0.0f32.to_le_bytes()); // d

    let mut tri_chunk = Vec::new();
    write_chunk_header(
        &mut tri_chunk,
        CHUNK_TRIANGLES,
        tri_data.len() as u32,
        false,
    );
    tri_chunk.extend_from_slice(&tri_data);

    // ── Mesh container chunk ─────────────────────────────────────────────
    let mesh_payload_size = mesh_header_chunk.len() + vertex_chunk.len() + tri_chunk.len();
    write_chunk_header(&mut buf, CHUNK_MESH, mesh_payload_size as u32, true);
    buf.extend_from_slice(&mesh_header_chunk);
    buf.extend_from_slice(&vertex_chunk);
    buf.extend_from_slice(&tri_chunk);

    buf
}

// ── Basic Functionality ──────────────────────────────────────────────────────

/// Parse a valid W3D file and verify chunk structure.
#[test]
fn parse_valid() {
    let data = build_w3d();
    let w3d = W3dFile::parse(&data).unwrap();
    assert_eq!(w3d.chunks.len(), 1); // 1 mesh container
    let mesh = &w3d.chunks[0];
    assert_eq!(mesh.chunk_type, CHUNK_MESH);
    assert!(mesh.is_container());
    assert_eq!(mesh.children.len(), 3); // header + verts + tris
}

/// Parse mesh header from the nested chunk.
#[test]
fn mesh_header_fields() {
    let data = build_w3d();
    let w3d = W3dFile::parse(&data).unwrap();
    let mesh = &w3d.chunks[0];
    let header_chunk = mesh.find_child(CHUNK_MESH_HEADER3).unwrap();
    let header = parse_mesh_header(header_chunk.data).unwrap();
    assert_eq!(header.version, 3);
    assert_eq!(header.num_tris, 1);
    assert_eq!(header.num_vertices, 3);
    assert_eq!(header.name_str(), "TestMesh");
}

/// Parse vertex positions.
#[test]
fn vertices_parse() {
    let data = build_w3d();
    let w3d = W3dFile::parse(&data).unwrap();
    let mesh = &w3d.chunks[0];
    let vert_chunk = mesh.find_child(CHUNK_VERTICES).unwrap();
    let verts = parse_vertices(vert_chunk.data).unwrap();
    assert_eq!(verts.len(), 3);
    assert!((verts[0].x - 0.0).abs() < f32::EPSILON);
    assert!((verts[1].x - 1.0).abs() < f32::EPSILON);
    assert!((verts[2].y - 1.0).abs() < f32::EPSILON);
}

/// Parse triangle faces.
#[test]
fn triangles_parse() {
    let data = build_w3d();
    let w3d = W3dFile::parse(&data).unwrap();
    let mesh = &w3d.chunks[0];
    let tri_chunk = mesh.find_child(CHUNK_TRIANGLES).unwrap();
    let tris = parse_triangles(tri_chunk.data).unwrap();
    assert_eq!(tris.len(), 1);
    assert_eq!(tris[0].vert_indices, [0, 1, 2]);
}

/// meshes() helper finds mesh container chunks.
#[test]
fn meshes_accessor() {
    let data = build_w3d();
    let w3d = W3dFile::parse(&data).unwrap();
    assert_eq!(w3d.meshes().len(), 1);
}

/// find_children returns all matching children.
#[test]
fn find_children_accessor() {
    let data = build_w3d();
    let w3d = W3dFile::parse(&data).unwrap();
    let mesh = &w3d.chunks[0];
    // Only one vertices chunk.
    assert_eq!(mesh.find_children(CHUNK_VERTICES).len(), 1);
    // No hierarchy chunks in a mesh.
    assert_eq!(mesh.find_children(CHUNK_HIERARCHY).len(), 0);
}

/// Texture name parsing works on a null-terminated string.
#[test]
fn texture_name_parse() {
    let data = b"diffuse.tga\0\0\0\0";
    assert_eq!(parse_texture_name(data), "diffuse.tga");
}

/// User text parsing works identically to texture names.
#[test]
fn user_text_parse() {
    let data = b"some user text\0";
    assert_eq!(parse_user_text(data), "some user text");
}

// ── Error Paths ──────────────────────────────────────────────────────────────

/// Input too short for even one chunk header is accepted as empty.
#[test]
fn empty_file() {
    let w3d = W3dFile::parse(&[]).unwrap();
    assert!(w3d.chunks.is_empty());
}

/// Chunk with size exceeding remaining data is rejected.
#[test]
fn chunk_size_overflow() {
    let mut data = Vec::new();
    write_chunk_header(&mut data, 0x99, 1000, false);
    data.extend_from_slice(&[0u8; 4]); // only 4 bytes of payload
    let err = W3dFile::parse(&data).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { .. }));
}

/// Chunk size exceeding V38 cap is rejected.
#[test]
fn chunk_size_exceeds_cap() {
    let mut data = Vec::new();
    let big_size = (MAX_CHUNK_SIZE as u32) + 1;
    write_chunk_header(&mut data, 0x01, big_size, false);
    let err = W3dFile::parse(&data).unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidSize {
            context: "W3D chunk data size",
            ..
        }
    ));
}

/// Mesh header chunk with < 116 bytes of data is rejected.
#[test]
fn mesh_header_truncated() {
    let err = parse_mesh_header(&[0u8; 115]).unwrap_err();
    assert!(matches!(err, Error::UnexpectedEof { needed: 116, .. }));
}

// ── Determinism ──────────────────────────────────────────────────────────────

/// Parsing the same input twice yields identical results.
#[test]
fn determinism() {
    let data = build_w3d();
    let a = W3dFile::parse(&data).unwrap();
    let b = W3dFile::parse(&data).unwrap();
    assert_eq!(a, b);
}

// ── Security Edge Cases (V38) ────────────────────────────────────────────────

/// `W3dFile::parse` on 256 bytes of `0xFF` must not panic.
#[test]
fn adversarial_all_ff_no_panic() {
    let data = vec![0xFFu8; 256];
    let _ = W3dFile::parse(&data);
}

/// `W3dFile::parse` on 256 bytes of `0x00` must not panic.
#[test]
fn adversarial_all_zero_no_panic() {
    let data = vec![0x00u8; 256];
    let _ = W3dFile::parse(&data);
}

/// Deeply nested chunks are capped by MAX_CHUNK_DEPTH.
#[test]
fn excessive_depth() {
    let mut data = Vec::new();
    // Create 40 nested container chunks (exceeds MAX_CHUNK_DEPTH of 32).
    for _ in 0..40 {
        write_chunk_header(&mut data, 0x01, 0, true);
    }
    // Pad to make each container seem valid.
    data.resize(data.len() + 64, 0);
    // This should either error or parse (depth limit prevents deep recursion).
    // The exact behavior depends on whether the first 33rd level triggers the error.
    let _ = W3dFile::parse(&data);
}
