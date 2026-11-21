// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Built-in exposure tiles and persistent scene tags for sequential LCD masks.
use sc_mesh_core::transform::Transform3D;
use sc_mesh_core::{IndexedMesh, IndexedTriangle, Vertex};
use sc_scene::{Scene, node::LeafPayload};
use std::sync::Arc;

/// A calibration request failed before changing a scene or output file.
#[derive(Debug)]
pub enum CalibrationError {
    /// The explicit exposure list contains a non-number.
    ExposureList,
    /// Selected printer cannot contain the standard machine test.
    MachineSize,
    /// Count, range, precision or distinctness is invalid.
    ExposureSet,
    /// A single tile/tag has an unrepresentable exposure.
    Exposure,
    /// A built-in feature could not be constructed as a closed solid.
    Geometry(&'static str, crate::model_tools::ModelToolError),
    /// Scene geometry could not be installed or baked.
    Scene(sc_scene::SceneError),
    /// Ordinary objects and calibration tiles cannot share an exposure job.
    MixedPlate,
    /// Persisted calibration metadata is invalid.
    InvalidTag,
    /// Two baked tile rectangles overlap.
    Overlap,
}
impl std::fmt::Display for CalibrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MachineSize => {
                f.write_str("Printer is too small for this machine test, or dimensions are invalid")
            }
            Self::ExposureList => f.write_str("Enter comma-separated exposure seconds"),
            Self::ExposureSet => f.write_str(
                "Choose 2 to 8 distinct exposures from 0.001 to 600 seconds in 0.001-second steps",
            ),
            Self::Exposure => {
                f.write_str("Exposure must be from 0.001 to 600 seconds in 0.001-second steps")
            }
            Self::Geometry(feature, error) => write!(f, "Calibration {feature}: {error}"),
            Self::Scene(error) => error.fmt(f),
            Self::MixedPlate => f.write_str("Calibration plate must contain exposure tiles only"),
            Self::InvalidTag => f.write_str("Invalid calibration exposure metadata"),
            Self::Overlap => {
                f.write_str("Calibration tile masks overlap; arrange the tiles before export")
            }
        }
    }
}
impl std::error::Error for CalibrationError {}
impl From<CalibrationError> for String {
    fn from(error: CalibrationError) -> Self {
        error.to_string()
    }
}
fn valid_exposure(value: f64) -> bool {
    value.is_finite()
        && (0.001..=600.0).contains(&value)
        && (value * 1000.0 - (value * 1000.0).round()).abs() < 1e-8
}
fn valid_set(values: &[f64]) -> bool {
    (2..=8).contains(&values.len())
        && values.iter().all(|v| valid_exposure(*v))
        && values.iter().enumerate().all(|(i, v)| {
            values[..i]
                .iter()
                .all(|a| (*a * 1000.0).round() != (*v * 1000.0).round())
        })
}
/// Exposure label with at least two decimals and millisecond precision.
pub fn exposure_text(value: f64) -> String {
    let mut text = format!("{value:.3}");
    if text.ends_with('0') {
        text.pop();
    }
    text
}

