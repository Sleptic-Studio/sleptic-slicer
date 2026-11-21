// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Layer outlines, from the top down: a stack of layers, each layer's
//! polygons, each polygon's rings. Outer rings run counter-clockwise and
//! holes clockwise, so filling by winding needs no other bookkeeping.
use crate::geom2d::Point2D;

/// Layers bottom to top, each strictly above the one before.
#[derive(Debug, Clone, PartialEq)]
pub struct ContourStack {
    layers: Vec<ContourLayer>,
}

/// Why layers do not make a stack.
#[derive(Debug, Clone, PartialEq)]
pub enum ContourStackError {
    NonFiniteZ { idx: usize, z_mm: f64 },
    NonMonotoneZ { prev_idx: usize, prev_z_mm: f64, next_z_mm: f64 },
}

impl ContourStack {
    /// The layers as a stack, if every height is finite and each is above
    /// the last. A height that is not a number is reported first, wherever
    /// it is, since no order can be judged around it.
    pub fn new(layers: Vec<ContourLayer>) -> Result<ContourStack, ContourStackError> {
        let heights: Vec<f64> = layers.iter().map(ContourLayer::z_mm).collect();
        if let Some(idx) = heights.iter().position(|z| !z.is_finite()) {
            return Err(ContourStackError::NonFiniteZ { idx, z_mm: heights[idx] });
        }
        for (prev_idx, step) in heights.windows(2).enumerate() {
            let &[prev_z_mm, next_z_mm] = step else { continue };
            if next_z_mm <= prev_z_mm {
                return Err(ContourStackError::NonMonotoneZ { prev_idx, prev_z_mm, next_z_mm });
            }
        }
        Ok(ContourStack { layers })
    }

    /// The layers as given, for callers that built them in order.
    pub fn new_unchecked(layers: Vec<ContourLayer>) -> ContourStack {
        ContourStack { layers }
    }

    /// How many layers.
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn layers(&self) -> &[ContourLayer] {
        &self.layers
    }

    pub fn into_layers(self) -> Vec<ContourLayer> {
        self.layers
    }
}

/// Stacks that cannot be laid side by side: different layer counts, or a
/// layer at a different height.
#[derive(Debug, Clone, PartialEq)]
pub enum LayerAlignmentError {
    LayerCountMismatch { stack_idx: usize, expected: usize, found: usize },
    LayerZMismatch { stack_idx: usize, layer_idx: usize, expected_z_mm: f64, found_z_mm: f64 },
}

/// One stack holding every stack's polygons, layer by layer. The stacks
/// must share their layer heights exactly; polygons are put side by side,
/// not unioned. No stacks give an empty stack.
pub fn merge_stacks(stacks: Vec<ContourStack>) -> Result<ContourStack, LayerAlignmentError> {
    let mut stacks = stacks.into_iter().enumerate();
    let Some((_, mut merged)) = stacks.next() else {
        return Ok(ContourStack::new_unchecked(Vec::new()));
    };
    for (stack_idx, stack) in stacks {
        let (expected, found) = (merged.len(), stack.len());
        if expected != found {
            return Err(LayerAlignmentError::LayerCountMismatch { stack_idx, expected, found });
        }
        let pairs = merged.layers.iter_mut().zip(stack.layers);
        for (layer_idx, (into, from)) in pairs.enumerate() {
            if into.z_mm != from.z_mm {
                let (expected_z_mm, found_z_mm) = (into.z_mm, from.z_mm);
                return Err(LayerAlignmentError::LayerZMismatch { stack_idx, layer_idx, expected_z_mm, found_z_mm });
            }
            into.polygons.extend(from.polygons);
        }
    }
    Ok(merged)
}

/// The polygons cut at one height.
#[derive(Debug, Clone, PartialEq)]
pub struct ContourLayer {
    polygons: Vec<Polygon>,
    z_mm: f64,
}

impl ContourLayer {
    pub fn new(z_mm: f64, polygons: Vec<Polygon>) -> ContourLayer {
        ContourLayer { polygons, z_mm }
    }

    pub fn z_mm(&self) -> f64 {
        self.z_mm
    }

    pub fn polygons(&self) -> &[Polygon] {
        &self.polygons
    }
}

/// An outer ring and the holes in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Polygon {
    holes: Vec<ClosedRing>,
    outer: ClosedRing,
}

/// A ring wound the wrong way for its role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolygonWindingError {
    OuterNotCcw,
    HoleNotCw { hole_idx: usize },
}

impl Polygon {
    /// The polygon, unless the outer ring runs clockwise (or encloses
    /// nothing) or a hole runs counter-clockwise (or encloses nothing). A
    /// ring whose area is not a number cannot be judged and is let through.
    pub fn new(outer: ClosedRing, holes: Vec<ClosedRing>) -> Result<Polygon, PolygonWindingError> {
        if outer.signed_area() <= 0.0 {
            return Err(PolygonWindingError::OuterNotCcw);
        }
        match holes.iter().position(|hole| hole.signed_area() >= 0.0) {
            Some(hole_idx) => Err(PolygonWindingError::HoleNotCw { hole_idx }),
            None => Ok(Polygon { holes, outer }),
        }
    }

    /// The rings as given, for callers that wound them already.
    pub fn new_unchecked(outer: ClosedRing, holes: Vec<ClosedRing>) -> Polygon {
        Polygon { holes, outer }
    }

    pub fn outer(&self) -> &ClosedRing {
        &self.outer
    }

    pub fn holes(&self) -> &[ClosedRing] {
        &self.holes
    }
}

/// Which way a ring runs, seen from above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Winding {
    Ccw,
    Cw,
}

/// A loop of points; the last joins back to the first without repeating it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClosedRing {
    vertices: Vec<Point2D>,
}

impl ClosedRing {
    pub fn new(vertices: Vec<Point2D>) -> ClosedRing {
        ClosedRing { vertices }
    }

    /// How many points.
    pub fn len(&self) -> usize {
        self.vertices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Runs the ring the other way.
    pub fn reverse(&mut self) {
        self.vertices.reverse()
    }

    pub fn vertices(&self) -> &[Point2D] {
        &self.vertices
    }

    /// The enclosed area by the shoelace formula: positive when the ring
    /// runs counter-clockwise, zero for fewer than three points.
    pub fn signed_area(&self) -> f64 {
        let points = &self.vertices;
        if points.len() < 3 {
            return 0.0;
        }
        let following = points.iter().skip(1).chain(points.first());
        let cross: f64 = points.iter().zip(following).map(|(a, b)| a.x * b.y - b.x * a.y).sum();
        cross / 2.0
    }

    /// The ring's direction; none when it encloses no area (or the area
    /// is not a number).
    pub fn winding(&self) -> Option<Winding> {
        let area = self.signed_area();
        if area > 0.0 {
            Some(Winding::Ccw)
        } else if area < 0.0 {
            Some(Winding::Cw)
        } else {
            None
        }
    }
}
