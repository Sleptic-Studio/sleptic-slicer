// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{
    error::{LoadStlError as E, WriteStlError},
    scene_writer::{WriteSceneProgress, tick},
};
use rayon::prelude::*;
use sc_mesh_core::{
    IndexedMesh, IndexedTriangle, MeshMetadata, Normal, Triangle, Vertex, indexed_mesh_from_soup,
};
use sc_scene::Scene;
use std::io::{Read, Seek, Write};
/// Something an STL file had that was read past.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadStlWarning {
    /// A binary file's 80-byte header is not text, so it names nothing.
    HeaderNotText,
}

fn input(r: &mut (impl Read + Seek)) -> Result<Vec<u8>, E> {
    let current = r.stream_position()?;
    let end = r.seek(std::io::SeekFrom::End(0))?;
    r.seek(std::io::SeekFrom::Start(current))?;
    let mut b = Vec::with_capacity(usize::try_from(end.saturating_sub(current)).unwrap_or(0));
    r.read_to_end(&mut b)?;
    Ok(b)
}
fn text(b: &[u8]) -> Result<&str, E> {
    std::str::from_utf8(b).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        )
        .into()
    })
}
fn header(b: &[u8]) -> Option<Option<String>> {
    let s = std::str::from_utf8(b).ok()?.trim_start_matches([' ', '\t']);
    if s.len() < 5 || !s.as_bytes()[..5].eq_ignore_ascii_case(b"solid") {
        return None;
    }
    if s.len() > 5 && !s.as_bytes()[5].is_ascii_whitespace() {
        return None;
    }
    let name = s[5..].trim();
    Some(if name.is_empty() {
        None
    } else {
        Some(name.into())
    })
}
fn build(triangles: Vec<Triangle>, name: Option<String>) -> Result<Scene, E> {
    let meta = Some(MeshMetadata {
        name,
        header_comment: None,
    });
    if triangles.len() < 8192 {
        return Ok(indexed_mesh_from_soup(meta, triangles, u32::MAX as usize)?.into());
    }
    // First deduplicate independent contiguous blocks. Their much smaller
    // vertex tables then merge in file order, retaining first-occurrence bits
    // and indices while moving most corner lookup work onto the workers.
    let blocks: Vec<_> = triangles
        .par_chunks(16384)
        .map(|block| indexed_mesh_from_soup(None, block.to_vec(), u32::MAX as usize))
        .collect();
    let count = triangles.len();
    drop(triangles);
    let blocks = blocks.into_iter().collect::<Result<Vec<_>, _>>()?;
    merge_blocks(blocks, meta, count)
}
fn merge_blocks(
    blocks: Vec<IndexedMesh>,
    meta: Option<MeshMetadata>,
    count: usize,
) -> Result<Scene, E> {
    let mut vertices = Vec::with_capacity(count / 2);
    let mut faces = Vec::with_capacity(count);
    let mut lookup = std::collections::HashMap::with_capacity_and_hasher(
        vertices.capacity(),
        std::hash::BuildHasherDefault::<PositionHasher>::default(),
    );
    for block in blocks {
        let mut remap = Vec::with_capacity(block.vertices.len());
        for v in block.vertices {
            let key = v.coords().map(|v| if v == 0. { 0 } else { v.to_bits() });
            let next = vertices.len();
            let index = match lookup.entry(key) {
                std::collections::hash_map::Entry::Occupied(e) => *e.get(),
                std::collections::hash_map::Entry::Vacant(e) => {
                    let index = u32::try_from(next).map_err(|_| {
                        sc_mesh_core::IndexedMeshBuildError::TooManyVertices {
                            limit: u32::MAX as usize,
                        }
                    })?;
                    if next == u32::MAX as usize {
                        return Err(sc_mesh_core::IndexedMeshBuildError::TooManyVertices {
                            limit: u32::MAX as usize,
                        }
                        .into());
                    }
                    vertices.push(v);
                    e.insert(index);
                    index
                }
            };
            remap.push(index);
        }
        for f in block.faces {
            faces.push(IndexedTriangle::from_raw_indices(
                f.normal,
                f.vertices.map(|i| remap[i.get() as usize]),
            ));
        }
    }
    Ok(IndexedMesh::new(meta, vertices, faces).into())
}
#[derive(Default)]
struct PositionHasher(u64);
impl std::hash::Hasher for PositionHasher {
    fn write_usize(&mut self, _: usize) {}
    fn write(&mut self, bytes: &[u8]) {
        for word in bytes.as_chunks::<4>().0 {
            self.0 = self.0.rotate_left(21)
                ^ (u32::from_ne_bytes(*word) as u64).wrapping_mul(0x9e3779b185ebca87);
        }
    }
    fn finish(&self) -> u64 {
        let h = (self.0 ^ (self.0 >> 29)).wrapping_mul(0xbf58476d1ce4e5b9);
        h ^ (h >> 32)
    }
}
fn normal(n: Normal, v: &[Vertex; 3]) -> Normal {
    if n.coords().iter().all(|x| *x == 0.) {
        sc_mesh_core::geometry::compute_normal_from_triangle_vertices(v)
            .unwrap_or(Normal::new([0.; 3]))
    } else {
        n
    }
}
pub fn read_stl(r: &mut (impl Read + Seek)) -> Result<(Scene, Vec<LoadStlWarning>), E> {
    let b = input(r)?;
    let exact = b.len() >= 84
        && b.len() as u64 == 84 + 50 * u32::from_le_bytes([b[80], b[81], b[82], b[83]]) as u64;
    if !exact && header(b.split(|c| *c == b'\n').next().unwrap_or(&[])).is_some() {
        Ok((ascii(&b)?, Vec::new()))
    } else {
        binary(&b)
    }
}
pub fn read_stla(r: &mut (impl Read + Seek)) -> Result<Scene, E> {
    ascii(&input(r)?)
}
pub fn read_stlb(r: &mut (impl Read + Seek)) -> Result<(Scene, Vec<LoadStlWarning>), E> {
    binary(&input(r)?)
}
fn binary(b: &[u8]) -> Result<(Scene, Vec<LoadStlWarning>), E> {
    if b.len() < 80 {
        return Err(E::Truncated {
            what: "the 80-byte STL header",
        });
    }
    if b.len() < 84 {
        return Err(E::Truncated {
            what: "the triangle count",
        });
    }
    let count = u32::from_le_bytes([b[80], b[81], b[82], b[83]]) as usize;
    if count > crate::MAX_MESH_ELEMENT_COUNT {
        return Err(E::DeclaredTriangleCountExceedsCap {
            declared: count,
            cap: crate::MAX_MESH_ELEMENT_COUNT,
        });
    }
    let h = &b[..b[..80].iter().position(|v| *v == 0).unwrap_or(80)];
    let mut warnings = Vec::new();
    let name = match std::str::from_utf8(h) {
        Ok(s) if !s.trim().chars().any(char::is_control) => {
            let s = s.trim();
            if s.is_empty() { None } else { Some(s.into()) }
        }
        _ => {
            warnings.push(LoadStlWarning::HeaderNotText);
            None
        }
    };
    let available = ((b.len() - 84) / 50).min(count);
    let parse = |record: &[u8]| -> Result<Triangle, E> {
        const WHAT: [&str; 12] = [
            "normal.x", "normal.y", "normal.z", "v0.x", "v0.y", "v0.z", "v1.x", "v1.y", "v1.z",
            "v2.x", "v2.y", "v2.z",
        ];
        let mut a = [0.; 12];
        for (i, c) in record[..48].as_chunks::<4>().0.iter().enumerate() {
            a[i] = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            if !a[i].is_finite() {
                return Err(E::NonFiniteFloat {
                    what: WHAT[i],
                    value: a[i],
                });
            }
        }
        let v = [
            Vertex::new([a[3], a[4], a[5]]),
            Vertex::new([a[6], a[7], a[8]]),
            Vertex::new([a[9], a[10], a[11]]),
        ];
        Ok(Triangle::new(
            normal(Normal::new([a[0], a[1], a[2]]), &v),
            v,
        ))
    };
    let records = &b[84..84 + available * 50];
    let mesh = if available >= 8192 {
        // Parsing and local indexing share a worker-local triangle buffer.
        // Ordered collection keeps the earliest record error deterministic.
        let blocks: Vec<Result<IndexedMesh, E>> = records
            .par_chunks(50 * 16384)
            .map(|block| {
                let triangles = block
                    .as_chunks::<50>()
                    .0
                    .iter()
                    .map(|r| parse(r))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(indexed_mesh_from_soup(None, triangles, u32::MAX as usize)?)
            })
            .collect();
        let blocks = blocks.into_iter().collect::<Result<Vec<_>, _>>()?;
        if available < count {
            return Err(E::Truncated {
                what: "a triangle record",
            });
        }
        merge_blocks(
            blocks,
            Some(MeshMetadata {
                name,
                header_comment: None,
            }),
            available,
        )?
    } else {
        let triangles = records
            .as_chunks::<50>()
            .0
            .iter()
            .map(|r| parse(r))
            .collect::<Result<Vec<_>, _>>()?;
        if available < count {
            return Err(E::Truncated {
                what: "a triangle record",
            });
        }
        build(triangles, name)?
    };
    Ok((mesh, warnings))
}
struct Lines<'a> {
    lines: std::slice::Split<'a, u8, fn(&u8) -> bool>,
}
impl<'a> Lines<'a> {
    fn next(&mut self, what: &'static str) -> Result<Vec<&'a str>, E> {
        for b in self.lines.by_ref() {
            let s = text(b)?;
            let t: Vec<_> = s.split_whitespace().collect();
            if !t.is_empty() {
                return Ok(t);
            }
        }
        Err(E::AsciiEofWhileExpecting { what })
    }
}
fn lower(t: &[&str]) -> Vec<String> {
    t.iter().map(|s| s.to_ascii_lowercase()).collect()
}
fn eq(t: &[&str], keys: &[&str]) -> bool {
    t.len() == keys.len() && t.iter().zip(keys).all(|(a, b)| a.eq_ignore_ascii_case(b))
}
fn floats(t: &[&str], what: &'static str) -> Result<[f32; 3], E> {
    let mut a = [0.0f32; 3];
    for (i, s) in t.iter().enumerate() {
        a[i] = s.parse().map_err(|source| E::InvalidFloat {
            token: s.to_ascii_lowercase(),
            source,
        })?;
        if !a[i].is_finite() {
            return Err(E::NonFiniteFloat { what, value: a[i] });
        }
    }
    Ok(a)
}
fn ascii(b: &[u8]) -> Result<Scene, E> {
    if b.is_empty() {
        return Err(E::AsciiEmptyInput);
    }
    let mut it = b.split((|c: &u8| *c == b'\n') as fn(&u8) -> bool);
    let first = it.next().unwrap_or(&[]);
    text(first)?;
    let name = header(first).ok_or(E::AsciiHeaderMissing)?;
    let body = &b[first.len() + usize::from(first.len() < b.len())..];
    if body.len() >= 16384 {
        // Partition at facet headers without decoding later lines. The regular
        // path is parallel; malformed input falls back to the sequential parser
        // to retain grammar/UTF-8/EOF error precedence exactly.
        let mut starts = vec![0];
        let mut offset = 0;
        let mut facets = 0usize;
        let mut end = body.len();
        for line in body.split_inclusive(|c| *c == b'\n') {
            let trimmed = line.trim_ascii_start();
            let word = trimmed
                .split(|c| c.is_ascii_whitespace())
                .next()
                .unwrap_or(&[]);
            if word.eq_ignore_ascii_case(b"endsolid") {
                end = offset;
                break;
            }
            if word.eq_ignore_ascii_case(b"facet") {
                if offset > 0 && facets.is_multiple_of(1024) {
                    starts.push(offset);
                }
                facets += 1;
            }
            offset += line.len();
        }
        if end < body.len() {
            starts.retain(|s| *s < end);
            starts.push(end);
            let blocks: Vec<_> = starts
                .par_windows(2)
                .map(|range| ascii_triangles(&body[range[0]..range[1]], true))
                .collect();
            if blocks.iter().all(Result::is_ok) {
                let mut triangles = Vec::with_capacity(b.len() / 180);
                for block in blocks {
                    triangles.extend(block?);
                }
                return build(triangles, name);
            }
        }
    }
    build(ascii_triangles(body, false)?, name)
}
fn ascii_triangles(body: &[u8], allow_eof: bool) -> Result<Vec<Triangle>, E> {
    let mut lines = Lines {
        lines: body.split((|c: &u8| *c == b'\n') as fn(&u8) -> bool),
    };
    let mut triangles = Vec::with_capacity(body.len() / 180);
    loop {
        let t = match lines.next("facet or endsolid") {
            Err(E::AsciiEofWhileExpecting { .. }) if allow_eof => break,
            result => result?,
        };
        if t[0].eq_ignore_ascii_case("endsolid") {
            break;
        }
        if t.len() != 5
            || !t[0].eq_ignore_ascii_case("facet")
            || !t[1].eq_ignore_ascii_case("normal")
        {
            return Err(E::AsciiInvalidFacetHeader { tokens: lower(&t) });
        }
        let n = Normal::new(floats(&t[2..], "facet normal component")?);
        let t = lines.next("outer loop")?;
        if !eq(&t, &["outer", "loop"]) {
            return Err(E::AsciiUnexpected {
                expected: "outer loop",
                got: lower(&t),
            });
        }
        let mut v = [Vertex::new([0.; 3]); 3];
        for p in &mut v {
            let t = lines.next("vertex")?;
            if t.len() != 4 || !t[0].eq_ignore_ascii_case("vertex") {
                return Err(E::AsciiInvalidVertexLine { tokens: lower(&t) });
            }
            *p = Vertex::new(floats(&t[1..], "vertex coordinate")?)
        }
        for keyword in ["endloop", "endfacet"] {
            let t = lines.next(keyword)?;
            if !eq(&t, &[keyword]) {
                return Err(E::AsciiUnexpected {
                    expected: keyword,
                    got: lower(&t),
                });
            }
        }
        triangles.push(Triangle::new(normal(n, &v), v));
    }
    Ok(triangles)
}
fn name(m: &IndexedMesh) -> Option<String> {
    m.meta
        .as_ref()
        .and_then(|m| m.name.as_deref())
        .and_then(|s| crate::clean_name(s, true))
}
pub fn write_stla(m: &IndexedMesh, w: &mut impl Write) -> Result<(), WriteStlError> {
    write_stla_with_progress(m, w, &mut |_| {})
}
pub fn write_stlb(m: &IndexedMesh, w: &mut impl Write) -> Result<(), WriteStlError> {
    write_stlb_with_progress(m, w, &mut |_| {})
}
pub fn write_stla_with_progress(
    m: &IndexedMesh,
    w: &mut impl Write,
    cb: &mut impl FnMut(WriteSceneProgress),
) -> Result<(), WriteStlError> {
    let name = name(m);
    let n = name.as_deref().unwrap_or("");
    writeln!(w, "solid {n}")?;
    tick(cb, 0, m.faces.len());
    for (i, f) in m.faces.iter().enumerate() {
        let [x, y, z] = f.normal.coords();
        writeln!(w, "  facet normal {x:e} {y:e} {z:e}\n    outer loop")?;
        for index in f.vertices {
            let [x, y, z] = m.vertices[index.get() as usize].coords();
            writeln!(w, "      vertex {x:e} {y:e} {z:e}")?
        }
        writeln!(w, "    endloop\n  endfacet")?;
        tick(cb, i + 1, m.faces.len());
    }
    write!(w, "endsolid{}{}", if name.is_some() { " " } else { "" }, n)?;
    w.flush()?;
    Ok(())
}
pub fn write_stlb_with_progress(
    m: &IndexedMesh,
    w: &mut impl Write,
    cb: &mut impl FnMut(WriteSceneProgress),
) -> Result<(), WriteStlError> {
    let n = name(m).unwrap_or_default();
    let n = n[..n.len().min(80)].trim_end();
    let mut h = [0; 80];
    h[..n.len()].copy_from_slice(n.as_bytes());
    w.write_all(&h)?;
    let count = u32::try_from(m.faces.len()).map_err(|_| WriteStlError::FaceCountExceedsU32 {
        count: m.faces.len(),
    })?;
    w.write_all(&count.to_le_bytes())?;
    tick(cb, 0, m.faces.len());
    const BLOCK: usize = 16384;
    let mut buffer = vec![0; 50 * BLOCK.min(m.faces.len())];
    for (block_id, faces) in m.faces.chunks(BLOCK).enumerate() {
        for (i, f) in faces.iter().enumerate() {
            let record = &mut buffer[i * 50..(i + 1) * 50];
            let vectors = [
                f.normal.coords(),
                m.vertices[f.vertices[0].get() as usize].coords(),
                m.vertices[f.vertices[1].get() as usize].coords(),
                m.vertices[f.vertices[2].get() as usize].coords(),
            ];
            for (dst, xyz) in record[..48].as_chunks_mut::<12>().0.iter_mut().zip(vectors) {
                dst[..4].copy_from_slice(&xyz[0].to_le_bytes());
                dst[4..8].copy_from_slice(&xyz[1].to_le_bytes());
                dst[8..12].copy_from_slice(&xyz[2].to_le_bytes());
            }
            tick(cb, block_id * BLOCK + i + 1, m.faces.len());
        }
        w.write_all(&buffer[..faces.len() * 50])?;
    }
    w.flush()?;
    Ok(())
}
