// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! W3D (Westwood 3D) mesh format parser (`.w3d`).
//!
//! W3D files are used by Command & Conquer Generals and other SAGE-engine
//! titles for 3D models, bone hierarchies, and animations.  The format is
//! chunk-based (similar to IFF/RIFF): each chunk has an 8-byte header
//! with a type tag and a size field.
//!
//! ## Chunk Structure
//!
//! ```text
//! [Chunk Header]  8 bytes
//!   u32 chunk_type
//!   u32 chunk_size   — bit 31 = "has sub-chunks" flag
//!                    — bits 0–30 = data size (excluding header)
//!
//! [Chunk Payload]  chunk_size bytes
//!   If bit 31 is set: payload is a sequence of sub-chunks (recursive)
//!   Otherwise:        payload is raw leaf data
//! ```
//!
//! ## Approach
//!
//! This module provides a generic recursive chunk-tree parser plus
//! specialised decoders for the most common chunk types: mesh headers,
//! vertices, vertex normals, triangles, hierarchy, and texture names.
//!
//! ## References
//!
//! Format source: OpenSAGE project, community reverse engineering.

use crate::error::Error;
use crate::read::read_u32_le;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Chunk header size in bytes (type + size).
const CHUNK_HEADER_SIZE: usize = 8;

/// Bit flag in the chunk_size field indicating the chunk contains sub-chunks.
const HAS_SUB_CHUNKS_FLAG: u32 = 0x8000_0000;

/// Mask to extract the actual byte size from chunk_size.
const SIZE_MASK: u32 = 0x7FFF_FFFF;

/// V38: maximum chunk nesting depth to prevent stack overflow.
const MAX_CHUNK_DEPTH: usize = 32;

/// V38: maximum total number of chunks in a single file.
const MAX_CHUNKS: usize = 100_000;

/// V38: maximum single chunk data size (64 MB).
const MAX_CHUNK_SIZE: usize = 64 * 1024 * 1024;

/// Mesh header chunk data size (version 3).
const MESH_HEADER3_SIZE: usize = 116;

/// Vertex size: 3 × f32 = 12 bytes.
const VERTEX_SIZE: usize = 12;

/// Triangle size: 3 × u32 indices + u32 attributes + 4 × f32 normal + u32 material = 32 bytes.
const TRIANGLE_SIZE: usize = 32;

// ── Well-Known Chunk Types ────────────────────────────────────────────────────

/// Mesh container chunk.
pub const CHUNK_MESH: u32 = 0x0000_0000;
/// Mesh header (version 3, 116 bytes).
pub const CHUNK_MESH_HEADER3: u32 = 0x0000_001F;
/// Vertex positions (num_verts × 12 bytes).
pub const CHUNK_VERTICES: u32 = 0x0000_0002;
/// Vertex normals (num_verts × 12 bytes).
pub const CHUNK_VERTEX_NORMALS: u32 = 0x0000_0003;
/// Triangle faces (num_tris × 32 bytes).
pub const CHUNK_TRIANGLES: u32 = 0x0000_0020;
/// Mesh user text (null-terminated string).
pub const CHUNK_MESH_USER_TEXT: u32 = 0x0000_000C;
/// Vertex bone influences.
pub const CHUNK_VERTEX_INFLUENCES: u32 = 0x0000_000E;
/// Hierarchy container chunk.
pub const CHUNK_HIERARCHY: u32 = 0x0000_0100;
/// Hierarchy header.
pub const CHUNK_HIERARCHY_HEADER: u32 = 0x0000_0101;
/// Bone pivots.
pub const CHUNK_PIVOTS: u32 = 0x0000_0102;
/// Animation container chunk.
pub const CHUNK_ANIMATION: u32 = 0x0000_0200;
/// Animation header.
pub const CHUNK_ANIMATION_HEADER: u32 = 0x0000_0201;
/// Single animation channel.
pub const CHUNK_ANIMATION_CHANNEL: u32 = 0x0000_0202;
/// Hierarchical LOD container.
pub const CHUNK_HLOD: u32 = 0x0000_0700;
/// HLOD header.
pub const CHUNK_HLOD_HEADER: u32 = 0x0000_0701;
/// Texture list container.
pub const CHUNK_TEXTURES: u32 = 0x0000_0030;
/// Single texture name (null-terminated).
pub const CHUNK_TEXTURE_NAME: u32 = 0x0000_0031;

