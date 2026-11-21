// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_3mf::model::*;
use sc_sl1cer::{contour::*, geom2d::Point2D};

/// Why held slices could not go into, or come back out of, a project's
/// slice model.
#[derive(Debug)]
pub enum SliceMappingError {
    /// The contours and the slab tops say different numbers of layers.
    LayerCountMismatch { layers: usize, tops: usize },
    /// A polygon names a vertex the stack does not have.
    VertexIndexOutOfRange { index: u32 },
    /// Read back, the layers do not rise strictly.
    NonMonotoneLayers,
    /// Read back, a layer's height is not a number.
    NonFiniteLayerZ,
    /// The stack points at slices in other parts, which is not supported.
    SliceRefsUnsupported,
}

impl std::fmt::Display for SliceMappingError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let fixed = match self {
            &Self::LayerCountMismatch { layers, tops } => {
                return write!(out, "contour stack has {layers} layers but {tops} slab tops were supplied");
            }
            &Self::VertexIndexOutOfRange { index } => {
                return write!(out, "a slice polygon referenced vertex index {index}, which is out of range");
            }
            Self::NonMonotoneLayers => "the reconstructed slice layers are not in strictly increasing Z",
            Self::NonFiniteLayerZ => "a reconstructed slice layer has a non-finite Z",
            Self::SliceRefsUnsupported => "the slice stack uses slicerefs to other parts, which are not supported",
        };
        out.write_str(fixed)
    }
}

impl std::error::Error for SliceMappingError {}
#[derive(Clone, Debug, PartialEq)]
pub struct PlateSlices {
    pub contours: ContourStack,
    pub slab_tops: Vec<f64>,
}
fn bits(p: &Point2D) -> (u64, u64) {
    (p.x.to_bits(), p.y.to_bits())
}
pub(crate) fn to_model(s: &PlateSlices) -> Result<ModelDocument, SliceMappingError> {
    if s.contours.layers().len() != s.slab_tops.len() {
        return Err(SliceMappingError::LayerCountMismatch {
            layers: s.contours.layers().len(),
            tops: s.slab_tops.len(),
        });
    }
    let mut slices = vec![];
    for (l, &z_top) in s.contours.layers().iter().zip(&s.slab_tops) {
        let mut vertices = vec![];
        let mut polygons = vec![];
        let mut shared = std::collections::HashMap::new();
        for p in l.polygons() {
            for r in std::iter::once(p.outer()).chain(p.holes()) {
                let mut points: Vec<_> = r.vertices().iter().collect();
                points.dedup_by(|a, b| bits(a) == bits(b));
                if points.len() > 1 && bits(points[0]) == bits(points[points.len() - 1]) {
                    points.pop();
                }
                if points.len() < 3 {
                    continue;
                }
                let mut indices = vec![];
                for p in points {
                    let key = bits(p);
                    let i = if let Some(&i) = shared.get(&key) {
                        i
                    } else {
                        let raw = u32::try_from(vertices.len()).map_err(|_| {
                            SliceMappingError::VertexIndexOutOfRange { index: u32::MAX }
                        })?;
                        let i = ResourceIndex::new(raw)
                            .ok_or(SliceMappingError::VertexIndexOutOfRange { index: raw })?;
                        vertices.push(SliceVertex { x: p.x, y: p.y });
                        shared.insert(key, i);
                        i
                    };
                    indices.push(i);
                }
                let start = indices[0];
                let segments = indices[1..]
                    .iter()
                    .copied()
                    .chain(std::iter::once(start))
                    .map(|end| SliceSegment { end })
                    .collect();
                polygons.push(SlicePolygon { start, segments });
            }
        }
        slices.push(Slice {
            z_top,
            vertices,
            polygons,
        });
    }
    Ok(ModelDocument {
        slice_stacks: vec![SliceStack {
            id: ResourceId::MIN,
            z_bottom: 0.0,
            content: SliceStackContent::Slices(slices),
        }],
        ..Default::default()
    })
}
pub(crate) fn from_model(m: &ModelDocument) -> Result<ContourStack, SliceMappingError> {
    let s = m
        .slice_stacks
        .first()
        .ok_or(SliceMappingError::SliceRefsUnsupported)?;
    let SliceStackContent::Slices(slices) = &s.content else {
        return Err(SliceMappingError::SliceRefsUnsupported);
    };
    let mut layers = vec![];
    let mut prev = None;
    for s in slices {
        if !s.z_top.is_finite() {
            return Err(SliceMappingError::NonFiniteLayerZ);
        }
        if prev.is_some_and(|v| v >= s.z_top) {
            return Err(SliceMappingError::NonMonotoneLayers);
        }
        prev = Some(s.z_top);
        let mut polygons = vec![];
        for p in &s.polygons {
            let mut points = vec![];
            for i in std::iter::once(p.start).chain(p.segments.iter().map(|s| s.end)) {
                let v = s
                    .vertices
                    .get(i.get() as usize)
                    .ok_or(SliceMappingError::VertexIndexOutOfRange { index: i.get() })?;
                points.push(Point2D::new(v.x, v.y));
            }
            if points.len() > 1 && bits(&points[0]) == bits(&points[points.len() - 1]) {
                points.pop();
            }
            if points.len() >= 3 {
                polygons.push(Polygon::new_unchecked(ClosedRing::new(points), vec![]));
            }
        }
        layers.push(ContourLayer::new(s.z_top, polygons));
    }
    Ok(ContourStack::new_unchecked(layers))
}