/// Parse 2–8 positive exposure times, either an explicit comma-separated list or
/// an arithmetic start/step sequence. Reject duplicate exposures.
pub fn exposures(
    count: usize,
    start: f64,
    step: f64,
    list: &str,
) -> Result<Vec<f64>, CalibrationError> {
    if list.trim().is_empty() && !(2..=8).contains(&count) {
        return Err(CalibrationError::ExposureSet);
    }
    let values = if list.trim().is_empty() {
        (0..count)
            .map(|i| start + step * i as f64)
            .collect::<Vec<_>>()
    } else {
        list.split(',')
            .take(9)
            .map(|v| {
                v.trim()
                    .parse::<f64>()
                    .map_err(|_| CalibrationError::ExposureList)
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    if !valid_set(&values) {
        return Err(CalibrationError::ExposureSet);
    }
    Ok(values
        .into_iter()
        .map(|v| (v * 1000.0).round() / 1000.0)
        .collect())
}
fn box_mesh(lo: [f64; 3], hi: [f64; 3]) -> IndexedMesh {
    hexahedron([
        [lo[0], lo[1], lo[2]],
        [hi[0], lo[1], lo[2]],
        [hi[0], hi[1], lo[2]],
        [lo[0], hi[1], lo[2]],
        [lo[0], lo[1], hi[2]],
        [hi[0], lo[1], hi[2]],
        [hi[0], hi[1], hi[2]],
        [lo[0], hi[1], hi[2]],
    ])
}
/// A closed six-sided solid with planar faces, corners ordered as in
/// [`box_mesh`] (bottom ring, then top ring above it).
fn hexahedron(corners: [[f64; 3]; 8]) -> IndexedMesh {
    let vertices: Vec<_> = corners
    .into_iter()
    .map(|p| Vertex::new(p.map(|v| v as f32)))
    .collect();
    let mut faces = Vec::new();
    for q in [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ] {
        for ids in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
            let n = sc_mesh_core::geometry::compute_normal_from_triangle_vertices(
                &ids.map(|i| vertices[i as usize]),
            )
            .unwrap();
            faces.push(IndexedTriangle::from_raw_indices(n, ids));
        }
    }
    IndexedMesh::new(None, vertices, faces)
}
fn cylinder(x: f64, y: f64, r: f64, z0: f64, z1: f64) -> IndexedMesh {
    let mut vertices = Vec::new();
    for z in [z0, z1] {
        for i in 0..24 {
            let a = i as f64 * std::f64::consts::TAU / 24.0;
            vertices.push(Vertex::new([
                (x + r * a.cos()) as f32,
                (y + r * a.sin()) as f32,
                z as f32,
            ]));
        }
    }
    vertices.push(Vertex::new([x as f32, y as f32, z0 as f32]));
    vertices.push(Vertex::new([x as f32, y as f32, z1 as f32]));
    let mut faces = Vec::new();
    for i in 0..24u32 {
        let j = (i + 1) % 24;
        for ids in [
            [48, j, i],
            [49, i + 24, j + 24],
            [i, j, j + 24],
            [i, j + 24, i + 24],
        ] {
            let n = sc_mesh_core::geometry::compute_normal_from_triangle_vertices(
                &ids.map(|i| vertices[i as usize]),
            )
            .unwrap();
            faces.push(IndexedTriangle::from_raw_indices(n, ids));
        }
    }
    IndexedMesh::new(None, vertices, faces)
}
fn append(target: &mut IndexedMesh, mesh: IndexedMesh) {
    let offset = target.vertices.len() as u32;
    target.vertices.extend(mesh.vertices);
    target.faces.extend(mesh.faces.into_iter().map(|f| {
        IndexedTriangle::from_raw_indices(f.normal, f.vertices.map(|i| i.get() + offset))
    }));
}
/// Generate a 32 × 24 mm tile with labelled exposure, columns, pins,
/// 0.3–1.5 mm holes, four 15–60 degree fins and a 6 mm bridge.
/// Closed overlapping feature shells are unioned by the normal slice pipeline.
pub fn tile(exposure: f64) -> Result<IndexedMesh, CalibrationError> {
    if !valid_exposure(exposure) {
        return Err(CalibrationError::Exposure);
    }
    let text = format!("{}s", exposure_text(exposure));
    let left = box_mesh([0.0, 0.0, 0.0], [16.0, 9.5, 0.8]);
    let right = box_mesh([16.0, 0.0, 0.0], [32.0, 9.5, 0.8]);
    let mut base = crate::model_tools::label(
        &left,
        &text,
        [2.0, 2.0, 0.8],
        [0.0, 0.0, 1.0],
        2.5,
        0.3,
        false,
    )
    .map_err(|e| CalibrationError::Geometry("positive label", e))?;
    append(
        &mut base,
        crate::model_tools::label(
            &right,
            &text,
            [18.0, 2.0, 0.8],
            [0.0, 0.0, 1.0],
            2.5,
            0.3,
            true,
        )
        .map_err(|e| CalibrationError::Geometry("negative label", e))?,
    );
    append(&mut base, box_mesh([0.0, 12.5, 0.0], [32.0, 24.0, 0.8]));
    for i in 0..7 {
        let x = 3.0 + i as f64 * 4.0;
        let d = 0.3 + 0.2 * i as f64;
        let lo = if i == 0 { 0.0 } else { x - 2.0 };
        let hi = if i == 6 { 32.0 } else { x + 2.0 };
        let cell = box_mesh([lo, 9.5, 0.0], [hi, 12.5, 0.8]);
        let cell = crate::model_tools::boolean(
            &cell,
            &cylinder(x, 11.0, d / 2.0, -0.1, 1.0),
            crate::model_tools::BooleanOp::Subtract,
        )
        .map_err(|e| CalibrationError::Geometry("hole", e))?;
        append(&mut base, cell);
    }
    for i in 0..5 {
        let d = 0.2 + 0.2 * i as f64;
        let x = 3.0 + i as f64 * 5.5;
        append(&mut base, box_mesh([x, 6.0, 0.79], [x + d, 6.0 + d, 4.0]));
        append(&mut base, cylinder(x, 8.0, d / 2.0, 0.79, 4.0));
    }
    for i in 0..4 {
        let angle = (15.0 + 15.0 * i as f64).to_radians();
        let mut fin = box_mesh([0.0, 0.0, 0.0], [0.5, 6.0, 0.4]);
        for v in &mut fin.vertices {
            let [x, y, z] = v.coords().map(f64::from);
            *v = Vertex::new([
                (3.0 + i as f64 * 5.0 + x) as f32,
                (15.0 + y * angle.cos() - z * angle.sin()) as f32,
                (0.7 + y * angle.sin() + z * angle.cos()) as f32,
            ]);
        }
        // Recompute normals after rotation.
        for f in &mut fin.faces {
            f.normal = sc_mesh_core::geometry::compute_normal_from_triangle_vertices(
                &f.vertices.map(|i| fin.vertices[i.get() as usize]),
            )
            .unwrap();
        }
        append(&mut base, fin);
    }
    for x in [24.0, 30.0] {
        append(&mut base, box_mesh([x, 15.0, 0.79], [x + 1.0, 17.0, 4.0]));
    }
    append(&mut base, box_mesh([24.0, 15.0, 3.8], [31.0, 17.0, 4.4]));
    Ok(base)
}
/// Overhang angles of the overhang test's ledges, in degrees from horizontal.
pub const OVERHANG_TEST_ANGLES: [f64; 6] = [30.0, 40.0, 50.0, 60.0, 70.0, 80.0];

/// The overhang test (Sleptic's "self support reach test", R80): a wall with
/// six ledges whose undersides leave it at 30 to 80 degrees from
/// horizontal, each with its angle raised on the base in front of it.
/// Printed without supports, the flattest ledge that comes out clean is
/// the resin's support angle: overhangs flatter than it need supports.
/// 60 x 15 x 15 mm, flat on the plate.
pub fn overhang_test() -> Result<IndexedMesh, CalibrationError> {
    const PITCH: f64 = 10.0;
    // Ledge: 8 mm along its slope, 1.5 mm thick, 6 mm wide; its underside
    // leaves the wall 4 mm up and reaches 0.5 mm into it.
    const SLOPE: f64 = 8.0;
    const THICK: f64 = 1.5;
    const ROOT_Z: f64 = 4.0;
    const INTO_WALL: f64 = 0.5;
    let width = PITCH * OVERHANG_TEST_ANGLES.len() as f64;
    let mut mesh = box_mesh([0.0, -3.0, 0.0], [width, 0.0, 15.0]);
    for (i, angle) in OVERHANG_TEST_ANGLES.iter().enumerate() {
        let x0 = i as f64 * PITCH;
        let cell = box_mesh([x0, 0.0, 0.0], [x0 + PITCH, 12.0, 1.5]);
        append(
            &mut mesh,
            crate::model_tools::label(
                &cell,
                &format!("{angle:.0}"),
                [x0 + 2.5, 8.0, 1.5],
                [0.0, 0.0, 1.0],
                3.0,
                0.4,
                false,
            )
            .map_err(|e| CalibrationError::Geometry("overhang label", e))?,
        );
        let a = angle.to_radians();
        let back_z = ROOT_Z - INTO_WALL * a.tan();
        let (reach, front_z) = (SLOPE * a.cos(), ROOT_Z + SLOPE * a.sin());
        let (xa, xb) = (x0 + 2.0, x0 + 8.0);
        append(
            &mut mesh,
            hexahedron([
                [xa, -INTO_WALL, back_z],
                [xb, -INTO_WALL, back_z],
                [xb, reach, front_z],
                [xa, reach, front_z],
                [xa, -INTO_WALL, back_z + THICK],
                [xb, -INTO_WALL, back_z + THICK],
                [xb, reach, front_z + THICK],
                [xa, reach, front_z + THICK],
            ]),
        );
    }
    Ok(mesh)
}

/// The overhang test as a placeable scene, named "Overhang test".
pub fn overhang_test_scene() -> Result<Scene, CalibrationError> {
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            Some(sc_scene::SceneMetadata {
                name: Some("Overhang test".into()),
                comment: Some("sleptic-overhang-test\n".into()),
            }),
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(overhang_test()?)),
        )
        .map_err(CalibrationError::Scene)?;
    scene.add_root(id).map_err(CalibrationError::Scene)?;
    Ok(scene)
}