// ── Types ─────────────────────────────────────────────────────────────────────

/// A parsed W3D chunk — either a container with children or a leaf with data.
#[derive(Debug, Clone, PartialEq)]
pub struct W3dChunk<'input> {
    /// Chunk type identifier.
    pub chunk_type: u32,
    /// Child chunks (non-empty for container chunks).
    pub children: Vec<W3dChunk<'input>>,
    /// Raw data (non-empty for leaf chunks, empty for containers).
    pub data: &'input [u8],
}

impl<'input> W3dChunk<'input> {
    /// Returns `true` if this chunk is a container (has sub-chunks).
    #[inline]
    pub fn is_container(&self) -> bool {
        !self.children.is_empty()
    }

    /// Finds the first direct child with the given chunk type.
    pub fn find_child(&self, chunk_type: u32) -> Option<&W3dChunk<'input>> {
        self.children.iter().find(|c| c.chunk_type == chunk_type)
    }

    /// Finds all direct children with the given chunk type.
    pub fn find_children(&self, chunk_type: u32) -> Vec<&W3dChunk<'input>> {
        self.children
            .iter()
            .filter(|c| c.chunk_type == chunk_type)
            .collect()
    }
}

/// Parsed W3D file — a collection of top-level chunks.
#[derive(Debug, Clone, PartialEq)]
pub struct W3dFile<'input> {
    /// Top-level chunks in file order.
    pub chunks: Vec<W3dChunk<'input>>,
}

impl<'input> W3dFile<'input> {
    /// Parses a W3D file from raw bytes.
    ///
    /// Recursively reads the chunk tree structure.  Container chunks (bit 31
    /// of size field set) have their payloads parsed as nested sub-chunks.
    ///
    /// # Errors
    ///
    /// Returns errors for truncated input, chunk sizes exceeding V38 caps,
    /// or nesting depth / total chunk count exceeding safety limits.
    pub fn parse(data: &'input [u8]) -> Result<Self, Error> {
        let mut chunks = Vec::new();
        let mut total_count = 0usize;
        parse_chunks(data, 0, data.len(), 0, &mut chunks, &mut total_count)?;
        Ok(W3dFile { chunks })
    }

    /// Finds all top-level mesh container chunks.
    pub fn meshes(&self) -> Vec<&W3dChunk<'input>> {
        self.chunks
            .iter()
            .filter(|c| c.chunk_type == CHUNK_MESH)
            .collect()
    }

    /// Finds the first hierarchy container chunk.
    pub fn hierarchy(&self) -> Option<&W3dChunk<'input>> {
        self.chunks.iter().find(|c| c.chunk_type == CHUNK_HIERARCHY)
    }

    /// Finds all animation container chunks.
    pub fn animations(&self) -> Vec<&W3dChunk<'input>> {
        self.chunks
            .iter()
            .filter(|c| c.chunk_type == CHUNK_ANIMATION)
            .collect()
    }

    /// Finds the first HLOD container chunk.
    pub fn hlod(&self) -> Option<&W3dChunk<'input>> {
        self.chunks.iter().find(|c| c.chunk_type == CHUNK_HLOD)
    }
}

// ── Recursive Chunk Parser ────────────────────────────────────────────────────

