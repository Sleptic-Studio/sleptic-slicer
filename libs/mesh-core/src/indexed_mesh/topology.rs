// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::{IndexedMesh, ValidatedEdgeManifoldMesh, validated::incidences};
use crate::primitives::{FaceIndex, VertexIndex};
pub type EdgeKey = (VertexIndex, VertexIndex);
pub fn edge_key(a: VertexIndex, b: VertexIndex) -> EdgeKey {
    (a.min(b), a.max(b))
}
#[derive(Debug, Clone)]
pub struct MeshTopology {
    edges: Vec<(EdgeKey, [FaceIndex; 2])>,
    vertex_offsets: Vec<usize>,
    fan_ranges: Vec<std::ops::Range<usize>>,
    fan_faces: Vec<FaceIndex>,
}
impl MeshTopology {
    pub fn from_edge_manifold(mesh: &ValidatedEdgeManifoldMesh<'_>) -> Self {
        let source = mesh.as_indexed_mesh();
        let records = incidences(source);
        let mut edges = Vec::with_capacity(records.len() / 2);
        let mut adjacent = vec![[usize::MAX; 2]; source.faces.len() * 3];
        for pair in records.as_chunks::<2>().0 {
            edges.push((pair[0].edge, [pair[0].face.into(), pair[1].face.into()]));
            for v in [pair[0].edge.0, pair[0].edge.1] {
                let a = corner(source, pair[0].face, v);
                let b = corner(source, pair[1].face, v);
                for (x, y) in [(a, b), (b, a)] {
                    let slot = if adjacent[x][0] == usize::MAX { 0 } else { 1 };
                    adjacent[x][slot] = y;
                }
            }
        }
        let mut fans = Vec::new();
        let mut fan_faces = Vec::with_capacity(adjacent.len());
        let mut visited = vec![false; adjacent.len()];
        for start in 0..adjacent.len() {
            if visited[start] {
                continue;
            }
            let vertex = usize::from(source.faces[start / 3].vertices[start % 3]);
            let fan_start = fan_faces.len();
            let mut previous = usize::MAX;
            let mut current = start;
            while !visited[current] {
                visited[current] = true;
                fan_faces.push((current / 3).into());
                let next = if adjacent[current][0] != previous {
                    adjacent[current][0]
                } else {
                    adjacent[current][1]
                };
                previous = current;
                current = next;
            }
            fans.push((vertex, fan_start..fan_faces.len()));
        }
        fans.sort_unstable_by_key(|(v, r)| (*v, r.start));
        let mut vertex_offsets = vec![0; source.vertices.len() + 1];
        for (v, _) in &fans {
            vertex_offsets[v + 1] += 1;
        }
        for i in 1..vertex_offsets.len() {
            vertex_offsets[i] += vertex_offsets[i - 1];
        }
        Self {
            edges,
            vertex_offsets,
            fan_ranges: fans.into_iter().map(|(_, r)| r).collect(),
            fan_faces,
        }
    }
    pub fn edge_faces(&self, a: VertexIndex, b: VertexIndex) -> Option<[FaceIndex; 2]> {
        match self.edges.binary_search_by_key(&edge_key(a, b), |e| e.0) {
            Ok(index) => Some(self.edges[index].1),
            Err(_) => None,
        }
    }
    pub fn vertex_fans(&self, v: VertexIndex) -> Option<impl Iterator<Item = &[FaceIndex]>> {
        let i = usize::from(v);
        let (&start, &end) = (self.vertex_offsets.get(i)?, self.vertex_offsets.get(i + 1)?);
        (start != end).then(|| {
            self.fan_ranges[start..end]
                .iter()
                .map(|r| &self.fan_faces[r.clone()])
        })
    }
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}
pub(super) fn corner(mesh: &IndexedMesh, face: usize, v: VertexIndex) -> usize {
    let f = &mesh.faces[face];
    face * 3
        + if f.vertices[0] == v {
            0
        } else if f.vertices[1] == v {
            1
        } else {
            2
        }
}
pub(super) struct Disjoint {
    parent: Vec<usize>,
    rank: Vec<u8>,
}
impl Disjoint {
    pub(super) fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }
    pub(super) fn root(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }
    pub(super) fn join(&mut self, a: usize, b: usize) {
        let mut a = self.root(a);
        let mut b = self.root(b);
        if a == b {
            return;
        }
        if self.rank[a] < self.rank[b] {
            std::mem::swap(&mut a, &mut b);
        }
        self.parent[b] = a;
        if self.rank[a] == self.rank[b] {
            self.rank[a] += 1;
        }
    }
}