/// Construct independently placeable tiles. Each tag survives project save/load.
pub fn scenes(values: &[f64]) -> Result<Vec<Scene>, CalibrationError> {
    if !valid_set(values) {
        return Err(CalibrationError::ExposureSet);
    }
    values
        .iter()
        .map(|&exposure| {
            let mut scene = Scene::new();
            let id = scene
                .add_leaf(
                    Some(sc_scene::SceneMetadata {
                        name: Some(format!("Exposure {}s", exposure_text(exposure))),
                        comment: Some(format!("sleptic-exposure-tile\nexposure_s = {exposure}\n")),
                    }),
                    Transform3D::Identity,
                    LeafPayload::Mesh(Arc::new(tile(exposure)?)),
                )
                .map_err(CalibrationError::Scene)?;
            scene.add_root(id).map_err(CalibrationError::Scene)?;
            Ok(scene)
        })
        .collect()
}
/// A disjoint calibration region in plate coordinates.
#[derive(Clone, Debug)]
pub struct Region {
    /// Lower XY corner.
    pub min: [f64; 2],
    /// Upper XY corner.
    pub max: [f64; 2],
    /// Normal exposure seconds.
    pub exposure_s: f64,
}
/// Extract tile regions; refuse mixed jobs and overlapping masks instead of
/// silently printing ordinary objects at an undefined exposure.
pub fn regions(scene: &Scene) -> Result<Vec<Region>, CalibrationError> {
    let ids = scene
        .reachable_leaves(sc_scene::CsgPolicy::Refuse, sc_scene::ReferencePolicy::Skip)
        .map_err(CalibrationError::Scene)?;
    let any = ids.iter().any(|&id| {
        scene
            .node(id)
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.comment.as_deref())
            .is_some_and(|c| c.starts_with("sleptic-exposure-tile\n"))
    });
    if !any {
        return Ok(Vec::new());
    }
    let mut regions = Vec::<Region>::new();
    for id in ids {
        let comment = scene
            .node(id)
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.comment.as_deref())
            .ok_or(CalibrationError::MixedPlate)?;
        if !comment.starts_with("sleptic-exposure-tile\n") {
            return Err(CalibrationError::MixedPlate);
        }
        let exposure_s = comment
            .lines()
            .find_map(|l| l.strip_prefix("exposure_s = ")?.parse::<f64>().ok())
            .filter(|v| valid_exposure(*v))
            .ok_or(CalibrationError::InvalidTag)?;
        let mesh = sc_scene::bake_leaf_to_world(scene, id).map_err(CalibrationError::Scene)?;
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for v in mesh.vertices {
            for i in 0..2 {
                min[i] = min[i].min(f64::from(v[i]));
                max[i] = max[i].max(f64::from(v[i]));
            }
        }
        if regions.iter().any(|r| {
            min[0] < r.max[0] && max[0] > r.min[0] && min[1] < r.max[1] && max[1] > r.min[1]
        }) {
            return Err(CalibrationError::Overlap);
        }
        regions.push(Region {
            min,
            max,
            exposure_s,
        });
    }
    Ok(regions)
}
#[cfg(test)]
#[path = "tests_calibration.rs"]
mod tests;