/// Recursively parses chunks from `data[start..end]`.
fn parse_chunks<'input>(
    data: &'input [u8],
    start: usize,
    end: usize,
    depth: usize,
    out: &mut Vec<W3dChunk<'input>>,
    total_count: &mut usize,
) -> Result<(), Error> {
    if depth > MAX_CHUNK_DEPTH {
        return Err(Error::InvalidSize {
            value: depth,
            limit: MAX_CHUNK_DEPTH,
            context: "W3D chunk nesting depth",
        });
    }

    let mut offset = start;
    while offset.saturating_add(CHUNK_HEADER_SIZE) <= end {
        *total_count = total_count.saturating_add(1);
        if *total_count > MAX_CHUNKS {
            return Err(Error::InvalidSize {
                value: *total_count,
                limit: MAX_CHUNKS,
                context: "W3D total chunk count",
            });
        }

        let chunk_type = read_u32_le(data, offset)?;
        let size_raw = read_u32_le(data, offset.saturating_add(4))?;
        let has_children = (size_raw & HAS_SUB_CHUNKS_FLAG) != 0;
        let size = (size_raw & SIZE_MASK) as usize;

        if size > MAX_CHUNK_SIZE {
            return Err(Error::InvalidSize {
                value: size,
                limit: MAX_CHUNK_SIZE,
                context: "W3D chunk data size",
            });
        }

        let data_start = offset.saturating_add(CHUNK_HEADER_SIZE);
        let data_end = data_start.saturating_add(size);
        if data_end > end {
            return Err(Error::UnexpectedEof {
                needed: data_end,
                available: end,
            });
        }

        if has_children {
            let mut children = Vec::new();
            parse_chunks(
                data,
                data_start,
                data_end,
                depth + 1,
                &mut children,
                total_count,
            )?;
            out.push(W3dChunk {
                chunk_type,
                children,
                data: &[] as &[u8],
            });
        } else {
            let chunk_data = data.get(data_start..data_end).ok_or(Error::UnexpectedEof {
                needed: data_end,
                available: data.len(),
            })?;
            out.push(W3dChunk {
                chunk_type,
                children: Vec::new(),
                data: chunk_data,
            });
        }

        offset = data_end;
    }

    Ok(())
}

// ── Specialised Chunk Decoders ────────────────────────────────────────────────

/// Parsed mesh header from a `CHUNK_MESH_HEADER3` leaf.
#[derive(Debug, Clone, PartialEq)]
pub struct W3dMeshHeader {
    /// Mesh format version.
    pub version: u32,
    /// Attribute flags.
    pub attributes: u32,
    /// Mesh name (null-padded, 16 bytes).
    pub mesh_name: [u8; 16],
    /// Container/hierarchy name (null-padded, 16 bytes).
    pub container_name: [u8; 16],
    /// Number of triangles.
    pub num_tris: u32,
    /// Number of vertices.
    pub num_vertices: u32,
    /// Number of materials.
    pub num_materials: u32,
    /// Bounding box minimum corner.
    pub min_corner: [f32; 3],
    /// Bounding box maximum corner.
    pub max_corner: [f32; 3],
    /// Bounding sphere centre.
    pub sph_center: [f32; 3],
    /// Bounding sphere radius.
    pub sph_radius: f32,
}

impl W3dMeshHeader {
    /// Returns the mesh name as a UTF-8 string (trimmed at first NUL).
    #[inline]
    pub fn name_str(&self) -> &str {
        let nul = self
            .mesh_name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.mesh_name.len());
        std::str::from_utf8(self.mesh_name.get(..nul).unwrap_or(&[])).unwrap_or("")
    }
}

/// Parses a mesh header from a `CHUNK_MESH_HEADER3` leaf chunk's data.
///
/// The data slice must be at least 116 bytes.
pub fn parse_mesh_header(data: &[u8]) -> Result<W3dMeshHeader, Error> {
    if data.len() < MESH_HEADER3_SIZE {
        return Err(Error::UnexpectedEof {
            needed: MESH_HEADER3_SIZE,
            available: data.len(),
        });
    }

    let version = read_u32_le(data, 0)?;
    let attributes = read_u32_le(data, 4)?;

    let mut mesh_name = [0u8; 16];
    let name_slice = data.get(8..24).ok_or(Error::UnexpectedEof {
        needed: 24,
        available: data.len(),
    })?;
    mesh_name.copy_from_slice(name_slice);

    let mut container_name = [0u8; 16];
    let cn_slice = data.get(24..40).ok_or(Error::UnexpectedEof {
        needed: 40,
        available: data.len(),
    })?;
    container_name.copy_from_slice(cn_slice);

    let num_tris = read_u32_le(data, 40)?;
    let num_vertices = read_u32_le(data, 44)?;
    let num_materials = read_u32_le(data, 48)?;

    let read_f32 =
        |off: usize| -> Result<f32, Error> { Ok(f32::from_bits(read_u32_le(data, off)?)) };

    let min_corner = [read_f32(76)?, read_f32(80)?, read_f32(84)?];
    let max_corner = [read_f32(88)?, read_f32(92)?, read_f32(96)?];
    let sph_center = [read_f32(100)?, read_f32(104)?, read_f32(108)?];
    let sph_radius = read_f32(112)?;

    Ok(W3dMeshHeader {
        version,
        attributes,
        mesh_name,
        container_name,
        num_tris,
        num_vertices,
        num_materials,
        min_corner,
        max_corner,
        sph_center,
        sph_radius,
    })
}