/// Resolve each tile's bottom/transition ramp and sum exposures while retaining
/// one lift/retract cycle per layer. The second result is indexed tile, layer.
pub fn schedule(
    material: &sc_print_profiles::MaterialResolved,
    grid: &sc_sl1cer::profile::LayerSchedule,
    regions: &[Region],
) -> (sc_print_job::ExposureSchedule, Vec<Vec<f64>>) {
    let mut schedule = sc_print_job::ExposureSchedule::new(material, grid);
    let exposures: Vec<Vec<f64>> = regions
        .iter()
        .map(|r| {
            let mut m = material.clone();
            m.normal_exposure_sec = sc_print_primitives::PositiveFiniteSec::new(r.exposure_s)
                .expect("validated region exposure");
            sc_print_job::ExposureSchedule::new(&m, grid)
                .layers
                .iter()
                .map(|l| l.exposure_s)
                .collect()
        })
        .collect();
    if !exposures.is_empty() {
        for (i, l) in schedule.layers.iter_mut().enumerate() {
            l.exposure_s = exposures.iter().map(|v| v[i]).sum();
        }
    }
    (schedule, exposures)
}

/// Machine tools use ordinary scene geometry and ordinary material exposure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineTool {
    /// One-layer labelled LCD grid.
    Orientation,
    /// Orthogonal 100 mm tick rulers.
    XyRuler,
    /// Five steps measured from the plate.
    ZTower,
}

// Outline text built from independently closed trapezoid prisms. This avoids
// Booleaning a full LCD plate and retains counters at a single layer height.
fn flat_text(mesh: &mut IndexedMesh, text: &str, origin: [f64; 2], size: f64, height: f64) {
    let mut advance = 0.0;
    for c in text.bytes() {
        let (width, rings) = crate::label_font::glyph(c);
        let mut ys: Vec<_> = rings.iter().flatten().map(|p| p[1]).collect();
        ys.sort_by(f64::total_cmp);
        ys.dedup();
        for band in ys.windows(2) {
            let (lo, hi) = (band[0], band[1]);
            if hi - lo < 1e-7 {
                continue;
            }
            let mid = (lo + hi) * 0.5;
            let mut edges = Vec::new();
            for ring in &rings {
                for i in 0..ring.len() {
                    let a = ring[i];
                    let b = ring[(i + 1) % ring.len()];
                    if (a[1] <= mid && b[1] > mid) || (b[1] <= mid && a[1] > mid) {
                        let at = |y| a[0] + (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]);
                        edges.push((at(mid), at(lo), at(hi)));
                    }
                }
            }
            edges.sort_by(|a, b| a.0.total_cmp(&b.0));
            for pair in edges.as_chunks::<2>().0 {
                // Nudge collapsed triangle tips by a micron to keep box topology.
                let x0 = pair[0].1;
                let x1 = pair[1].1.max(x0 + 0.0001);
                let x3 = pair[0].2;
                let x2 = pair[1].2.max(x3 + 0.0001);
                let points = [[x0, lo], [x1, lo], [x2, hi], [x3, hi]];
                let mut prism = box_mesh([0.0, 0.0, 0.0], [1.0, 1.0, height]);
                for vertex in &mut prism.vertices {
                    let [x, y, z] = vertex.coords();
                    let index = match (x > 0.5, y > 0.5) {
                        (false, false) => 0,
                        (true, false) => 1,
                        (true, true) => 2,
                        (false, true) => 3,
                    };
                    *vertex = Vertex::new([
                        (origin[0] + size * (points[index][0] + advance)) as f32,
                        (origin[1] + size * points[index][1]) as f32,
                        z,
                    ]);
                }
                for face in &mut prism.faces {
                    face.normal = sc_mesh_core::geometry::compute_normal_from_triangle_vertices(
                        &face.vertices.map(|i| prism.vertices[usize::from(i)]),
                    )
                    .unwrap();
                }
                append(mesh, prism);
            }
        }
        advance += width;
    }
}
/// Generate a single layer LCD orientation pattern, 100 mm rulers, or a tower
/// with 5/10/15/20/25 mm top surfaces measured from the plate.
pub fn machine_scene(
    tool: MachineTool,
    build: [f64; 3],
    layer_height: f64,
) -> Result<Scene, CalibrationError> {
    if build.iter().any(|v| !v.is_finite() || *v <= 0.0)
        || !layer_height.is_finite()
        || layer_height <= 0.0
    {
        return Err(CalibrationError::MachineSize);
    }
    let layer_height = layer_height * 0.999;
    let mut mesh = IndexedMesh::new(None, Vec::new(), Vec::new());
    let name = match tool {
        MachineTool::Orientation => {
            if build[0] < 80.0 || build[1] < 60.0 {
                return Err(CalibrationError::MachineSize);
            }
            let (x, y) = (build[0] / 2.0 - 3.0, build[1] / 2.0 - 3.0);
            for i in (-(x as i32) / 10)..=((x as i32) / 10) {
                let a = f64::from(i) * 10.0;
                append(
                    &mut mesh,
                    box_mesh([a - 0.15, -y + 8.0, 0.0], [a + 0.15, y - 8.0, layer_height]),
                );
            }
            for i in (-(y as i32) / 10)..=((y as i32) / 10) {
                let a = f64::from(i) * 10.0;
                if a.abs() > y - 8.0 {
                    continue;
                }
                append(
                    &mut mesh,
                    box_mesh([-x, a - 0.15, 0.0], [x, a + 0.15, layer_height]),
                );
            }
            // Blank label panels in the grid make the large corner words readable.
            for (text, px, py) in [
                ("FRONT-LEFT", -x, -y + 1.0),
                ("FRONT-RIGHT", x - 55.0, -y + 1.0),
                ("BACK-LEFT", -x, y - 6.0),
                ("BACK-RIGHT", x - 52.0, y - 6.0),
            ] {
                flat_text(&mut mesh, text, [px, py], 6.0, layer_height);
            }
            append(
                &mut mesh,
                box_mesh([3.0, 3.0, 0.0], [28.0, 4.0, layer_height]),
            );
            append(
                &mut mesh,
                box_mesh([3.0, 3.0, 0.0], [4.0, 23.0, layer_height]),
            );
            for i in 0..5 {
                let d = f64::from(i) * 0.5;
                append(
                    &mut mesh,
                    box_mesh([25.0 + d, 1.0 + d, 0.0], [25.6 + d, 6.0 - d, layer_height]),
                );
                append(
                    &mut mesh,
                    box_mesh([1.0 + d, 20.0 + d, 0.0], [6.0 - d, 20.6 + d, layer_height]),
                );
            }
            flat_text(&mut mesh, "+X", [18.0, 7.0], 5.0, layer_height);
            flat_text(&mut mesh, "+Y", [7.0, 20.0], 5.0, layer_height);
            flat_text(&mut mesh, "10 mm GRID", [-18.0, -8.0], 3.0, layer_height);
            "LCD orientation - FRONT is -Y"
        }
        MachineTool::XyRuler => {
            if build[0] < 110.0 || build[1] < 110.0 || build[2] < 2.0 {
                return Err(CalibrationError::MachineSize);
            }
            append(&mut mesh, box_mesh([-50.0, -50.0, 0.0], [50.0, -46.0, 1.0]));
            append(&mut mesh, box_mesh([-50.0, -50.0, 0.0], [-46.0, 50.0, 1.0]));
            for i in 0..=100 {
                let a = -50.0 + f64::from(i);
                let len = if i % 10 == 0 {
                    4.0
                } else if i % 5 == 0 {
                    2.5
                } else {
                    1.5
                };
                append(
                    &mut mesh,
                    box_mesh([a - 0.15, -46.1, 0.0], [a + 0.15, -46.0 + len, 1.0]),
                );
                append(
                    &mut mesh,
                    box_mesh([-46.1, a - 0.15, 0.0], [-46.0 + len, a + 0.15, 1.0]),
                );
            }
            flat_text(&mut mesh, "X 100 mm", [-20.0, -38.0], 4.0, 1.0);
            flat_text(&mut mesh, "Y 100 mm", [-38.0, 35.0], 4.0, 1.0);
            "XY rulers - measure 0 to 100 tick centres"
        }
        MachineTool::ZTower => {
            if build[0] < 50.0 || build[1] < 20.0 || build[2] < 25.0 {
                return Err(CalibrationError::MachineSize);
            }
            for i in 0..5 {
                let x = -20.0 + f64::from(i) * 8.0;
                let h = f64::from(i + 1) * 5.0;
                append(&mut mesh, box_mesh([x, -6.0, 0.0], [x + 8.0, 6.0, h]));
                flat_text(
                    &mut mesh,
                    &format!("{}", (i + 1) * 5),
                    [x + 1.0, -10.0],
                    3.0,
                    0.5,
                );
            }
            "Z check - 5 10 15 20 25 mm"
        }
    };
    let mut scene = Scene::new();
    let id = scene
        .add_leaf(
            Some(sc_scene::SceneMetadata {
                name: Some(name.into()),
                comment: Some(format!("sleptic-machine-calibration\n{tool:?}")),
            }),
            Transform3D::Identity,
            LeafPayload::Mesh(Arc::new(mesh)),
        )
        .map_err(CalibrationError::Scene)?;
    scene.add_root(id).map_err(CalibrationError::Scene)?;
    Ok(scene)
}