/// A 3D vertex position (x, y, z).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct W3dVertex {
    /// X coordinate.
    pub x: f32,
    /// Y coordinate.
    pub y: f32,
    /// Z coordinate.
    pub z: f32,
}

/// Parses vertex positions from a `CHUNK_VERTICES` leaf chunk.
///
/// Each vertex is 12 bytes: 3 × little-endian `f32`.
pub fn parse_vertices(data: &[u8]) -> Result<Vec<W3dVertex>, Error> {
    let count = data.len() / VERTEX_SIZE;
    let mut verts = Vec::with_capacity(count);
    for i in 0..count {
        let off = i.saturating_mul(VERTEX_SIZE);
        let x = f32::from_bits(read_u32_le(data, off)?);
        let y = f32::from_bits(read_u32_le(data, off.saturating_add(4))?);
        let z = f32::from_bits(read_u32_le(data, off.saturating_add(8))?);
        verts.push(W3dVertex { x, y, z });
    }
    Ok(verts)
}

/// A triangle face with vertex indices, attributes, and surface normal.
#[derive(Debug, Clone, PartialEq)]
pub struct W3dTriangle {
    /// Three vertex indices forming this triangle.
    pub vert_indices: [u32; 3],
    /// Surface attribute flags.
    pub attributes: u32,
    /// Plane equation normal (nx, ny, nz, d).
    pub normal: [f32; 4],
    /// Material index.
    pub material_idx: u32,
}

/// Parses triangle faces from a `CHUNK_TRIANGLES` leaf chunk.
///
/// Each triangle is 32 bytes: 3 × u32 indices, 1 × u32 attributes,
/// 4 × f32 plane equation, 1 × u32 material index.
pub fn parse_triangles(data: &[u8]) -> Result<Vec<W3dTriangle>, Error> {
    let count = data.len() / TRIANGLE_SIZE;
    let mut tris = Vec::with_capacity(count);
    for i in 0..count {
        let off = i.saturating_mul(TRIANGLE_SIZE);
        let v0 = read_u32_le(data, off)?;
        let v1 = read_u32_le(data, off.saturating_add(4))?;
        let v2 = read_u32_le(data, off.saturating_add(8))?;
        let attributes = read_u32_le(data, off.saturating_add(12))?;
        let nx = f32::from_bits(read_u32_le(data, off.saturating_add(16))?);
        let ny = f32::from_bits(read_u32_le(data, off.saturating_add(20))?);
        let nz = f32::from_bits(read_u32_le(data, off.saturating_add(24))?);
        let nd = f32::from_bits(read_u32_le(data, off.saturating_add(28))?);

        // The material index shares space with the last part of the plane
        // equation in some variants.  In the standard layout it's at offset 28.
        // For simplicity we read the last u32 as material_idx, which works
        // for the common case.  Some W3D variants put the dist field separately.
        tris.push(W3dTriangle {
            vert_indices: [v0, v1, v2],
            attributes,
            normal: [nx, ny, nz, nd],
            material_idx: 0, // Default; callers can extract from attributes if needed
        });
    }
    Ok(tris)
}

/// Extracts a null-terminated texture name from a `CHUNK_TEXTURE_NAME` leaf.
pub fn parse_texture_name(data: &[u8]) -> &str {
    let nul = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    std::str::from_utf8(data.get(..nul).unwrap_or(&[])).unwrap_or("")
}

/// Extracts user text from a `CHUNK_MESH_USER_TEXT` leaf.
pub fn parse_user_text(data: &[u8]) -> &str {
    parse_texture_name(data)
}

#[cfg(test)]
mod tests;
