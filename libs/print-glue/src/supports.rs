// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Deterministic world-space resin support geometry.
//!
//! Callers supply a placed, lifted mesh. Shafts obstructed by lower model geometry
//! along their centreline are skipped. Full swept-radius clearance is not tested.
//! Raster-island completion is a separate application pass, not implied by this
//! geometric sampler. All generated meshes are independent closed scene leaves.

use sc_mesh_core::geometry::compute_normal_from_triangle_vertices;
use sc_mesh_core::{IndexedMesh, IndexedTriangle, Vertex};
use sc_print_profiles::support_settings::{InvalidSupportSettings, SupportSettings};

#[path = "supports_standard.rs"]
mod standard;

/// A contact on the lifted mesh surface, in world millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SupportPoint {
    /// Surface position.
    pub position: [f32; 3],
    /// Outward geometric unit normal.
    pub normal: [f32; 3],
}

/// Closed columns or shared branching trunks, and an optional closed raft.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SupportStructure {
    /// All accepted contacts. Leading entries identify each column or trunk.
    pub points: Vec<SupportPoint>,
    /// One mesh per column or trunk, containing closed overlapping branch shells.
    pub columns: Vec<IndexedMesh>,
    /// Chamfered slab under the base footprint plus margin.
    pub raft: Option<IndexedMesh>,
    /// The branches of the trees in `columns`.
    pub(crate) paths: Vec<BranchPath>,
    /// The plate contact the model needs from a lattice raft, mm2 (R105).
    pub(crate) raft_target_mm2: f64,
}

impl SupportStructure {
    /// How far each tree branch leans from vertical, in degrees.
    pub fn branch_inclinations_deg(&self) -> Vec<f64> {
        self.paths.iter().map(|p| p.inclination_deg).collect()
    }
}

/// Invalid input or a requested sampling workload too large to allocate safely.
#[derive(Clone, Debug, PartialEq)]
pub enum SupportError {
    /// Invalid process settings.
    Settings(InvalidSupportSettings),
    /// Mesh is nonfinite or has invalid faces.
    InvalidMesh,
    /// Contact is nonfinite, has no normal, or leaves no room for a column.
    InvalidContact,
    /// Sampling would exceed one million grid candidates in total.
    SamplingLimit,
    /// Dimensions lose precision when converted to the mesh's f32 coordinates.
    UnrepresentableGeometry,
    /// Scene expansion or editing failed.
    Scene(sc_scene::SceneError),
    /// Slicing or contour merge failed during island completion.
    Slice(String),
}

impl std::fmt::Display for SupportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SupportError::Settings(err) => write!(f, "{err}"),
            SupportError::InvalidMesh => write!(f, "invalid support input mesh"),
            SupportError::InvalidContact => write!(f, "invalid support contact"),
            SupportError::SamplingLimit => write!(f, "support sampling limit exceeded"),
            SupportError::UnrepresentableGeometry => {
                write!(f, "support dimensions cannot be represented by the mesh")
            }
            SupportError::Scene(err) => write!(f, "{err}"),
            SupportError::Slice(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for SupportError {}

/// How auto supports treat one model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Prints as it stands: no lift, no supports.
    Direct,
    /// Flat base on the plate: stays down; only real overhangs get columns,
    /// standing on the plate (or on the model below), with no raft.
    Anchored,
    /// No usable base: lifted by `lift_mm` onto supports and a raft.
    Lifted,
}

/// Decide [`Placement`] for one world-space model. With a profile, "direct"
/// also needs the raster check to find no islands.
pub fn placement(
    mesh: &IndexedMesh,
    settings: &SupportSettings,
    profile: Option<&sc_sl1cer::profile::SliceProfile>,
) -> Placement {
    if crate::orient::prints_without_supports(mesh, settings.overhang_angle_deg)
        && profile.is_none_or(|profile| direct_on_grid(mesh, profile))
    {
        Placement::Direct
    } else if crate::orient::anchored_on_plate(mesh) {
        Placement::Anchored
    } else {
        Placement::Lifted
    }
}

type Placements = std::collections::BTreeMap<sc_scene::NodeId, Placement>;

/// Per-model automatic support outcome for frontend result messages.
#[derive(Clone, Debug)]
pub struct ModelSupportResult {
    /// Scene display name, if present.
    pub name: String,
    /// Generated contacts owned by this model.
    pub contacts: usize,
    /// Generated column meshes, excluding small bridges.
    pub columns: usize,
    /// Short bridges whose height is below the normal base and tip length.
    pub small: usize,
    /// Model remains on the plate with no lift or raft.
    pub direct: bool,
}

/// The model leaves [`model_results`] reports, in the same order.
pub fn model_ids(scene: &sc_scene::Scene) -> Vec<sc_scene::NodeId> {
    scene
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .unwrap_or_default()
        .into_iter()
        .filter(|&id| {
            scene.node(id).is_some_and(|n| {
                !n.meta
                    .as_ref()
                    .and_then(|m| m.comment.as_deref())
                    .is_some_and(|s| s.starts_with("sc-generated-support"))
                    && sc_scene::bake_leaf_to_world(scene, id).is_ok()
            })
        })
        .collect()
}

/// Report direct models alongside supported neighbours, using actual ownership.
pub fn model_results(
    scene: &sc_scene::Scene,
    structure: &SupportStructure,
    settings: &SupportSettings,
) -> Vec<ModelSupportResult> {
    let owners = support_owners(scene, &structure.points);
    scene
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .unwrap_or_default()
        .into_iter()
        .filter_map(|id| {
            let node = scene.node(id)?;
            if node
                .meta
                .as_ref()
                .and_then(|m| m.comment.as_deref())
                .is_some_and(|s| s.starts_with("sc-generated-support"))
            {
                return None;
            }
            sc_scene::bake_leaf_to_world(scene, id).ok()?;
            Some(ModelSupportResult {
                name: node
                    .meta
                    .as_ref()
                    .and_then(|m| m.name.clone())
                    .unwrap_or_else(|| format!("Model {}", id.get())),
                contacts: owners.iter().filter(|owner| **owner == Some(id)).count(),
                columns: structure
                    .columns
                    .iter()
                    .enumerate()
                    .filter(|(i, m)| {
                        owners.get(*i) == Some(&Some(id)) && !small_support(m, settings)
                    })
                    .count(),
                small: structure
                    .columns
                    .iter()
                    .enumerate()
                    .filter(|(i, m)| {
                        owners.get(*i) == Some(&Some(id)) && small_support(m, settings)
                    })
                    .count(),
                // Nothing generated for it: it prints on the plate as it is
                // (a flat-based part whose overhangs were all too low or
                // bridged reads "0 supports" otherwise).
                direct: !owners.contains(&Some(id)),
            })
        })
        .collect()
}

/// Lift a copy of a placed scene and add automatic columns and raft as new
/// leaves. The input stays unchanged on success and on error. This is geometric
/// placement only; callers must complete and verify raster islands separately.
pub fn geometric_supported_scene(
    source: &sc_scene::Scene,
    settings: &SupportSettings,
) -> Result<(sc_scene::Scene, SupportStructure), SupportError> {
    geometric_supported_scene_on_grid(source, settings, None, &Default::default())
}

fn geometric_supported_scene_on_grid(
    source: &sc_scene::Scene,
    settings: &SupportSettings,
    profile: Option<&sc_sl1cer::profile::SliceProfile>,
    lift: &std::collections::BTreeSet<sc_scene::NodeId>,
) -> Result<(sc_scene::Scene, SupportStructure), SupportError> {
    let (scene, placements) = lifted_scene(source, settings, profile, lift)?;
    geometric_supported_lifted(
        scene,
        &placements,
        settings,
        profile,
        &IndexCache::default(),
    )
}

/// [`geometric_supported_scene_on_grid`] on a scene already placed by
/// [`lifted_scene`]: the support pass places it once (R92; it was placed
/// twice, and placing slices every part that might print on the plate).
fn geometric_supported_lifted(
    mut scene: sc_scene::Scene,
    placements: &Placements,
    settings: &SupportSettings,
    profile: Option<&sc_sl1cer::profile::SliceProfile>,
    cache: &IndexCache,
) -> Result<(sc_scene::Scene, SupportStructure), SupportError> {
    let mut sampling = *settings;
    sampling.symmetric_supports = false;
    let mut points = Vec::new();
    let fills = fill_copies(&scene);
    for (&id, &place) in placements {
        if place == Placement::Direct || fills.contains_key(&id) {
            continue;
        }
        let mesh = sc_scene::bake_leaf_to_world(&scene, id).map_err(SupportError::Scene)?;
        points.extend(sample_overhangs(&mesh, &sampling)?);
    }
    apply_painted_regions(&scene, settings, &mut points)?;
    if settings.symmetric_supports {
        mirror_scene_contacts(&scene, &mut points)?;
        apply_painted_regions(&scene, settings, &mut points)?;
    }
    let (structure, rafts) =
        build_per_model(&scene, placements, &points, settings, 1e-4, profile, cache)?;
    append_structure(&mut scene, &structure, &rafts, placements, settings)?;
    Ok((scene, structure))
}

/// Overhang contacts of a model that is not printing directly: the sampler
/// without [`sample_contacts`]'s whole-model "prints directly" early out.
pub fn sample_overhang_contacts(
    mesh: &IndexedMesh,
    settings: &SupportSettings,
) -> Result<Vec<SupportPoint>, SupportError> {
    sample_contacts_inner(mesh, settings, None, None)
}

/// [`sample_contacts`] without its whole-model "prints directly" early out:
/// an anchored model is not direct, but still has overhangs to sample.
fn sample_overhangs(
    mesh: &IndexedMesh,
    settings: &SupportSettings,
) -> Result<Vec<SupportPoint>, SupportError> {
    sample_contacts_inner(mesh, settings, None, None)
}

/// Build each model's columns (and, for lifted models, its own raft) from the
/// contacts it owns. Anchored models stand on the plate with no raft; contacts
/// too low for a column on them are dropped (those overhangs sit on the base).
/// Rafts are kept inside the profile's plate when one is given.
fn build_per_model(
    scene: &sc_scene::Scene,
    placements: &Placements,
    contacts: &[SupportPoint],
    settings: &SupportSettings,
    tolerance: f64,
    profile: Option<&sc_sl1cer::profile::SliceProfile>,
    cache: &IndexCache,
) -> Result<(SupportStructure, Vec<(sc_scene::NodeId, IndexedMesh)>), SupportError> {
    let owners = support_owners(scene, contacts);
    let fills = fill_copies(scene);
    let mut groups: std::collections::BTreeMap<sc_scene::NodeId, Vec<SupportPoint>> =
        std::collections::BTreeMap::new();
    for (point, owner) in contacts.iter().zip(owners) {
        // A Fill plate copy gets its part's supports pasted below.
        if let Some(owner) = owner.filter(|o| !fills.contains_key(o)) {
            groups.entry(owner).or_default().push(*point);
        }
    }
    let mut built: std::collections::BTreeMap<
        sc_scene::NodeId,
        (std::ops::Range<usize>, std::ops::Range<usize>),
    > = std::collections::BTreeMap::new();
    let mut merged = SupportStructure::default();
    let mut rafts = Vec::new();
    // Each part's lattice raft target, for its fill copies.
    let mut targets: std::collections::BTreeMap<sc_scene::NodeId, f64> = Default::default();
    // Columns are checked against every model on the plate, so one standing
    // under another model lands on it instead of drilling through it.
    let plate_index = match cache.plate.get() {
        Some(index) => index.clone(),
        None => {
            let Some(all_models) = scene.merge_meshes().map_err(SupportError::Scene)? else {
                return Ok((merged, rafts));
            };
            let index = std::rc::Rc::new(ModelIndex::new(&all_models)?);
            let _ = cache.plate.set(index.clone());
            index
        }
    };
    for (owner, mut points) in groups {
        // Raster completion can still find an island on a "direct" model
        // (sub-pixel neck); support it from the plate like an anchored one.
        let place = placements.get(&owner).copied().unwrap_or(Placement::Lifted);
        let mut own = *settings;
        own.raft = settings.raft && place == Placement::Lifted;
        let floor = if own.raft { own.raft_thickness_mm } else { 0.0 };
        points.retain(|p| f64::from(p.position[2]) > floor + own.lowest_contact_mm());
        if points.is_empty() {
            continue;
        }
        let cached = cache.own.borrow().get(&owner).cloned();
        let own_index = match cached {
            Some(index) => index,
            None => {
                let model =
                    sc_scene::bake_leaf_to_world(scene, owner).map_err(SupportError::Scene)?;
                let index = std::rc::Rc::new(ModelIndex::new(&model)?);
                cache.own.borrow_mut().insert(owner, index.clone());
                index
            }
        };
        let plate = profile.map(|profile| {
            let volume = profile.build_volume();
            [volume.x_mm() / 2.0, volume.y_mm() / 2.0]
        });
        let mut structure = build_supports_indexed(
            &plate_index,
            Some(&own_index),
            &points,
            &own,
            tolerance,
            plate,
        )?;
        if let Some(profile) = profile {
            let volume = profile.build_volume();
            keep_bases_on_plate(
                &mut structure.columns,
                [volume.x_mm() / 2.0, volume.y_mm() / 2.0],
                &own,
            );
        }
        // Columns and contacts are counted apart: a tree has fewer columns
        // than contacts, and copies took the next part's columns (R84).
        built.insert(
            owner,
            (
                merged.columns.len()..merged.columns.len() + structure.columns.len(),
                merged.points.len()..merged.points.len() + structure.points.len(),
            ),
        );
        let feet = raft_feet(&structure.columns, floor);
        let target = structure.raft_target_mm2;
        targets.insert(owner, target);
        merged.columns.extend(structure.columns);
        merged.points.extend(structure.points);
        merged.paths.extend(structure.paths);
        if let Some(raft) = structure.raft {
            let raft = match profile {
                Some(profile) if !feet.is_empty() => {
                    let volume = profile.build_volume();
                    make_raft(
                        &feet,
                        &own,
                        Some([volume.x_mm() / 2.0, volume.y_mm() / 2.0]),
                        target,
                    )?
                }
                _ => raft,
            };
            rafts.push((owner, raft));
        }
    }
    // Fill plate copies: the part's columns, moved onto each copy (Sleptic:
    // work a filled plate's part out once and copy the result). Each copy's
    // raft is its own footprint, so it still stays on the plate.
    for (copy, (part, d)) in &fills {
        let Some((columns, contacts)) = built.get(part).cloned() else {
            continue;
        };
        let moved: Vec<SupportPoint> = contacts
            .map(|k| {
                let mut p = merged.points[k];
                for (axis, offset) in d.iter().enumerate() {
                    p.position[axis] += *offset as f32;
                }
                p
            })
            .collect();
        let first_column = merged.columns.len();
        for k in columns {
            let mut column = merged.columns[k].clone();
            for v in &mut column.vertices {
                *v = Vertex::new([v[0] + d[0] as f32, v[1] + d[1] as f32, v[2] + d[2] as f32]);
            }
            merged.columns.push(column);
        }
        merged.points.extend(moved.iter().copied());
        let copy_feet = raft_feet(
            &merged.columns[first_column..],
            if settings.raft {
                settings.raft_thickness_mm
            } else {
                0.0
            },
        );
        if let Some(raft) = rafts
            .iter()
            .find(|(o, _)| o == part)
            .map(|(_, r)| r.clone())
        {
            let mut own = *settings;
            own.raft = true;
            let raft = match profile {
                Some(profile) if !copy_feet.is_empty() => {
                    let volume = profile.build_volume();
                    make_raft(
                        &copy_feet,
                        &own,
                        Some([volume.x_mm() / 2.0, volume.y_mm() / 2.0]),
                        targets.get(part).copied().unwrap_or(0.0),
                    )?
                }
                _ => {
                    let mut raft = raft;
                    for v in &mut raft.vertices {
                        *v = Vertex::new([v[0] + d[0] as f32, v[1] + d[1] as f32, v[2]]);
                    }
                    raft
                }
            };
            rafts.push((*copy, raft));
        }
    }
    merged.raft = merge_meshes(rafts.iter().map(|(_, m)| m));
    Ok((merged, rafts))
}

/// Moves the base of each column standing on the plate inward until it is
/// on the plate (`half`: plate half width and depth, scene XY is
/// plate-centred). A part as deep as the plate had 4 mm base pads under
/// overhangs at its edges reaching 1.3 mm past it (Sleptic's motor mount: red
/// walls after Auto prepare). Only the base and the trunk's foot move (the
/// shaft leans a little); the contact stays where it is. Whatever is still
/// past the edge (a wide shaft right at the part's edge) is cut flat at it,
/// as the printer's screen would: nothing cures past the plate.
fn keep_bases_on_plate(columns: &mut [IndexedMesh], half: [f64; 2], settings: &SupportSettings) {
    // The base pad, and a tree trunk's foot and branch roots, all sit at or
    // below the base top (plus the trunk's 0.1 mm collar).
    let top = settings.base_height_mm + 0.1 + 1e-3;
    for column in columns {
        let low = || column.vertices.iter().filter(|v| f64::from(v[2]) <= top);
        let on_plate = column
            .vertices
            .iter()
            .map(|v| f64::from(v[2]))
            .fold(f64::INFINITY, f64::min)
            <= 1e-3;
        let mut shift = [0.0f64; 2];
        for axis in (0..2).filter(|_| on_plate) {
            let lo = low()
                .map(|v| f64::from(v[axis]))
                .fold(f64::INFINITY, f64::min);
            let hi = low()
                .map(|v| f64::from(v[axis]))
                .fold(f64::NEG_INFINITY, f64::max);
            if hi - lo > 2.0 * half[axis] {
                continue;
            }
            if lo < -half[axis] {
                shift[axis] = -half[axis] - lo;
            } else if hi > half[axis] {
                shift[axis] = half[axis] - hi;
            }
        }
        // Past the plate by more than a base: the column itself is off the
        // plate, which moving its foot does not fix.
        if shift.iter().any(|d| d.abs() > settings.base_diameter_mm) {
            shift = [0.0, 0.0];
        }
        let off = |v: &Vertex| (0..2).any(|axis| f64::from(v[axis]).abs() > half[axis]);
        if shift == [0.0, 0.0] && !column.vertices.iter().any(off) {
            continue;
        }
        let vertices = column
            .vertices
            .iter()
            .map(|v| {
                let d = if f64::from(v[2]) <= top {
                    shift
                } else {
                    [0.0, 0.0]
                };
                Vertex::new([
                    (f64::from(v[0]) + d[0]).clamp(-half[0], half[0]) as f32,
                    (f64::from(v[1]) + d[1]).clamp(-half[1], half[1]) as f32,
                    v[2],
                ])
            })
            .collect();
        let faces = column
            .faces
            .iter()
            .map(|f| f.vertices.map(|i| i.get()))
            .collect();
        // Leaning faces get fresh normals.
        if let Ok(moved) = recompute_mesh(vertices, faces) {
            *column = moved;
        }
    }
}

/// Fill plate copies (tagged by Fill plate, see `FILL_COPY_TAG` in the GUI):
/// copy -> (the part it copies, world offset from part to copy). Only a copy
/// that still has its part's mesh and the same turn and scale counts; one the
/// user has turned or scaled since is supported on its own.
pub fn fill_copies(
    scene: &sc_scene::Scene,
) -> std::collections::BTreeMap<sc_scene::NodeId, (sc_scene::NodeId, [f64; 3])> {
    use sc_mesh_core::transform::Transform3D;
    let mut out = std::collections::BTreeMap::new();
    let Ok(leaves) = scene.reachable_leaves(
        sc_scene::CsgPolicy::Descend,
        sc_scene::ReferencePolicy::Skip,
    ) else {
        return out;
    };
    let pose = |id: sc_scene::NodeId| {
        let mesh = std::sync::Arc::as_ptr(scene.leaf_mesh_arc(id)?) as usize;
        Some(match scene.world_transform_of(id).ok()? {
            Transform3D::Affine(a) => (mesh, *a.linear(), *a.translation()),
            Transform3D::Identity => (
                mesh,
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                [0.0; 3],
            ),
        })
    };
    for &copy in &leaves {
        let Some(part) = scene
            .node(copy)
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.comment.as_deref())
            .and_then(|c| c.lines().find_map(|l| l.strip_prefix("sc-fill-copy-of = ")))
            .and_then(|v| v.trim().parse::<u32>().ok())
            .and_then(|n| {
                leaves
                    .iter()
                    .copied()
                    .find(|id| id.get() == n && *id != copy)
            })
        else {
            continue;
        };
        let (Some((m1, l1, t1)), Some((m2, l2, t2))) = (pose(part), pose(copy)) else {
            continue;
        };
        if m1 == m2 && (0..3).all(|r| (0..3).all(|c| (l1[r][c] - l2[r][c]).abs() < 1e-9)) {
            out.insert(copy, (part, [t2[0] - t1[0], t2[1] - t1[1], t2[2] - t1[2]]));
        }
    }
    out
}

/// Per model, the offsets to its identical copies: models with the same
/// mesh, turn and scale, placed the same way.
fn identical_copies(
    scene: &sc_scene::Scene,
    placements: &Placements,
) -> std::collections::BTreeMap<sc_scene::NodeId, Vec<[f64; 3]>> {
    use sc_mesh_core::transform::Transform3D;
    let pose = |id: sc_scene::NodeId| {
        let mesh = std::sync::Arc::as_ptr(scene.leaf_mesh_arc(id)?) as usize;
        Some(match scene.world_transform_of(id).ok()? {
            Transform3D::Affine(a) => (mesh, *a.linear(), *a.translation()),
            Transform3D::Identity => (
                mesh,
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                [0.0; 3],
            ),
        })
    };
    let poses: Vec<_> = placements
        .iter()
        .filter_map(|(&id, &place)| Some((id, place, pose(id)?)))
        .collect();
    let mut out = std::collections::BTreeMap::new();
    for (id, place, (mesh, linear, t)) in &poses {
        let offsets: Vec<[f64; 3]> = poses
            .iter()
            .filter(|(other, other_place, (other_mesh, other_linear, _))| {
                other != id
                    && other_place == place
                    && other_mesh == mesh
                    && (0..3)
                        .all(|r| (0..3).all(|c| (linear[r][c] - other_linear[r][c]).abs() < 1e-9))
            })
            .map(|(_, _, (_, _, u))| [u[0] - t[0], u[1] - t[1], u[2] - t[2]])
            .collect();
        if !offsets.is_empty() {
            out.insert(*id, offsets);
        }
    }
    out
}

/// `scene` without the Fill plate copies and the supports they own: the
/// island checks look at each filled part once, not at every copy.
fn without_copies(
    scene: &sc_scene::Scene,
    fills: &std::collections::BTreeMap<sc_scene::NodeId, (sc_scene::NodeId, [f64; 3])>,
) -> Result<sc_scene::Scene, SupportError> {
    if fills.is_empty() {
        return Ok(scene.clone());
    }
    let mut out = scene.clone();
    let leaves = scene
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .map_err(SupportError::Scene)?;
    for id in leaves.into_iter().rev() {
        let owner = scene
            .node(id)
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.comment.as_deref())
            .filter(|c| c.starts_with("sc-generated-support"))
            .and_then(|c| c.lines().find_map(|l| l.strip_prefix("owner = ")))
            .and_then(|v| v.trim().parse::<u32>().ok());
        let copy =
            fills.contains_key(&id) || owner.is_some_and(|o| fills.keys().any(|k| k.get() == o));
        if copy {
            out.remove_leaf(id).map_err(SupportError::Scene)?;
        }
    }
    Ok(out)
}

/// Several closed meshes as one mesh (shells stay separate).
fn merge_meshes<'a>(meshes: impl Iterator<Item = &'a IndexedMesh>) -> Option<IndexedMesh> {
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    for mesh in meshes {
        let offset = vertices.len() as u32;
        vertices.extend(mesh.vertices.iter().copied());
        faces.extend(mesh.faces.iter().map(|f| {
            IndexedTriangle::from_raw_indices(f.normal, f.vertices.map(|i| i.get() + offset))
        }));
    }
    (!faces.is_empty()).then(|| IndexedMesh::new(None, vertices, faces))
}

fn direct_on_grid(mesh: &IndexedMesh, profile: &sc_sl1cer::profile::SliceProfile) -> bool {
    let mut scene = sc_scene::Scene::new();
    let Ok(id) = scene.add_leaf(
        None,
        sc_mesh_core::transform::Transform3D::Identity,
        sc_scene::LeafPayload::Mesh(std::sync::Arc::new(mesh.clone())),
    ) else {
        return false;
    };
    if scene.add_root(id).is_err() {
        return false;
    }
    let Ok(sc_sl1cer::SliceOutcome::Completed(leaves)) = sc_sl1cer::api::slice_scene_with_fallback(
        &scene,
        profile,
        &sc_sl1cer::CancelToken::new(),
        |_| {},
        true,
    ) else {
        return false;
    };
    let Ok(stack) =
        sc_sl1cer::contour::merge_stacks(leaves.into_iter().map(|l| l.into_stack()).collect())
    else {
        return false;
    };
    sc_print_job::find_islands(&stack, profile).is_empty()
}

/// `force_lift`: models lifted onto supports whatever their placement check
/// says (their islands sit too low to support where they stand).
fn lifted_scene(
    source: &sc_scene::Scene,
    settings: &SupportSettings,
    profile: Option<&sc_sl1cer::profile::SliceProfile>,
    force_lift: &std::collections::BTreeSet<sc_scene::NodeId>,
) -> Result<(sc_scene::Scene, Placements), SupportError> {
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};

    settings.validate().map_err(SupportError::Settings)?;
    let mut scene = source.clone();
    let mut placements = Placements::new();
    let lift = Transform3D::Affine(AffineTransform3D::from_translation([
        0.0,
        0.0,
        settings.lift_mm,
    ]));
    let fills = fill_copies(source);
    let mut roots = source
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .map_err(SupportError::Scene)?;
    // Parts first, so each Fill plate copy takes its part's placement
    // instead of repeating the check (it slices the part on a profile).
    roots.sort_by_key(|id| fills.contains_key(id));
    for root in &roots {
        // Direct and flat-based models stay at their real Z; only models
        // with no usable base are lifted onto supports and a raft.
        let place = match fills.get(root).and_then(|(part, _)| placements.get(part)) {
            Some(&place) => place,
            None if force_lift.contains(root) => Placement::Lifted,
            None => {
                let mesh =
                    sc_scene::bake_leaf_to_world(source, *root).map_err(SupportError::Scene)?;
                placement(&mesh, settings, profile)
            }
        };
        placements.insert(*root, place);
        if place != Placement::Lifted {
            continue;
        }
        let current = source
            .node(*root)
            .ok_or(SupportError::InvalidMesh)?
            .local_transform;
        let world = source
            .world_transform_of(*root)
            .map_err(SupportError::Scene)?;
        let parent = world.compose(&current.inverse().map_err(|_| SupportError::InvalidMesh)?);
        let after = parent
            .inverse()
            .map_err(|_| SupportError::InvalidMesh)?
            .compose(&lift)
            .compose(&world);
        scene
            .set_local_transform(*root, after)
            .map_err(SupportError::Scene)?;
    }
    Ok((scene, placements))
}

fn small_support(mesh: &IndexedMesh, settings: &SupportSettings) -> bool {
    let lo = mesh
        .vertices
        .iter()
        .map(|v| v[2])
        .fold(f32::INFINITY, f32::min);
    let hi = mesh
        .vertices
        .iter()
        .map(|v| v[2])
        .fold(f32::NEG_INFINITY, f32::max);
    f64::from(hi - lo) < settings.base_height_mm + settings.tip_length_mm
}

fn append_structure(
    scene: &mut sc_scene::Scene,
    structure: &SupportStructure,
    rafts: &[(sc_scene::NodeId, IndexedMesh)],
    placements: &Placements,
    settings: &SupportSettings,
) -> Result<(), SupportError> {
    use sc_mesh_core::transform::Transform3D;
    use sc_scene::LeafPayload;
    use std::sync::Arc;
    let owners = support_owners(scene, &structure.points);
    // Removal lowers a model by the lift recorded on its supports: anchored
    // models were never lifted, so they record 0.
    let lift_of = |owner: Option<sc_scene::NodeId>| {
        if owner.and_then(|o| placements.get(&o)) == Some(&Placement::Lifted) {
            settings.lift_mm
        } else {
            0.0
        }
    };
    for (index, mesh) in structure.columns.iter().enumerate() {
        let point = structure.points[index];
        let meta = sc_scene::node::SceneMetadata {
            name: Some(format!("Support {}", index + 1)),
            comment: Some(format!(
                "sc-generated-support\nposition = {:?}\nnormal = {:?}\nlift_mm = {}\nowner = {}\nsmall = {}\n",
                point.position,
                point.normal,
                lift_of(owners[index]),
                owners[index].map_or(0, |id| id.get()),
                small_support(mesh, settings)
            )),
        };
        let leaf = scene
            .add_leaf(
                Some(meta),
                Transform3D::Identity,
                LeafPayload::Mesh(Arc::new(mesh.clone())),
            )
            .map_err(SupportError::Scene)?;
        scene.add_root(leaf).map_err(SupportError::Scene)?;
    }
    for (owner, raft) in rafts {
        let meta = sc_scene::SceneMetadata {
            name: Some("Support raft".into()),
            // The model's lift is recorded on its columns (3MF projects keep
            // only the owner on rafts).
            comment: Some(format!(
                "sc-generated-support-raft\nowner = {}\n",
                owner.get()
            )),
        };
        let leaf = scene
            .add_leaf(
                Some(meta),
                Transform3D::Identity,
                LeafPayload::Mesh(Arc::new(raft.clone())),
            )
            .map_err(SupportError::Scene)?;
        scene.add_root(leaf).map_err(SupportError::Scene)?;
    }
    Ok(())
}

/// Generate geometric supports, then complete raster islands with spaced seeds.
/// A final unrestricted safety pass retains mandatory coverage. The resolved grid is retained; its layer count is extended if a
/// geometric direct pose needs lift after raster verification. The returned islands
/// are the final verification result; callers must not assume convergence.
pub fn auto_supported_scene(
    source: &sc_scene::Scene,
    settings: &SupportSettings,
    profile: &sc_sl1cer::profile::SliceProfile,
) -> Result<(sc_scene::Scene, SupportStructure, Vec<sc_print_job::Island>), SupportError> {
    let (scene, structure, analysis) = auto_supported_scene_analyzed(source, settings, profile)?;
    Ok((scene, structure, analysis.islands))
}

/// Like [`auto_supported_scene`], retaining the final raster analysis so callers
/// can report islands and estimate volume without rasterizing the same scene again.
/// The same grid must be used for final export, with the layer count derived
/// from the returned scene if raster verification required an additional lift.
pub fn auto_supported_scene_analyzed(
    source: &sc_scene::Scene,
    settings: &SupportSettings,
    profile: &sc_sl1cer::profile::SliceProfile,
) -> Result<
    (
        sc_scene::Scene,
        SupportStructure,
        sc_print_job::LayerAnalysis,
    ),
    SupportError,
> {
    auto_supported_scene_analyzed_with_fallback(source, settings, profile, false)
}

/// Retain support-completion analysis while optionally recovering non-manifold leaves.
pub fn auto_supported_scene_analyzed_with_fallback(
    source: &sc_scene::Scene,
    settings: &SupportSettings,
    profile: &sc_sl1cer::profile::SliceProfile,
    allow_tolerant: bool,
) -> Result<
    (
        sc_scene::Scene,
        SupportStructure,
        sc_print_job::LayerAnalysis,
    ),
    SupportError,
> {
    // Islands are found with the same overhang angle the contacts use.
    let profile = &profile
        .clone()
        .with_support_overhang_angle(settings.overhang_angle_deg);
    // A part standing on the plate whose islands sit below the shortest
    // column it could have there kept them unsupported (Sleptic's priority
    // one: the part must be held; a drone camera mount was left with two).
    // Such parts are lifted onto supports and a raft, and the plate is
    // supported again.
    let mut low = std::collections::BTreeSet::new();
    let first = auto_supported_pass(
        source,
        settings,
        profile,
        allow_tolerant,
        &Default::default(),
        &mut low,
    )?;
    if low.is_empty() {
        return Ok(first);
    }
    auto_supported_pass(
        source,
        settings,
        profile,
        allow_tolerant,
        &low,
        &mut Default::default(),
    )
}

fn auto_supported_pass(
    source: &sc_scene::Scene,
    settings: &SupportSettings,
    profile: &sc_sl1cer::profile::SliceProfile,
    allow_tolerant: bool,
    lift: &std::collections::BTreeSet<sc_scene::NodeId>,
    low: &mut std::collections::BTreeSet<sc_scene::NodeId>,
) -> Result<
    (
        sc_scene::Scene,
        SupportStructure,
        sc_print_job::LayerAnalysis,
    ),
    SupportError,
> {
    let (base, placements) = lifted_scene(source, settings, Some(profile), lift)?;
    let has_models = !placements.is_empty();
    let fills = fill_copies(&base);
    // The models' search structures, built once for every pass below.
    let cache = IndexCache::default();
    let (mut scene, mut structure) =
        geometric_supported_lifted(base.clone(), &placements, settings, Some(profile), &cache)?;
    // A subpixel neck can fail raster verification after geometric planning.
    // Keep its resolved grid and extend the layer count to cover the required
    // lift; application exports derive their final schedule from the result.
    let sc_sl1cer::profile::LayerSchedule::Uniform(grid) = profile.layer_schedule();
    let height = base
        .aabb()
        .map_err(SupportError::Scene)?
        .map_or(0.0, |bb| f64::from(bb.max()[2]));
    let needed = crate::projection::total_layers_for_height(
        height,
        grid.layer_height().value(),
        grid.first_layer_height().value(),
    )
    .map_err(|e| SupportError::Slice(e.to_string()))?;
    let extended;
    let profile = if needed.get() > grid.total_layers().get() {
        extended = sc_sl1cer::profile::SliceProfile::new(
            *profile.build_volume(),
            *profile.pixel_resolution(),
            sc_sl1cer::profile::LayerSchedule::uniform(
                grid.layer_height().value(),
                grid.first_layer_height().value(),
                needed.get(),
            )
            .map_err(|e| SupportError::Slice(format!("{e:?}")))?,
            profile.mirror(),
            profile.aa_level(),
            profile.packing_mode(),
        )
        .with_accuracy(profile.accuracy())
        .with_calibration(profile.calibration())
        .with_support_overhang_angle(profile.support_overhang_angle_deg());
        &extended
    } else {
        profile
    };
    let b = profile.build_volume();
    let map = sc_sl1cer::raster::pixel_map::PixelMap::new(
        b.x(),
        b.y(),
        *profile.pixel_resolution(),
        profile.mirror(),
    );
    // Each model is checked for raster islands with its own corner on the
    // pixel grid (a nudge under one pixel), so a part rasterises the same
    // wherever it sits and copies of it come out the same on their own; a
    // plate position half a pixel over used to add or drop an island (Sleptic:
    // identical arranged parts got different supports). Contacts found are
    // moved back by the same nudge. Fill plate copies are left out of these
    // checks: they get their part's supports.
    let mut shifts = pixel_alignment(&base, &placements, &map)?;
    // The nudges only keep identical copies alike; with none on the plate
    // they only cost a second full slice (a part with no supports took
    // 6 s, half of it this pass).
    if identical_copies(&without_copies(&base, &fills)?, &placements).is_empty() {
        shifts.clear();
    }
    let mut aligned_base = without_copies(&shifted_scene(&base, &shifts)?, &fills)?;
    const COMPLETION_PASSES: usize = 12;
    for pass in 0..=COMPLETION_PASSES {
        let aligned = without_copies(&shifted_scene(&scene, &shifts)?, &fills)?;
        let (stack, analysis) = analyze(&aligned, profile, allow_tolerant)?;
        let done = analysis.islands.is_empty() || pass == COMPLETION_PASSES || !has_models;
        if done {
            // Then one safety check at the parts' real positions: a contact
            // moved back by its nudge can leave a pixel island beside it.
            if !shifts.is_empty() && pass < COMPLETION_PASSES && has_models {
                shifts.clear();
                aligned_base = without_copies(&base, &fills)?;
                continue;
            }
            return Ok(report(
                scene,
                structure,
                analysis,
                &shifts,
                &fills,
                profile,
                allow_tolerant,
            )?);
        }
        let mut contacts = structure.points.clone();
        // An island too low for a column (inside the raft/base/tip zone) cannot
        // be supported: it stays a reported island instead of failing the whole
        // job. A model hanging past the plate edge produces these where the
        // raster clips it (vice grips: islands at x = +/-96 mm).
        let lowest = settings.lowest_contact_mm();
        let before = contacts.len();
        // Complete the earliest spaced seeds, then re-slice. Adding every
        // future-layer island in one batch creates redundant pins: supporting
        // an early seed often connects those later regions through the model.
        let mut completion_seeds: Vec<[f32; 3]> = Vec::new();
        for island in &analysis.islands {
            let p = map.pixel_to_mm(island.contact_px);
            let z = stack.layers()[island.layer].z_mm();
            let mut position = [p.x as f32, p.y as f32, z as f32];
            // An island thinner than a pixel lights a pixel whose centre is
            // off the part: no model owned the contact, no column was built,
            // and the same pixel came back every pass (a drone camera mount
            // kept two islands). Move it onto the part.
            if support_owners(
                &aligned_base,
                &[SupportPoint {
                    position,
                    normal: [0.0, 0.0, -1.0],
                }],
            )[0]
            .is_none()
                && let Some(on) = snap_onto_model(&aligned_base, position)
            {
                position = on;
            }
            if z <= lowest {
                // Too low for a column under a part on the plate: note the
                // part so the plate is redone with it lifted.
                let point = SupportPoint {
                    position,
                    normal: [0.0, 0.0, -1.0],
                };
                if let Some(owner) = support_owners(&aligned_base, &[point])[0]
                    && placements.get(&owner) != Some(&Placement::Lifted)
                {
                    low.insert(owner);
                }
                continue;
            }
            if pass + 1 < COMPLETION_PASSES
                && completion_seeds.iter().any(|seed| {
                    let dx = f64::from(seed[0] - position[0]);
                    let dy = f64::from(seed[1] - position[1]);
                    let dz = f64::from(seed[2] - position[2]);
                    dx * dx + dy * dy + dz * dz < settings.spacing_mm * settings.spacing_mm
                })
            {
                continue;
            }
            completion_seeds.push(position);
            contacts.push(SupportPoint {
                position,
                normal: [0.0, 0.0, -1.0],
            });
        }
        if contacts.len() == before {
            return Ok(report(
                scene,
                structure,
                analysis,
                &shifts,
                &fills,
                profile,
                allow_tolerant,
            )?);
        }
        // An island on one part is on every identical copy of it: the
        // raster saw it on one copy only because their positions round a
        // couple of microns differently (six arranged copies, one got an
        // extra contact). Each copy gets the contact at the same spot.
        let copies = identical_copies(&aligned_base, &placements);
        if !copies.is_empty() {
            let found = contacts[before..].to_vec();
            for (point, owner) in found.iter().zip(support_owners(&aligned_base, &found)) {
                for d in owner.and_then(|o| copies.get(&o)).into_iter().flatten() {
                    let mut copy = *point;
                    for (axis, offset) in d.iter().enumerate() {
                        copy.position[axis] += *offset as f32;
                    }
                    let near = |q: &SupportPoint| {
                        (0..3)
                            .map(|a| f64::from(q.position[a] - copy.position[a]).powi(2))
                            .sum::<f64>()
                            < 0.05f64.powi(2)
                    };
                    if !contacts.iter().any(near) {
                        contacts.push(copy);
                    }
                }
            }
        }
        let owners = support_owners(&aligned_base, &contacts[before..]);
        for (point, owner) in contacts[before..].iter_mut().zip(owners) {
            if let Some(d) = owner.and_then(|o| shifts.get(&o)) {
                point.position[0] -= d[0];
                point.position[1] -= d[1];
            }
        }
        let sc_sl1cer::profile::LayerSchedule::Uniform(grid) = profile.layer_schedule();
        apply_painted_regions(&base, settings, &mut contacts)?;
        if settings.symmetric_supports {
            mirror_scene_contacts(&base, &mut contacts)?;
            apply_painted_regions(&base, settings, &mut contacts)?;
        }
        let (rebuilt, rafts) = build_per_model(
            &base,
            &placements,
            &contacts,
            settings,
            grid.layer_height()
                .value()
                .max(grid.first_layer_height().value())
                + 1e-4,
            Some(profile),
            &cache,
        )?;
        structure = rebuilt;
        scene = base.clone();
        append_structure(&mut scene, &structure, &rafts, &placements, settings)?;
    }
    unreachable!()
}

type Shifts = std::collections::BTreeMap<sc_scene::NodeId, [f32; 2]>;

/// Per model, the XY nudge (under one pixel) that puts its bounding-box
/// corner on the pixel grid of `map`.
fn pixel_alignment(
    base: &sc_scene::Scene,
    placements: &Placements,
    map: &sc_sl1cer::raster::pixel_map::PixelMap,
) -> Result<Shifts, SupportError> {
    let mut shifts = Shifts::new();
    for &id in placements.keys() {
        let mesh = sc_scene::bake_leaf_to_world(base, id).map_err(SupportError::Scene)?;
        let (mut x, mut y) = (f64::INFINITY, f64::INFINITY);
        for v in &mesh.vertices {
            x = x.min(f64::from(v[0]));
            y = y.min(f64::from(v[1]));
        }
        if !x.is_finite() {
            continue;
        }
        let (px, py) = map.mm_to_pixel(sc_sl1cer::geom2d::Point2D::new(x, y));
        let snapped = map.pixel_to_mm((px.round(), py.round()));
        shifts.insert(id, [(snapped.x - x) as f32, (snapped.y - y) as f32]);
    }
    Ok(shifts)
}

/// `scene` with every model, and the supports it owns, moved by its nudge.
fn shifted_scene(
    scene: &sc_scene::Scene,
    shifts: &Shifts,
) -> Result<sc_scene::Scene, SupportError> {
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
    let mut out = scene.clone();
    let leaves = scene
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .map_err(SupportError::Scene)?;
    for id in leaves {
        let owner = scene
            .node(id)
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.comment.as_deref())
            .and_then(|c| c.lines().find_map(|l| l.strip_prefix("owner = ")))
            .and_then(|v| v.trim().parse::<u32>().ok());
        let shift = match owner {
            Some(owner) => shifts
                .iter()
                .find(|(k, _)| k.get() == owner)
                .map(|(_, d)| *d),
            None => shifts.get(&id).copied(),
        };
        let Some([dx, dy]) = shift.filter(|d| d[0] != 0.0 || d[1] != 0.0) else {
            continue;
        };
        let current = scene
            .node(id)
            .ok_or(SupportError::InvalidMesh)?
            .local_transform;
        let world = scene.world_transform_of(id).map_err(SupportError::Scene)?;
        let parent = world.compose(&current.inverse().map_err(|_| SupportError::InvalidMesh)?);
        let moved = parent
            .inverse()
            .map_err(|_| SupportError::InvalidMesh)?
            .compose(&Transform3D::Affine(AffineTransform3D::from_translation([
                f64::from(dx),
                f64::from(dy),
                0.0,
            ])))
            .compose(&world);
        out.set_local_transform(id, moved)
            .map_err(SupportError::Scene)?;
    }
    Ok(out)
}

fn analyze(
    scene: &sc_scene::Scene,
    profile: &sc_sl1cer::profile::SliceProfile,
    allow_tolerant: bool,
) -> Result<
    (
        sc_sl1cer::contour::ContourStack,
        sc_print_job::LayerAnalysis,
    ),
    SupportError,
> {
    let outcome = sc_sl1cer::api::slice_scene_with_fallback(
        scene,
        profile,
        &sc_sl1cer::CancelToken::new(),
        |_| {},
        allow_tolerant,
    )
    .map_err(|e| SupportError::Slice(format!("{e:?}")))?;
    let sc_sl1cer::SliceOutcome::Completed(leaves) = outcome else {
        unreachable!("fresh token is never cancelled");
    };
    let stack =
        sc_sl1cer::contour::merge_stacks(leaves.into_iter().map(|l| l.into_stack()).collect())
            .map_err(|e| SupportError::Slice(format!("{e:?}")))?;
    let analysis = sc_print_job::analyze_islands(&stack, profile);
    Ok((stack, analysis))
}

/// The finished job, with its island report taken at the parts' real
/// positions (the aligned check is only for placing contacts).
fn report(
    scene: sc_scene::Scene,
    structure: SupportStructure,
    aligned: sc_print_job::LayerAnalysis,
    shifts: &Shifts,
    fills: &std::collections::BTreeMap<sc_scene::NodeId, (sc_scene::NodeId, [f64; 3])>,
    profile: &sc_sl1cer::profile::SliceProfile,
    allow_tolerant: bool,
) -> Result<
    (
        sc_scene::Scene,
        SupportStructure,
        sc_print_job::LayerAnalysis,
    ),
    SupportError,
> {
    let analysis = if fills.is_empty() && shifts.values().all(|d| d[0] == 0.0 && d[1] == 0.0) {
        aligned
    } else {
        analyze(&scene, profile, allow_tolerant)?.1
    };
    Ok((scene, structure, analysis))
}

fn triangles(mesh: &IndexedMesh) -> Result<Vec<[Vertex; 3]>, SupportError> {
    if mesh
        .vertices
        .iter()
        .any(|v| v.iter().any(|c| !c.is_finite()))
        || mesh.validate_faces().is_err()
    {
        return Err(SupportError::InvalidMesh);
    }
    Ok(mesh
        .faces
        .iter()
        .map(|f| f.vertices.map(|i| mesh.vertices[i.get() as usize]))
        .collect())
}

// Vertical ray intersection, including shared triangle edges. Degenerate XY
// projections (vertical faces) cannot obstruct a vertical column's centreline.
fn height_at(t: &[Vertex; 3], x: f64, y: f64) -> Option<f64> {
    let [a, b, c] = t.map(|v| v.to_f64().coords());
    let denominator = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if denominator.abs() < 1e-12 {
        return None;
    }
    let u = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / denominator;
    let v = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / denominator;
    let w = 1.0 - u - v;
    (u >= -1e-8 && v >= -1e-8 && w >= -1e-8).then_some(u * a[2] + v * b[2] + w * c[2])
}

// A vertical-query BVH: exact triangle tests, bounded storage, no duplicated
// triangle references for large faces. Branch clearance visits nearby surfaces.
struct HeightTree {
    lo: [f64; 2],
    hi: [f64; 2],
    children: Option<(Box<Self>, Box<Self>)>,
    indices: Vec<usize>,
}

impl HeightTree {
    fn new(tris: &[[Vertex; 3]]) -> Self {
        Self::build(tris, (0..tris.len()).collect())
    }
    fn build(tris: &[[Vertex; 3]], mut indices: Vec<usize>) -> Self {
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for &i in &indices {
            for v in &tris[i] {
                for axis in 0..2 {
                    lo[axis] = lo[axis].min(f64::from(v[axis]));
                    hi[axis] = hi[axis].max(f64::from(v[axis]));
                }
            }
        }
        let children = if indices.len() > 8 {
            let axis = usize::from(hi[1] - lo[1] > hi[0] - lo[0]);
            let centre = |i: usize| tris[i].iter().map(|v| v[axis]).sum::<f32>();
            let middle = indices.len() / 2;
            indices.select_nth_unstable_by(middle, |&a, &b| {
                centre(a).total_cmp(&centre(b)).then(a.cmp(&b))
            });
            let other = indices.split_off(middle);
            Some((
                Box::new(Self::build(tris, std::mem::take(&mut indices))),
                Box::new(Self::build(tris, other)),
            ))
        } else {
            None
        };
        Self {
            lo,
            hi,
            children,
            indices,
        }
    }
    fn any(&self, tris: &[[Vertex; 3]], x: f64, y: f64, predicate: &impl Fn(f64) -> bool) -> bool {
        let padding = 2e-8 * (self.hi[0] - self.lo[0]).max(self.hi[1] - self.lo[1]) + 1e-7;
        if x < self.lo[0] - padding
            || x > self.hi[0] + padding
            || y < self.lo[1] - padding
            || y > self.hi[1] + padding
        {
            return false;
        }
        if let Some((a, b)) = &self.children {
            a.any(tris, x, y, predicate) || b.any(tris, x, y, predicate)
        } else {
            self.indices
                .iter()
                .filter_map(|&i| height_at(&tris[i], x, y))
                .any(predicate)
        }
    }
    /// Every triangle a vertical line at (x, y) crosses, with the height.
    /// Replaces scanning all triangles per support column (Sleptic: auto
    /// supports were "often very slow"; a Benchy scanned ~225k triangles
    /// for each of hundreds of columns, on every completion pass).
    fn visit(&self, tris: &[[Vertex; 3]], x: f64, y: f64, f: &mut impl FnMut(usize, f64)) {
        let padding = 2e-8 * (self.hi[0] - self.lo[0]).max(self.hi[1] - self.lo[1]) + 1e-7;
        if x < self.lo[0] - padding
            || x > self.hi[0] + padding
            || y < self.lo[1] - padding
            || y > self.hi[1] + padding
        {
            return;
        }
        if let Some((a, b)) = &self.children {
            a.visit(tris, x, y, f);
            b.visit(tris, x, y, f);
        } else {
            for &i in &self.indices {
                if let Some(h) = height_at(&tris[i], x, y) {
                    f(i, h);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "tests_support_height.rs"]
mod tests_height;

/// Sample downward faces on a world-aligned XY grid and include local minimum
/// vertices. Grid placement does not depend on triangle size or stored normals.
/// Each connected, equal-height minimum plateau receives one mandatory contact,
/// even when a different minimum is closer than the configured grid spacing.
pub fn sample_contacts(
    mesh: &IndexedMesh,
    settings: &SupportSettings,
) -> Result<Vec<SupportPoint>, SupportError> {
    settings.validate().map_err(SupportError::Settings)?;
    if crate::orient::prints_without_supports(mesh, settings.overhang_angle_deg) {
        return Ok(Vec::new());
    }
    sample_contacts_inner(mesh, settings, None, None)
}

/// [`sample_contacts`] for a model whose surface tree the caller already
/// has: `access` is that tree in the model's own coordinates with the
/// rotation and shift that place it as `mesh` is placed. Auto orient scores
/// dozens of poses of one model; building the tree again for each took most
/// of its time (R86).
pub(crate) fn sample_contacts_placed(
    mesh: &IndexedMesh,
    settings: &SupportSettings,
    access: (&crate::hollow::Tree, glam::DQuat, glam::DVec3),
    topology: Option<&ContactTopology>,
) -> Result<Vec<SupportPoint>, SupportError> {
    settings.validate().map_err(SupportError::Settings)?;
    if crate::orient::prints_without_supports(mesh, settings.overhang_angle_deg) {
        return Ok(Vec::new());
    }
    sample_contacts_inner(mesh, settings, Some(access), topology)
}

/// Connectivity is unchanged by a finalist's placement. Preserve face/edge
/// insertion order, including duplicate neighbours on non-manifold meshes.
/// Only the geometry-dependent predicates are repeated for each pose.
pub(crate) struct ContactTopology {
    neighbours: Vec<Vec<usize>>,
    edges: std::collections::HashMap<(u32, u32), Vec<usize>>,
}
impl ContactTopology {
    pub(crate) fn new(mesh: &IndexedMesh) -> Self {
        Self {
            neighbours: vertex_neighbours(mesh),
            edges: face_edges(mesh),
        }
    }
}

fn vertex_neighbours(mesh: &IndexedMesh) -> Vec<Vec<usize>> {
    let mut neighbours = vec![Vec::new(); mesh.vertices.len()];
    for face in &mesh.faces {
        let [a, b, c] = face.vertices.map(|i| i.get() as usize);
        for (u, v) in [(a, b), (b, c), (c, a)] {
            neighbours[u].push(v);
            neighbours[v].push(u);
        }
    }
    neighbours
}

fn face_edges(mesh: &IndexedMesh) -> std::collections::HashMap<(u32, u32), Vec<usize>> {
    let mut edges: std::collections::HashMap<_, Vec<usize>> = Default::default();
    for (f, face) in mesh.faces.iter().enumerate() {
        let [a, b, c] = face.vertices.map(|i| i.get());
        for (u, v) in [(a, b), (b, c), (c, a)] {
            edges.entry((u.min(v), u.max(v))).or_default().push(f);
        }
    }
    edges
}

/// Overhang-only sampling with connectivity shared by orientation finalists.
pub(crate) fn sample_overhang_contacts_indexed(
    mesh: &IndexedMesh,
    settings: &SupportSettings,
    topology: &ContactTopology,
) -> Result<Vec<SupportPoint>, SupportError> {
    sample_contacts_inner(mesh, settings, None, Some(topology))
}

fn sample_contacts_inner(
    mesh: &IndexedMesh,
    settings: &SupportSettings,
    access: Option<(&crate::hollow::Tree, glam::DQuat, glam::DVec3)>,
    topology: Option<&ContactTopology>,
) -> Result<Vec<SupportPoint>, SupportError> {
    settings.validate().map_err(SupportError::Settings)?;
    let tris = triangles(mesh)?;
    let mut points = Vec::new();
    let spacing = settings.spacing_mm;
    // A face counts as an overhang only clearly past the overhang angle: a
    // face right at it (a part turned exactly 45 degrees) otherwise flipped
    // either way on float rounding, which differs between copies of the same
    // part placed apart, so identical copies got different supports.
    let threshold = settings.overhang_angle_deg.to_radians().cos() + 1e-5;
    let mut remaining_candidates = 1_000_000.0;
    let mut minima = vec![true; mesh.vertices.len()];
    let mut neighbours = vec![Vec::new(); mesh.vertices.len()];
    let mut minimum_normals = vec![None; mesh.vertices.len()];
    for face in &mesh.faces {
        let t = face.vertices.map(|i| mesh.vertices[i.get() as usize]);
        let normal = compute_normal_from_triangle_vertices(&t).ok();
        for index in face.vertices {
            let i = index.get() as usize;
            for j in face.vertices {
                let j = j.get() as usize;
                if i != j && (mesh.vertices[i][2] - mesh.vertices[j][2]).abs() <= 1e-5 {
                    neighbours[i].push(j);
                }
            }
            if let Some(n) = normal
                && n[2] < 0.0
                && minimum_normals[i].is_none_or(|old: sc_mesh_core::Normal| n[2] < old[2])
            {
                minimum_normals[i] = Some(n);
            }
            if face
                .vertices
                .iter()
                .any(|j| mesh.vertices[j.get() as usize][2] < mesh.vertices[i][2] - 1e-5)
            {
                minima[i] = false;
            }
        }
    }
    let persistence = match topology {
        Some(t) => minimum_persistence_indexed(mesh, &t.neighbours),
        None => minimum_persistence(mesh),
    };
    // Lowest points first so the grid cannot replace a true bottom contact.
    let mut visited = vec![false; mesh.vertices.len()];
    let mut candidates = Vec::new();
    for seed in 0..mesh.vertices.len() {
        if visited[seed] {
            continue;
        }
        let mut pending = vec![seed];
        visited[seed] = true;
        let mut minimum = true;
        let mut representative = None;
        let mut lasts = 0.0f64;
        while let Some(i) = pending.pop() {
            minimum &= minima[i];
            lasts = lasts.max(persistence[i]);
            if minimum_normals[i].is_some() && representative.is_none_or(|old| i < old) {
                representative = Some(i);
            }
            for &j in &neighbours[i] {
                if !visited[j] {
                    visited[j] = true;
                    pending.push(j);
                }
            }
        }
        // A dip that rejoins lower material within MIN_ISLAND_HEIGHT_MM
        // never prints as an island (tessellation noise, a shallow bump):
        // no pin for it. Real islands the raster sees are still completed.
        if minimum
            && lasts >= MIN_ISLAND_HEIGHT_MM
            && let Some(i) = representative
        {
            candidates.push((i, &mesh.vertices[i]));
        }
    }
    candidates.sort_by(|(_, a), (_, b)| {
        a[2].total_cmp(&b[2])
            .then(a[0].total_cmp(&b[0]))
            .then(a[1].total_cmp(&b[1]))
    });
    for (i, v) in candidates {
        if let Some(normal) = minimum_normals[i] {
            points.push(SupportPoint {
                position: v.coords(),
                normal: normal.coords(),
            });
        }
    }
    // Contacts before this are island bottoms: a part needs them wherever
    // they are. The overhang grid after it only improves a surface.
    let mandatory = points.len();
    let narrow = narrow_overhang_faces(mesh, threshold);
    // The grid is anchored to the model's own centre, not the plate: a
    // plate-aligned grid gave every copy of the same part a different
    // pattern, depending on where it sat (Sleptic: six identical arranged
    // parts got different supports).
    let (grid_x, grid_y) = model_grid_origin(mesh);
    // Hexagonal grid with the square grid's worst gap: no overhang point is
    // farther than spacing / sqrt(2) from a contact, as on a square grid of
    // `spacing`, with about 23% fewer contacts (Sleptic: properly support with
    // the fewest contacts touching the part).
    let point_pitch = spacing * 1.5f64.sqrt();
    let row_pitch = point_pitch * 3f64.sqrt() / 2.0;
    // A wall the overhang hangs from holds it along its whole edge, like a
    // line of contacts. A grid point this close to such an edge adds
    // nothing: the gap between the wall and the next row stays within the
    // grid's own worst gap without it.
    let wall_reach = 2.0 * spacing / 2f64.sqrt() - row_pitch;
    let walls = WallEdges::new(mesh, threshold, wall_reach, topology);
    for (face, t) in tris.iter().enumerate() {
        let Ok(normal) = compute_normal_from_triangle_vertices(t) else {
            continue;
        };
        if f64::from(-normal[2]) < threshold || narrow[face] {
            continue;
        }
        let min_x = t
            .iter()
            .map(|v| f64::from(v[0]))
            .fold(f64::INFINITY, f64::min);
        let max_x = t
            .iter()
            .map(|v| f64::from(v[0]))
            .fold(f64::NEG_INFINITY, f64::max);
        let min_y = t
            .iter()
            .map(|v| f64::from(v[1]))
            .fold(f64::INFINITY, f64::min);
        let max_y = t
            .iter()
            .map(|v| f64::from(v[1]))
            .fold(f64::NEG_INFINITY, f64::max);
        let y0 = ((min_y - grid_y) / row_pitch).ceil();
        let y1 = ((max_y - grid_y) / row_pitch).floor();
        let ny = (y1 - y0 + 1.0).max(0.0);
        let nx = ((max_x - min_x) / point_pitch).floor() + 1.0;
        if ny == 0.0 {
            continue;
        }
        if nx * ny > remaining_candidates {
            return Err(SupportError::SamplingLimit);
        }
        remaining_candidates -= nx * ny;
        for iy in 0..ny as i64 {
            let row = y0 as i64 + iy;
            let y = grid_y + row as f64 * row_pitch;
            // Odd rows sit half a pitch over (a hexagonal pattern).
            let offset = if row.rem_euclid(2) == 1 { 0.5 } else { 0.0 };
            let x0 = ((min_x - grid_x) / point_pitch - offset).ceil() as i64;
            let x1 = ((max_x - grid_x) / point_pitch - offset).floor() as i64;
            for ix in x0..=x1 {
                let x = grid_x + (ix as f64 + offset) * point_pitch;
                if let Some(z) = height_at(t, x, y)
                    && !walls.near([x, y, z])
                {
                    add_spaced(
                        &mut points,
                        SupportPoint {
                            position: [x as f32, y as f32, z as f32],
                            normal: normal.coords(),
                        },
                        spacing,
                    );
                }
            }
        }
    }
    // Sleptic: auto supports sometimes land where nobody could remove them.
    // An overhang-grid contact with the part all round it (no tool can
    // approach sideways, the same test auto orient scores access with) is
    // left out; the surface there prints unsupported rather than with a
    // support that cannot come off. Island bottoms always stay, and the
    // raster check still supports any island the slicer finds.
    if points.len() > mandatory {
        let own;
        let (tree, rotation, shift) = match access {
            Some(access) => access,
            None => {
                own = crate::hollow::Tree::from_mesh(mesh);
                (&own, glam::DQuat::IDENTITY, glam::DVec3::ZERO)
            }
        };
        let inverse = rotation.inverse();
        let mut index = 0;
        points.retain(|p| {
            index += 1;
            index <= mandatory || !unreachable(tree, p, inverse, shift)
        });
    }
    if settings.symmetric_supports {
        mirror_contacts(mesh, &mut points);
    }
    // A bed-level minimum is attached to the plate, including on mixed plates.
    points.retain(|p| p.position[2] > 0.02);
    Ok(points)
}

/// Edges where an overhang face meets a face going down from it (a wall
/// holding the overhang up), bucketed by `reach`-sized cells for
/// [`WallEdges::near`]. An edge whose neighbour rises from it is a free rim
/// and is not a wall.
struct WallEdges {
    reach: f64,
    cells: std::collections::HashMap<(i64, i64, i64), Vec<(glam::DVec3, glam::DVec3)>>,
}

impl WallEdges {
    fn new(
        mesh: &IndexedMesh,
        threshold: f64,
        reach: f64,
        topology: Option<&ContactTopology>,
    ) -> Self {
        use glam::DVec3;
        let mut cells: std::collections::HashMap<_, Vec<_>> = std::collections::HashMap::new();
        let mut walls = Self {
            reach,
            cells: Default::default(),
        };
        if reach <= 0.0 {
            return walls;
        }
        let vertex = |i: u32| DVec3::from_array(mesh.vertices[i as usize].coords().map(f64::from));
        let overhang = |f: usize| {
            let t = mesh.faces[f]
                .vertices
                .map(|i| mesh.vertices[i.get() as usize]);
            compute_normal_from_triangle_vertices(&t).is_ok_and(|n| f64::from(-n[2]) >= threshold)
        };
        let own;
        let edges = match topology {
            Some(t) => &t.edges,
            None => {
                own = face_edges(mesh);
                &own
            }
        };
        let cell = |p: DVec3| {
            (
                (p.x / reach).floor() as i64,
                (p.y / reach).floor() as i64,
                (p.z / reach).floor() as i64,
            )
        };
        for (&(u, v), faces) in edges {
            let &[f, g] = faces.as_slice() else {
                continue;
            };
            let other = match (overhang(f), overhang(g)) {
                (true, false) => g,
                (false, true) => f,
                _ => continue,
            };
            let (a, b) = (vertex(u), vertex(v));
            let Some(&third) = mesh.faces[other]
                .vertices
                .iter()
                .find(|i| i.get() != u && i.get() != v)
            else {
                continue;
            };
            if vertex(third.get()).z >= a.z.min(b.z) - 1e-4 {
                continue;
            }
            // Every cell the segment passes within reach of.
            let (lo, hi) = (
                cell(a.min(b) - DVec3::splat(reach)),
                cell(a.max(b) + DVec3::splat(reach)),
            );
            if (hi.0 - lo.0 + 1) * (hi.1 - lo.1 + 1) * (hi.2 - lo.2 + 1) > 4096 {
                continue;
            }
            for i in lo.0..=hi.0 {
                for j in lo.1..=hi.1 {
                    for k in lo.2..=hi.2 {
                        cells.entry((i, j, k)).or_default().push((a, b));
                    }
                }
            }
        }
        walls.cells = cells;
        walls
    }

    /// Whether `p` is within reach of a wall edge.
    fn near(&self, p: [f64; 3]) -> bool {
        let p = glam::DVec3::from_array(p);
        let key = (
            (p.x / self.reach).floor() as i64,
            (p.y / self.reach).floor() as i64,
            (p.z / self.reach).floor() as i64,
        );
        self.cells.get(&key).is_some_and(|segments| {
            segments.iter().any(|&(a, b)| {
                let d = b - a;
                let t = ((p - a).dot(d) / d.length_squared().max(1e-18)).clamp(0.0, 1.0);
                (a + d * t).distance(p) <= self.reach
            })
        })
    }
}

/// Origin of a model's overhang sampling grid: its XY bounding-box centre,
/// offset half a step so grid lines do not run along a symmetric part's
/// centre edges. Moving a part moves its grid with it.
fn model_grid_origin(mesh: &IndexedMesh) -> (f64, f64) {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for v in &mesh.vertices {
        for axis in 0..2 {
            lo[axis] = lo[axis].min(f64::from(v[axis]));
            hi[axis] = hi[axis].max(f64::from(v[axis]));
        }
    }
    if !lo[0].is_finite() {
        return (0.0, 0.0);
    }
    (0.5 * (lo[0] + hi[0]), 0.5 * (lo[1] + hi[1]))
}

/// Whether a support at `point` could not be reached to remove it: just
/// under the contact, the part blocks every sideways approach within 8 mm
/// (the access test of [`orientation_access_indexed`]).
fn unreachable(
    tree: &crate::hollow::Tree,
    point: &SupportPoint,
    inverse: glam::DQuat,
    shift: glam::DVec3,
) -> bool {
    use glam::DVec3;
    // The tree is in the model's own coordinates: the placed point and the
    // four sideways directions are taken back there.
    let approach =
        inverse * (DVec3::from_array(point.position.map(f64::from)) - DVec3::Z * 0.25 - shift);
    [DVec3::X, -DVec3::X, DVec3::Y, -DVec3::Y]
        .into_iter()
        .all(|direction| tree.ray_any(approach, inverse * direction, 0.05, 8.0))
}

/// A local minimum whose patch rejoins lower material within this height
/// (mm) is not an island worth a support (Sleptic: auto supports
/// "overcompensate on super small overhangs").
const MIN_ISLAND_HEIGHT_MM: f64 = 0.1;

/// Overhang regions narrower than this (mm) print fine without supports:
/// a small lip or chamfer bridges from the wall it hangs off.
const MIN_OVERHANG_WIDTH_MM: f64 = 1.0;

/// How long (mm of height) each vertex's local-minimum patch stays a
/// separate island: a bottom-up sweep joins each vertex to its lower
/// neighbours; when two patches meet, the younger one (higher birth)
/// ends there. `INFINITY` for each shell's lowest patch, 0 for vertices
/// that start no patch.
fn minimum_persistence(mesh: &IndexedMesh) -> Vec<f64> {
    minimum_persistence_indexed(mesh, &vertex_neighbours(mesh))
}

fn minimum_persistence_indexed(mesh: &IndexedMesh, neighbours: &[Vec<usize>]) -> Vec<f64> {
    let n = mesh.vertices.len();
    let z = |i: usize| f64::from(mesh.vertices[i][2]);
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| z(a).total_cmp(&z(b)).then(a.cmp(&b)));
    let mut rank = vec![0; n];
    for (r, &i) in order.iter().enumerate() {
        rank[i] = r;
    }
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut persistence = vec![0.0; n];
    let mut alive = vec![false; n];
    for &v in &order {
        let mut root: Option<usize> = None;
        for &u in &neighbours[v] {
            if rank[u] > rank[v] {
                continue;
            }
            let ru = find(&mut parent, u);
            match root {
                None => root = Some(ru),
                Some(r) if r != ru => {
                    // Roots are birth vertices: the older (lower) survives.
                    let (old, young) = if rank[r] < rank[ru] { (r, ru) } else { (ru, r) };
                    persistence[young] = z(v) - z(young);
                    alive[young] = false;
                    parent[young] = old;
                    root = Some(old);
                }
                _ => {}
            }
        }
        match root {
            Some(r) => parent[v] = r,
            None => alive[v] = true,
        }
    }
    for i in 0..n {
        if alive[i] {
            persistence[i] = f64::INFINITY;
        }
    }
    persistence
}

/// Overhang faces (facing down past the overhang angle) in connected
/// regions narrower than `MIN_OVERHANG_WIDTH_MM`, measured as twice the
/// region's plan area over its outline length (a strip's width).
fn narrow_overhang_faces(mesh: &IndexedMesh, threshold: f64) -> Vec<bool> {
    use std::collections::HashMap;
    let n = mesh.faces.len();
    let corners = |f: usize| mesh.faces[f].vertices.map(|i| i.get());
    let over: Vec<bool> = (0..n)
        .map(|f| {
            let t = corners(f).map(|i| mesh.vertices[i as usize]);
            compute_normal_from_triangle_vertices(&t).is_ok_and(|nm| f64::from(-nm[2]) >= threshold)
        })
        .collect();
    let mut edges: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for f in (0..n).filter(|&f| over[f]) {
        let [a, b, c] = corners(f);
        for (u, v) in [(a, b), (b, c), (c, a)] {
            edges.entry((u.min(v), u.max(v))).or_default().push(f);
        }
    }
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for faces in edges.values() {
        for pair in faces.windows(2) {
            let (a, b) = (find(&mut parent, pair[0]), find(&mut parent, pair[1]));
            if a != b {
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let xy = |i: u32| {
        let v = mesh.vertices[i as usize];
        [f64::from(v[0]), f64::from(v[1])]
    };
    let mut area: HashMap<usize, f64> = HashMap::new();
    let mut outline: HashMap<usize, f64> = HashMap::new();
    for f in (0..n).filter(|&f| over[f]) {
        let [a, b, c] = corners(f).map(xy);
        let root = find(&mut parent, f);
        *area.entry(root).or_default() +=
            ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0;
    }
    for (&(u, v), faces) in &edges {
        if faces.len() == 1 {
            let (a, b) = (xy(u), xy(v));
            let root = find(&mut parent, faces[0]);
            *outline.entry(root).or_default() += (b[0] - a[0]).hypot(b[1] - a[1]);
        }
    }
    (0..n)
        .map(|f| {
            over[f] && {
                let root = find(&mut parent, f);
                let p = outline.get(&root).copied().unwrap_or(0.0);
                p > 0.0 && 2.0 * area.get(&root).copied().unwrap_or(0.0) / p < MIN_OVERHANG_WIDTH_MM
            }
        })
        .collect()
}

/// Detect a vertical mirror plane through the surface-area centre. Candidates
/// include XY axes, principal axes and horizontal face normals. Reflected
/// vertices, edge midpoints and face centres must lie within 0.01 mm of the
/// actual surface, using the existing distance BVH (independent of tessellation).
/// Returns the horizontal unit normal and plane offset, `dot(normal, xy) = offset`.
pub fn symmetry_plane(mesh: &IndexedMesh) -> Option<([f32; 2], f32)> {
    use glam::DVec3;
    let tris = triangles(mesh).ok()?;
    if tris.is_empty() {
        return None;
    }
    let tree = crate::hollow::Tree::from_mesh(mesh);
    let mut centre = DVec3::ZERO;
    let mut total = 0.0;
    let mut candidates = vec![[1.0_f64, 0.0], [0.0, 1.0]];
    for t in &tris {
        let [a, b, c] = t.map(|v| DVec3::from_array(v.coords().map(f64::from)));
        let n = (b - a).cross(c - a);
        let area = n.length();
        centre += (a + b + c) / 3.0 * area;
        total += area;
        let horizontal = n.truncate();
        if horizontal.length_squared() > 1e-12 && candidates.len() < 66 {
            let n = horizontal.normalize();
            let v = [n.x, n.y];
            if candidates
                .iter()
                .all(|p| (p[0] * v[1] - p[1] * v[0]).abs() > 1e-4)
            {
                candidates.push(v);
            }
        }
    }
    if total <= 0.0 {
        return None;
    }
    centre /= total;
    let mut moments = [0.0; 3];
    for v in &mesh.vertices {
        let x = f64::from(v[0]) - centre.x;
        let y = f64::from(v[1]) - centre.y;
        moments[0] += x * x;
        moments[1] += x * y;
        moments[2] += y * y;
    }
    let angle = 0.5 * (2.0 * moments[1]).atan2(moments[0] - moments[2]);
    candidates.extend([[angle.cos(), angle.sin()], [-angle.sin(), angle.cos()]]);
    for n in candidates {
        let normal = DVec3::new(n[0], n[1], 0.0);
        let offset = normal.dot(centre);
        let matches = tris.iter().all(|t| {
            let [a, b, c] = t.map(|v| DVec3::from_array(v.coords().map(f64::from)));
            [
                a,
                b,
                c,
                (a + b) * 0.5,
                (a + c) * 0.5,
                (b + c) * 0.5,
                (a + b + c) / 3.0,
            ]
            .into_iter()
            .all(|p| tree.near(p - normal * (2.0 * (normal.dot(p) - offset)), 0.01))
        });
        if matches {
            return Some(([n[0] as f32, n[1] as f32], offset as f32));
        }
    }
    None
}

fn mirror_contacts(mesh: &IndexedMesh, points: &mut Vec<SupportPoint>) {
    if let Some((n, offset)) = symmetry_plane(mesh) {
        for mut p in points.clone() {
            let distance = 2.0 * (n[0] * p.position[0] + n[1] * p.position[1] - offset);
            let direction = 2.0 * (n[0] * p.normal[0] + n[1] * p.normal[1]);
            for (axis, component) in n.iter().enumerate() {
                p.position[axis] -= distance * component;
                p.normal[axis] -= direction * component;
            }
            add_spaced(points, p, 0.01);
        }
    }
}

// Detect each model independently, so a pair of asymmetric models cannot
// accidentally qualify as one symmetric assembly.
fn mirror_scene_contacts(
    scene: &sc_scene::Scene,
    points: &mut Vec<SupportPoint>,
) -> Result<(), SupportError> {
    let owners = support_owners(scene, points);
    let distinct: std::collections::BTreeSet<_> = owners.iter().flatten().copied().collect();
    for id in distinct {
        let mesh = sc_scene::bake_leaf_to_world(scene, id).map_err(SupportError::Scene)?;
        let mut local: Vec<_> = points
            .iter()
            .zip(&owners)
            .filter_map(|(p, owner)| (*owner == Some(id)).then_some(*p))
            .collect();
        mirror_contacts(&mesh, &mut local);
        for point in local {
            add_spaced(points, point, 0.01);
        }
    }
    Ok(())
}

/// Shaft diameter for a contact carrying the given island area. The reference
/// area is one spacing cell; growth follows radius squared, with explicit bounds.
pub fn column_diameter(settings: &SupportSettings, area_mm2: f64) -> f64 {
    (settings.shaft_diameter_mm * (area_mm2 / settings.spacing_mm.powi(2)).sqrt()).clamp(
        settings.min_column_diameter_mm,
        settings.max_column_diameter_mm,
    )
}

fn add_spaced(points: &mut Vec<SupportPoint>, point: SupportPoint, spacing: f64) {
    if points.iter().all(|p| {
        let dx = f64::from(p.position[0] - point.position[0]);
        let dy = f64::from(p.position[1] - point.position[1]);
        let dz = f64::from(p.position[2] - point.position[2]);
        dx * dx + dy * dy + dz * dz >= spacing * spacing * 0.999
    }) {
        points.push(point);
    }
}

/// Build ten-sided, closed support columns. Contacts obstructed below are skipped.
/// The tip penetrates the surface by 0.02 mm to avoid a coplanar slicing seam.
/// Input mesh and contacts must already include the process lift.
pub fn build_supports(
    mesh: &IndexedMesh,
    contacts: &[SupportPoint],
    settings: &SupportSettings,
) -> Result<SupportStructure, SupportError> {
    build_supports_with_tolerance(mesh, None, contacts, settings, 1e-4)
}

/// `mesh` is everything columns must not drill through (every model on the
/// plate); `own`, when given, is the supported model alone, whose downward
/// faces set each column's load.
pub(crate) fn build_supports_with_tolerance(
    mesh: &IndexedMesh,
    own: Option<&IndexedMesh>,
    contacts: &[SupportPoint],
    settings: &SupportSettings,
    surface_tolerance: f64,
) -> Result<SupportStructure, SupportError> {
    build_supports_on_plate(mesh, own, contacts, settings, surface_tolerance, None)
}

/// The search structures support building reads from a mesh: its
/// triangles, the height tree under them and the solid for clearance checks.
/// Built once per support pass and reused by every island pass, since the
/// models do not move between passes (R92: rebuilding them for a 225k
/// triangle Benchy was most of each pass's build).
pub(crate) struct ModelIndex {
    tris: Vec<[Vertex; 3]>,
    heights: HeightTree,
    solid: crate::hollow::Tree,
}

impl ModelIndex {
    pub(crate) fn new(mesh: &IndexedMesh) -> Result<Self, SupportError> {
        let tris = triangles(mesh)?;
        let heights = HeightTree::new(&tris);
        Ok(Self {
            tris,
            heights,
            solid: crate::hollow::Tree::from_mesh(mesh),
        })
    }
}

/// Indexes kept for one support pass: every model on the plate together, and
/// each supported model alone (for its contacts' loads).
#[derive(Default)]
pub(crate) struct IndexCache {
    plate: std::cell::OnceCell<std::rc::Rc<ModelIndex>>,
    own: std::cell::RefCell<std::collections::BTreeMap<sc_scene::NodeId, std::rc::Rc<ModelIndex>>>,
}

/// [`build_supports_with_tolerance`] on a plate of half-size `plate`
/// (scene XY is plate-centred): leaning columns keep their feet on it.
pub(crate) fn build_supports_on_plate(
    mesh: &IndexedMesh,
    own: Option<&IndexedMesh>,
    contacts: &[SupportPoint],
    settings: &SupportSettings,
    surface_tolerance: f64,
    plate: Option<[f64; 2]>,
) -> Result<SupportStructure, SupportError> {
    settings.validate().map_err(SupportError::Settings)?;
    let index = ModelIndex::new(mesh)?;
    let own = own.map(ModelIndex::new).transpose()?;
    build_supports_indexed(
        &index,
        own.as_ref(),
        contacts,
        settings,
        surface_tolerance,
        plate,
    )
}

/// [`build_supports_on_plate`] from indexes already built.
pub(crate) fn build_supports_indexed(
    index: &ModelIndex,
    own: Option<&ModelIndex>,
    contacts: &[SupportPoint],
    settings: &SupportSettings,
    surface_tolerance: f64,
    plate: Option<[f64; 2]>,
) -> Result<SupportStructure, SupportError> {
    settings.validate().map_err(SupportError::Settings)?;
    let (tris, heights, solid) = (&index.tris, &index.heights, &index.solid);
    let floor = if settings.raft {
        settings.raft_thickness_mm
    } else {
        0.0
    };
    let mut result = SupportStructure::default();
    // Loads come from the supported model's own faces: scanning every model
    // on a filled plate each pass cost more than the part's own supports.
    let loads = match own {
        Some(own) => contact_island_loads(&own.tris, &own.heights, contacts, false),
        None => contact_island_loads(tris, heights, contacts, false),
    };
    // Per built column: XY, stands on the plate, shaft bottom / top, diameter.
    let mut shafts: Vec<([f64; 2], bool, f64, f64, f64)> = Vec::new();
    // Contacts with a clear way straight down: built as standard pillars and
    // trees once all are known (R86).
    let mut free = Vec::new();
    for (index, (point, (_, area))) in contacts.iter().zip(&loads).enumerate() {
        let mut diameter = column_diameter(settings, *area);
        let [x, y, z] = point.position.map(f64::from);
        if point
            .position
            .iter()
            .chain(point.normal.iter())
            .any(|c| !c.is_finite())
            || point.normal.iter().map(|v| v * v).sum::<f32>() < 1e-12
            || z <= floor + settings.lowest_contact_mm()
        {
            return Err(SupportError::InvalidContact);
        }
        if let Some(head) = standard::head(
            &solid,
            &heights,
            &tris,
            index,
            point,
            diameter,
            settings.min_column_diameter_mm.min(diameter),
            settings,
            floor,
            plate,
        ) {
            free.push(head);
            continue;
        }
        let mut lower_surface: Option<f64> = None;
        heights.visit(&tris, x, y, &mut |_, h| {
            if h > floor + 1e-5 && h < z - surface_tolerance {
                lower_surface = Some(lower_surface.map_or(h, |m: f64| m.max(h)));
            }
        });
        if lower_surface.is_some() && surface_tolerance <= 1e-4 {
            continue;
        }
        // Mandatory raster contacts may land on the highest model surface
        // below the island. Starting there avoids drilling a plate column
        // through an existing solid. Short gaps shorten both transitions.
        let column_floor = lower_surface.map_or(floor, |h| h - 0.02);
        let base_height = settings.base_height_mm.min((z - column_floor) / 3.0);
        let tip_length = tip_length_above(settings, z - column_floor);
        // A column whose shaft runs into a model would be fused into it and
        // could not be removed (Sleptic: a Benchy got columns half inside its
        // bow and cabin wall). Beside a wall the shaft narrows to fit, down
        // to the thinnest column; a geometric contact that still does not
        // fit is dropped. A raster island contact keeps the thinnest column,
        // since an island must be held. The tips, meant to touch, are not
        // checked.
        let shaft_bottom = column_floor
            + if lower_surface.is_some() {
                tip_length
            } else {
                base_height
            };
        let fits = |d: f64| {
            let top = z - tip_length - d;
            top <= shaft_bottom
                || bar_clear(
                    &solid,
                    [x, y, shaft_bottom],
                    [x, y, top],
                    d / 2.0 + COLUMN_GAP_MM,
                )
        };
        let wanted = diameter;
        let thinnest = settings.min_column_diameter_mm.min(diameter);
        while !fits(diameter) && diameter > thinnest + 1e-9 {
            diameter = (diameter * 0.8).max(thinnest);
        }
        let straight = fits(diameter);
        // A column that would stand on the part (two contact marks) or does
        // not fit straight down leans away from the part instead, as far as
        // the branch angle allows, and drops to the plate where it can: one
        // contact, clear of the part (Sleptic, R83). Standing on the part, it
        // keeps clear of it too.
        if (lower_surface.is_some() || !straight)
            && let Some(lean) = lean_route(
                &solid,
                &heights,
                &tris,
                [x, y, z],
                &[wanted, thinnest],
                settings,
                floor,
                plate,
                // A straight column on the part is kept over a leaning one
                // that would stand on the part all the same.
                straight,
            )
        {
            result
                .columns
                .push(lean_column(&lean, [x, y, z], &solid, settings, floor)?);
            result.points.push(*point);
            // The leaning part, from the contact to the knee, is a member
            // like a branch: kept within MAX_LEAN_DEG (R91).
            let (dx, dy, dz) = (lean.knee[0] - x, lean.knee[1] - y, lean.knee[2] - z);
            result.paths.push(BranchPath {
                length_mm: (dx * dx + dy * dy + dz * dz).sqrt(),
                rise_mm: dz.abs(),
                inclination_deg: dx.hypot(dy).atan2(dz.abs()).to_degrees(),
            });
            shafts.push((
                [lean.knee[0], lean.knee[1]],
                lean.lower.is_none(),
                lean.lower
                    .map_or(floor + settings.base_height_mm, |h| h + lean.tip_length),
                lean.knee[2],
                lean.diameter,
            ));
            continue;
        }
        if surface_tolerance <= 1e-4 && !straight {
            continue;
        }
        // Its base pad narrows the same way beside a part on the plate.
        let mut base_radius = settings.base_diameter_mm / 2.0;
        while lower_surface.is_none()
            && base_height > 0.02
            && base_radius > diameter / 2.0 + 1e-9
            && !bar_clear(
                &solid,
                [x, y, column_floor + 0.01],
                [x, y, column_floor + base_height],
                base_radius + COLUMN_GAP_MM,
            )
        {
            base_radius = (base_radius * 0.8).max(diameter / 2.0);
        }
        // A column standing on the model ends in a contact tip there too,
        // as in Chitubox / Lychee: a wide base pad fused to the part left a
        // scar the size of the pad (R58).
        // Each tip ends where a full tip of its length would (R104); a 45
        // degree shoulder widens it back to the shaft.
        let end = tip_end_radius(settings, tip_length, diameter / 2.0);
        let shoulder = diameter / 2.0 - end;
        let mut rings = match lower_surface {
            Some(_) => vec![
                (
                    column_floor + 0.02 - settings.contact_depth_mm,
                    settings.tip_contact_diameter_mm / 2.0,
                ),
                (column_floor + tip_length, end),
                (column_floor + tip_length + shoulder, diameter / 2.0),
            ],
            None => vec![
                (column_floor, base_radius),
                (column_floor + base_height, diameter / 2.0),
            ],
        };
        rings.extend([
            (z - tip_length - shoulder, diameter / 2.0),
            (z - tip_length, end),
            (
                z + settings.contact_depth_mm,
                settings.tip_contact_diameter_mm / 2.0,
            ),
        ]);
        // Short columns have no room for both shoulders: keep the rings in
        // order (a ring at or below the one before it is dropped).
        let mut ordered: Vec<(f64, f64)> = Vec::with_capacity(rings.len());
        for ring in rings {
            if ordered.last().is_none_or(|last| ring.0 > last.0 + 1e-6) {
                ordered.push(ring);
            }
        }
        let rings = ordered;
        let vertices = rings
            .iter()
            .flat_map(|(height, radius)| {
                (0..ROUND_SIDES).map(move |i| {
                    let a = f64::from(i) * std::f64::consts::TAU / f64::from(ROUND_SIDES);
                    Vertex::new([
                        (x + radius * a.cos()) as f32,
                        (y + radius * a.sin()) as f32,
                        *height as f32,
                    ])
                })
            })
            .collect();
        result.columns.push(closed_rings(vertices, ROUND_SIDES)?);
        result.points.push(*point);
        shafts.push((
            [x, y],
            lower_surface.is_none(),
            column_floor + base_height,
            z - tip_length,
            diameter,
        ));
    }
    // One contact per column first, then the other contacts of the trees.
    let mut shared = Vec::new();
    for built in standard::grow(&free, &solid, settings, floor)? {
        result.columns.push(built.mesh);
        result.points.push(contacts[built.contacts[0]]);
        shared.extend(built.contacts[1..].iter().map(|&i| contacts[i]));
        let (xy, lo, hi, diameter) = built.shaft;
        shafts.push((xy, true, lo, hi, diameter));
        result.paths.extend(built.paths);
    }
    result.points.extend(shared);
    // Tall pillars and trunks are braced into one network (R84).
    brace_columns(&mut result, &shafts, &solid, settings, floor, plate)?;
    let feet = raft_feet(&result.columns, floor);
    result.raft_target_mm2 = raft_target_area(&own.unwrap_or(index).tris);
    if settings.raft && !feet.is_empty() {
        result.raft = Some(make_raft(&feet, settings, None, result.raft_target_mm2)?);
    }
    Ok(result)
}

/// Shafts at least this tall (mm) get cross-braced to a neighbour.
const BRACE_MIN_SHAFT_MM: f64 = 10.0;
/// Neighbours farther apart than this (mm) are not braced together.
const BRACE_MAX_GAP_MM: f64 = 10.0;
/// A plate-standing shaft at least this tall (mm) with no neighbour to brace
/// to gets a companion pillar beside it (Lychee's first reinforcement: any
/// support over 20 mm needs bracing).
const LONE_MIN_SHAFT_MM: f64 = 20.0;
/// How far (mm) the companion stands from the shaft it steadies, at least:
/// with wide feet it moves out so the two feet stay [`FOOT_APART_MM`] apart.
const COMPANION_GAP_MM: f64 = 7.0;
/// The least gap (mm) between two feet.
const FOOT_APART_MM: f64 = 2.0;
/// The companion rises this share of the shaft it steadies.
const COMPANION_SHARE: f64 = 0.75;

/// Cross-bracing as in Chitubox / Lychee: each tall plate-standing column is
/// tied to its nearest tall neighbour by thin rods zig-zagging up at about
/// 45 degrees, so slender columns cannot sway or snap under the peel force.
/// Each rod is a closed shell added to the lower-index column's mesh, so the
/// column count (one mesh per contact) is unchanged.
fn brace_columns(
    result: &mut SupportStructure,
    shafts: &[([f64; 2], bool, f64, f64, f64)],
    solid: &crate::hollow::Tree,
    settings: &SupportSettings,
    floor: f64,
    plate: Option<[f64; 2]>,
) -> Result<(), SupportError> {
    let tall: Vec<usize> = (0..shafts.len())
        .filter(|&i| shafts[i].1 && shafts[i].3 - shafts[i].2 >= BRACE_MIN_SHAFT_MM)
        .collect();
    // One network, not loose pairs: the shortest links joining every tall
    // column within reach (a minimum spanning forest over the trunks). It
    // always holds each column's link to its nearest neighbour,
    // the pairs braced before (R84).
    let mut edges = Vec::new();
    for (k, &i) in tall.iter().enumerate() {
        for &j in &tall[k + 1..] {
            let d = (shafts[j].0[0] - shafts[i].0[0]).hypot(shafts[j].0[1] - shafts[i].0[1]);
            if (1.0..=BRACE_MAX_GAP_MM).contains(&d) {
                edges.push((d, i, j));
            }
        }
    }
    edges.sort_by(|a, b| a.0.total_cmp(&b.0).then((a.1, a.2).cmp(&(b.1, b.2))));
    let mut parent: Vec<usize> = (0..shafts.len()).collect();
    let root = |parent: &mut Vec<usize>, mut i: usize| {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    };
    let mut pairs = std::collections::BTreeSet::new();
    for (_, i, j) in edges {
        let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
        if ri != rj {
            parent[ri] = rj;
            pairs.insert((i, j));
        }
    }
    let mut braced = vec![false; shafts.len()];
    for &(a, b) in &pairs {
        braced[a] = true;
        braced[b] = true;
    }
    for &i in &tall {
        if !braced[i] && shafts[i].3 - shafts[i].2 >= LONE_MIN_SHAFT_MM {
            companion(result, i, shafts[i], solid, settings, floor, plate)?;
        }
    }
    for (a, b) in pairs {
        let (pa, _, lo_a, hi_a, da) = shafts[a];
        let (pb, _, lo_b, hi_b, db) = shafts[b];
        let gap = (pb[0] - pa[0]).hypot(pb[1] - pa[1]);
        let lo = lo_a.max(lo_b) + 1.0;
        let hi = hi_a.min(hi_b) - 1.0;
        let radius = (da.min(db) * 0.3).max(0.25);
        let mut shells = Vec::new();
        let mut z = lo;
        let mut from_a = true;
        while z + gap <= hi {
            let (p, q) = if from_a { (pa, pb) } else { (pb, pa) };
            let (from, to) = ([p[0], p[1], z], [q[0], q[1], z + gap]);
            // A rod that would pass close to a part is left out.
            if bar_clear(solid, from, to, radius + COLUMN_GAP_MM) {
                shells.push(rod(from, to, radius)?);
            }
            z += gap;
            from_a = !from_a;
        }
        if !shells.is_empty() {
            let mut meshes = vec![result.columns[a].clone()];
            meshes.extend(shells);
            if let Some(merged) = merge_meshes(meshes.iter()) {
                result.columns[a] = merged;
            }
        }
    }
    Ok(())
}

/// A companion pillar beside lone tall shaft `index`, braced to it with
/// zigzag rods: it touches no part, so it costs no contact mark, and it
/// stops a slender pillar with nothing near it from swaying. Left out when
/// no spot around the shaft is clear.
fn companion(
    result: &mut SupportStructure,
    index: usize,
    shaft: ([f64; 2], bool, f64, f64, f64),
    solid: &crate::hollow::Tree,
    settings: &SupportSettings,
    floor: f64,
    plate: Option<[f64; 2]>,
) -> Result<(), SupportError> {
    let ([x, y], _, lo, hi, diameter) = shaft;
    let radius = (diameter * 0.4).max(settings.min_column_diameter_mm / 2.0);
    let top = lo + (hi - lo) * COMPANION_SHARE;
    let base = settings.base_diameter_mm / 2.0;
    let foot_top = floor + settings.base_height_mm;
    let gap = COMPANION_GAP_MM.max(settings.base_diameter_mm + FOOT_APART_MM);
    for k in 0..8 {
        let a = f64::from(k) * std::f64::consts::TAU / 8.0;
        let (cx, cy) = (x + gap * a.cos(), y + gap * a.sin());
        if let Some(half) = plate
            && (cx.abs() > half[0] - base || cy.abs() > half[1] - base)
        {
            continue;
        }
        if !bar_clear(
            solid,
            [cx, cy, floor + 0.01],
            [cx, cy, top + radius],
            base + COLUMN_GAP_MM,
        ) {
            continue;
        }
        let mut parts = vec![result.columns[index].clone()];
        let rings = [(floor, base), (foot_top, radius), (top, radius)];
        let vertices = rings
            .iter()
            .flat_map(|&(z, r)| {
                (0..ROUND_SIDES).map(move |i| {
                    let a = f64::from(i) * std::f64::consts::TAU / f64::from(ROUND_SIDES);
                    Vertex::new([
                        (cx + r * a.cos()) as f32,
                        (cy + r * a.sin()) as f32,
                        z as f32,
                    ])
                })
            })
            .collect();
        parts.push(closed_rings(vertices, ROUND_SIDES)?);
        let rod_radius = (radius * 0.6).max(0.25);
        let mut z = lo + 1.0;
        let mut out = true;
        while z + gap <= top {
            let (p, q) = if out {
                ([x, y], [cx, cy])
            } else {
                ([cx, cy], [x, y])
            };
            let (from, to) = ([p[0], p[1], z], [q[0], q[1], z + gap]);
            if bar_clear(solid, from, to, rod_radius + COLUMN_GAP_MM) {
                parts.push(rod(from, to, rod_radius)?);
            }
            z += gap;
            out = !out;
        }
        // A companion with no brace to its shaft steadies nothing: left out
        // (wide feet push it too far away for a short shaft, R89).
        if parts.len() < 3 {
            return Ok(());
        }
        if let Some(merged) = merge_meshes(parts.iter()) {
            result.columns[index] = merged;
        }
        return Ok(());
    }
    Ok(())
}

/// A closed six-sided rod between two points (axis to axis).
fn rod(from: [f64; 3], to: [f64; 3], radius: f64) -> Result<IndexedMesh, SupportError> {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let sides = sides_for(radius);
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1e-6 {
        return Err(SupportError::UnrepresentableGeometry);
    }
    let w = [d[0] / len, d[1] / len, d[2] / len];
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let helper = if w[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let u = cross(helper, w);
    let ul = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt();
    let u = [u[0] / ul, u[1] / ul, u[2] / ul];
    // u x v = w, so each ring winds counter-clockwise about the rod axis,
    // the same way the vertical columns' rings do about +Z.
    let v = cross(w, u);
    let vertices = [from, to]
        .iter()
        .flat_map(|c| {
            (0..sides).map(move |i| {
                let a = f64::from(i) * std::f64::consts::TAU / f64::from(sides);
                let (s, k) = a.sin_cos();
                Vertex::new([
                    (c[0] + radius * (u[0] * k + v[0] * s)) as f32,
                    (c[1] + radius * (u[1] * k + v[1] * s)) as f32,
                    (c[2] + radius * (u[2] * k + v[2] * s)) as f32,
                ])
            })
        })
        .collect();
    closed_rings(vertices, sides)
}

/// Sides of every round support part (pillars, tubes, rods, balls). Fewer
/// read as faceted in the viewer (R88: 10 and 12 looked low resolution).
pub(crate) const ROUND_SIDES: u32 = 48;
/// The longest facet (mm) round a small part: tips, branches, balls and
/// rods get as many sides as keep their facets this short, up to
/// [`ROUND_SIDES`], so a thin part is as smooth to the eye as a pillar
/// without the pillar's triangle count (R89: 24 sides on every part doubled
/// the prepare benchmark's time).
const FACET_MM: f64 = 0.08;

thread_local! {
    /// Set while auto orient measures a candidate pose's supports.
    static COARSE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `f` with small round parts (tips, branches, balls, rods) built with
/// 8 sides: auto orient builds every finalist pose's supports only to
/// measure them (footprint, branch lengths, contact areas), and 48-sided
/// balls were most of that time (R90). Pillars and their feet keep full
/// detail, so contact areas read the same. Support building never leaves the
/// calling thread, so a thread-local flag reaches all of it.
pub(crate) fn with_coarse_meshes<R>(f: impl FnOnce() -> R) -> R {
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            COARSE.with(|c| c.set(self.0));
        }
    }
    let _reset = Reset(COARSE.with(|c| c.replace(true)));
    f()
}

/// Sides for a round part of `radius`: a multiple of four, 8 to
/// [`ROUND_SIDES`]; 8 while orient measures poses ([`with_coarse_meshes`]).
pub(crate) fn sides_for(radius: f64) -> u32 {
    if COARSE.with(std::cell::Cell::get) {
        return 8;
    }
    let wanted = (std::f64::consts::TAU * radius / FACET_MM).ceil();
    let wanted = if wanted.is_finite() {
        wanted as u32
    } else {
        ROUND_SIDES
    };
    (wanted.div_ceil(4) * 4).clamp(8, ROUND_SIDES)
}

/// The most any support member leans from vertical (degrees), whatever the
/// branch angle setting: a bar leaning further has an underside that is
/// itself an overhang, a support that would need supporting (R91). Resin
/// slicers keep support sticks to the resin's 45 degrees (PrusaSlicer's
/// critical angle).
/// A tip near the plate gets this share of its height over the floor, at
/// most the full tip length (R104: a third made low tips stubby).
pub(crate) const TIP_SHARE: f64 = 0.45;

/// The tip length for a contact `height` mm over what its column stands on.
pub(crate) fn tip_length_above(settings: &SupportSettings, height: f64) -> f64 {
    settings.tip_length_mm.min(height * TIP_SHARE).max(0.0)
}

/// The radius where a tip of `length` ends, under a column or arm of
/// radius `neck`: a short tip is a full tip cut short, not squashed, so it
/// widens at the same rate and ends narrower (R104: low tips widened six
/// times faster and sat fat and buried in the part). The cone, the ball under
/// it and an arm leaving it all meet at this radius, so no lip sticks out.
pub(crate) fn tip_end_radius(settings: &SupportSettings, length: f64, neck: f64) -> f64 {
    let contact = settings.tip_contact_diameter_mm / 2.0;
    let share = (length / settings.tip_length_mm.max(1e-6)).clamp(0.0, 1.0);
    (contact + (neck - contact).max(0.0) * share).max(contact)
}

pub(crate) const MAX_LEAN_DEG: f64 = 45.0;

/// The vertical lip (mm) under a raft's bevelled edge.
const RAFT_LIP_MM: f64 = 0.2;

/// Standard resin-slicer raft: a chamfered slab following the outline of the
/// column bases (their convex hull grown by `margin`, with rounded corners),
/// not the bounding rectangle of every support on the plate.
fn footprint_raft(
    points: &[SupportPoint],
    margin: f64,
    foot_radius: f64,
    thickness: f64,
    plate: Option<[f64; 2]>,
) -> Result<IndexedMesh, SupportError> {
    bevelled_slab(points, margin, foot_radius, thickness, plate, 96)
}

/// A slab around `points` (their convex hull grown by `margin`) with the
/// solid raft's bevelled edge, its outline in `SIDES` steps.
#[allow(non_snake_case)]
fn bevelled_slab(
    points: &[SupportPoint],
    margin: f64,
    foot_radius: f64,
    thickness: f64,
    plate: Option<[f64; 2]>,
    SIDES: usize,
) -> Result<IndexedMesh, SupportError> {
    let core = convex_hull(
        points
            .iter()
            .map(|p| [f64::from(p.position[0]), f64::from(p.position[1])])
            .collect(),
    );
    // The whole edge slopes at 45 degrees from a thin lip on the plate, as
    // PreForm's and PrusaSlicer's pads do, so a scraper slides under it
    // (R87; a small top chamfer left a square edge to pry at).
    // A short vertical lip stays, since a knife edge does not cure.
    let lip = RAFT_LIP_MM.min(thickness / 2.0);
    // The top keeps covering every foot: with no raft margin the bevel cut
    // in under the feet's edges (R89).
    let chamfer = (thickness - lip)
        .min(margin / 2.0)
        .min((margin - foot_radius).max(0.0));
    // With no room for a bevel the edge is straight; two rings at the same
    // height would leave the mesh open.
    let rings = if chamfer > 1e-3 {
        [
            (0.0, margin),
            (thickness - chamfer, margin),
            (thickness, margin - chamfer),
        ]
    } else {
        [
            (0.0, margin),
            (thickness / 2.0, margin),
            (thickness, margin),
        ]
    };
    let clipped: Vec<Vec<[f64; 2]>> = rings
        .iter()
        .map(|&(_, r)| {
            let outline = rounded_offset(&core, r);
            match plate {
                Some(half) => clip_to_rect(&outline, half),
                None => outline,
            }
        })
        .collect();
    if clipped.iter().any(|o| o.len() < 3) {
        return Err(SupportError::UnrepresentableGeometry);
    }
    // Resample every ring by angle around one centre so all rings have the
    // same vertex count and line up, however the plate edge cut them.
    let centre = polygon_centroid(&clipped[2]);
    let outlines: Vec<Vec<[f64; 2]>> = clipped
        .iter()
        .map(|outline| resample_by_angle(outline, centre, SIDES))
        .collect();
    let sides = SIDES as u32;
    let vertices = rings
        .iter()
        .zip(&outlines)
        .flat_map(|(&(z, _), outline)| {
            outline
                .iter()
                .map(move |[x, y]| Vertex::new([*x as f32, *y as f32, z as f32]))
        })
        .collect();
    closed_rings_centre_capped(vertices, sides)
}

/// The raft for these columns: solid footprint slab, or the lattice when asked
/// for. With `plate` (half width, half depth) it never leaves the plate.
fn make_raft(
    points: &[SupportPoint],
    settings: &SupportSettings,
    plate: Option<[f64; 2]>,
    target: f64,
) -> Result<IndexedMesh, SupportError> {
    let margin = settings.base_diameter_mm / 2.0 + settings.raft_margin_mm;
    if settings.skate_raft {
        return skate_raft(points, settings, plate);
    }
    if settings.lattice_raft {
        lattice_raft(points, settings, plate, target)
    } else {
        footprint_raft(
            points,
            margin,
            settings.base_diameter_mm / 2.0,
            settings.raft_thickness_mm,
            plate,
        )
    }
}

/// Sutherland-Hodgman clip of a convex polygon to the plate rectangle
/// `[-half, half]` (scene XY is plate-centred).
fn clip_to_rect(polygon: &[[f64; 2]], half: [f64; 2]) -> Vec<[f64; 2]> {
    let mut out = polygon.to_vec();
    for (axis, sign) in [(0, 1.0), (0, -1.0), (1, 1.0), (1, -1.0)] {
        let limit = half[axis];
        let inside = |p: &[f64; 2]| sign * p[axis] <= limit;
        let input = std::mem::take(&mut out);
        for i in 0..input.len() {
            let a = input[i];
            let b = input[(i + 1) % input.len()];
            if inside(&a) {
                out.push(a);
            }
            if inside(&a) != inside(&b) {
                let t = (sign * limit - a[axis]) / (b[axis] - a[axis]);
                out.push([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
            }
        }
    }
    out
}

fn polygon_centroid(polygon: &[[f64; 2]]) -> [f64; 2] {
    let n = polygon.len() as f64;
    let sum = polygon
        .iter()
        .fold([0.0, 0.0], |acc, p| [acc[0] + p[0], acc[1] + p[1]]);
    [sum[0] / n, sum[1] / n]
}

/// `count` points on a convex polygon's boundary, one per ray from `centre`
/// at evenly spaced angles (counter-clockwise).
fn resample_by_angle(polygon: &[[f64; 2]], centre: [f64; 2], count: usize) -> Vec<[f64; 2]> {
    (0..count)
        .map(|k| {
            let a = k as f64 * std::f64::consts::TAU / count as f64;
            let d = [a.cos(), a.sin()];
            let mut best = 0.0_f64;
            for i in 0..polygon.len() {
                let p = polygon[i];
                let q = polygon[(i + 1) % polygon.len()];
                let e = [q[0] - p[0], q[1] - p[1]];
                let denom = d[0] * e[1] - d[1] * e[0];
                if denom.abs() < 1e-12 {
                    continue;
                }
                let w = [p[0] - centre[0], p[1] - centre[1]];
                let t = (w[0] * e[1] - w[1] * e[0]) / denom;
                let u = (w[0] * d[1] - w[1] * d[0]) / denom;
                if t > 0.0 && (-1e-9..=1.0 + 1e-9).contains(&u) {
                    best = best.max(t);
                }
            }
            [centre[0] + best * d[0], centre[1] + best * d[1]]
        })
        .collect()
}

/// Counter-clockwise convex hull (Andrew's monotone chain); duplicates and
/// collinear points are dropped. One or two points come back as-is.
pub(crate) fn convex_hull(mut points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
    if points.len() < 3 {
        return points;
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut hull: Vec<[f64; 2]> = Vec::with_capacity(points.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &[f64; 2]>> = if pass == 0 {
            Box::new(points.iter())
        } else {
            Box::new(points.iter().rev())
        };
        for &p in iter {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 1e-12
            {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

/// Grow a convex CCW polygon by `radius` with round corners. The vertex count
/// depends only on the polygon, not the radius, so outlines of the same hull
/// at different radii stack into one closed ring mesh.
pub(crate) fn rounded_offset(hull: &[[f64; 2]], radius: f64) -> Vec<[f64; 2]> {
    const STEPS: usize = 32;
    let n = hull.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return (0..STEPS)
            .map(|k| {
                let a = k as f64 * std::f64::consts::TAU / STEPS as f64;
                [hull[0][0] + radius * a.cos(), hull[0][1] + radius * a.sin()]
            })
            .collect();
    }
    // Outward normal angle of the edge leaving vertex i (CCW: right of edge).
    let normal_angle = |i: usize| {
        let a = hull[i];
        let b = hull[(i + 1) % n];
        (b[1] - a[1]).atan2(b[0] - a[0]) - std::f64::consts::FRAC_PI_2
    };
    let mut out = Vec::new();
    for i in 0..n {
        let from = normal_angle((i + n - 1) % n);
        let mut to = normal_angle(i);
        while to < from {
            to += std::f64::consts::TAU;
        }
        // Each corner gets its share of a full turn (fixed for this hull).
        let steps = (((to - from) / std::f64::consts::TAU) * STEPS as f64)
            .ceil()
            .max(1.0) as usize;
        for k in 0..=steps {
            let a = from + (to - from) * k as f64 / steps as f64;
            out.push([hull[i][0] + radius * a.cos(), hull[i][1] + radius * a.sin()]);
        }
    }
    out
}

/// Fill cells until no two filled cells touch only at a corner: two such
/// cells share a vertical edge with four faces, a non-manifold mesh the
/// slicer refuses (a pad's round edge meeting a grid bar did this on most
/// lattice rafts). Of the two empty cells beside a corner touch, one inside
/// the raft outline is filled first.
pub(crate) fn close_corner_touches(
    mut filled: Vec<bool>,
    nx: usize,
    ny: usize,
    inside: impl Fn(usize, usize) -> bool,
) -> Vec<bool> {
    loop {
        let mut changed = false;
        for y in 0..ny.saturating_sub(1) {
            for x in 0..nx.saturating_sub(1) {
                let at = |x: usize, y: usize| y * nx + x;
                let (a, b) = (filled[at(x, y)], filled[at(x + 1, y)]);
                let (c, d) = (filled[at(x, y + 1)], filled[at(x + 1, y + 1)]);
                let fill = if a && d && !b && !c {
                    Some(if inside(x + 1, y) || !inside(x, y + 1) {
                        (x + 1, y)
                    } else {
                        (x, y + 1)
                    })
                } else if b && c && !a && !d {
                    Some(if inside(x, y) || !inside(x + 1, y + 1) {
                        (x, y)
                    } else {
                        (x + 1, y + 1)
                    })
                } else {
                    None
                };
                if let Some((x, y)) = fill {
                    filled[at(x, y)] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            return filled;
        }
    }
}

/// Factor over the plate contact Auto orient counts as adequate for a model
/// (orient's ADHESION_* rates). Its adequacy already sits where more
/// adhesion stops helping; twice it asked more of a tall part's lattice than
/// the solid raft gives (281 against 179 mm2), so the lattice keeps to it.
pub(crate) const RAFT_ADHESION_SAFETY: f64 = 1.0;

/// Width of a lattice raft strut: the thinnest bar that survives removal in
/// one piece, about a column's thinnest diameter and a bit more.
const LATTICE_STRUT_MM: f64 = 1.2;

/// The plate contact a model needs from its raft (R105), mm2: Auto orient's
/// adequate adhesion (20 mm2 + 0.15 of its largest layer + 0.5 per mm of
/// height + 0.01 per mm3 of volume) times RAFT_ADHESION_SAFETY.
pub(crate) fn raft_target_area(tris: &[[Vertex; 3]]) -> f64 {
    let load = part_load(tris);
    RAFT_ADHESION_SAFETY * (20.0 + 0.15 * load.peak_layer_mm2 + 0.5 * load.height_mm + 0.01 * load.volume_mm3)
}

/// What a part asks of its supports and raft (R106).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PartLoad {
    /// Its largest layer, mm2: the most peel force one layer puts on it.
    pub peak_layer_mm2: f64,
    /// Its printing height, mm.
    pub height_mm: f64,
    /// Its volume, mm3.
    pub volume_mm3: f64,
}

/// The peak layer, height and volume of a part as it sits.
pub fn part_load(tris: &[[Vertex; 3]]) -> PartLoad {
    if tris.is_empty() {
        return PartLoad::default();
    }
    let p = |v: &Vertex| glam::DVec3::from_array(v.coords().map(f64::from));
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut volume = 0.0;
    let mut centre = glam::DVec3::ZERO;
    for t in tris {
        let [a, b, c] = t.each_ref().map(p);
        lo = lo.min(a.z).min(b.z).min(c.z);
        hi = hi.max(a.z).max(b.z).max(c.z);
        volume += a.dot(b.cross(c)) / 6.0;
        centre += (a + b + c) / 3.0;
    }
    centre /= tris.len() as f64;
    // The largest layer: shoelace sums of the sections at 48 heights.
    let mut peak: f64 = 0.0;
    for k in 0..48 {
        let z = lo + (hi - lo) * (f64::from(k) + 0.5) / 48.0;
        let mut area = 0.0;
        for t in tris {
            let v = t.each_ref().map(p);
            let normal = (v[1] - v[0]).cross(v[2] - v[0]);
            let mut hits = [glam::DVec3::ZERO; 2];
            let mut count = 0;
            for (a, b) in [(v[0], v[1]), (v[1], v[2]), (v[2], v[0])] {
                if (a.z - z) * (b.z - z) < 0.0 && count < 2 {
                    hits[count] = a + (b - a) * ((z - a.z) / (b.z - a.z));
                    count += 1;
                }
            }
            if count == 2 {
                let [a, b] = hits.map(|h| h - centre);
                let sign = if (b - a).dot(glam::DVec3::Z.cross(normal)) >= 0.0 { 1.0 } else { -1.0 };
                area += sign * (a.x * b.y - b.x * a.y) * 0.5;
            }
        }
        peak = peak.max(area.abs());
    }
    PartLoad {
        peak_layer_mm2: peak,
        height_mm: hi - lo,
        volume_mm3: volume.abs(),
    }
}

/// When the solid rafts of everything on a plate would cover this share of
/// it, Auto prepare picks the lattice raft (R106): rafts print together in
/// the first layers, so their areas add into one peel; the lattice holds each
/// part with far less (a flat 36 x 24 mm part: 732 against 200 mm2).
pub const LATTICE_PLATE_SHARE: f64 = 0.05;

/// The support settings Auto prepare tunes for a plate (R106).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SupportChoice {
    /// Every support setting, tuned to the parts.
    pub settings: SupportSettings,
    /// How demanding the plate's most demanding part is, 0 (a small light
    /// part) to 1 (a 2000 mm2 layer, 150 mm tall or 150 mL).
    pub load: f64,
    /// Lattice raft (least peel) rather than solid.
    pub lattice: bool,
    /// Share of the plate the parts' solid rafts would cover.
    pub raft_share: f64,
    /// Parts weighed.
    pub parts: usize,
    /// Of those, the parts lifted onto a raft, which hang on their supports
    /// (the others print on the plate and keep the user's settings).
    pub lifted: usize,
}

/// Whether a part prints straight on the plate as it sits: Auto orient's
/// rule (plate contact at least 2 mm2 + 0.5 mm2 per mm of height and a tenth
/// of its footprint). Its supports then hold only overhangs; the plate holds
/// its weight and peel.
fn prints_on_plate(tris: &[[Vertex; 3]], height: f64) -> bool {
    let p = |v: &Vertex| glam::DVec3::from_array(v.coords().map(f64::from));
    let (mut lo, mut min, mut max) = (f64::INFINITY, glam::DVec2::splat(f64::INFINITY), glam::DVec2::splat(f64::NEG_INFINITY));
    for t in tris {
        for v in t {
            let q = p(v);
            lo = lo.min(q.z);
            min = min.min(q.truncate());
            max = max.max(q.truncate());
        }
    }
    let bed: f64 = tris
        .iter()
        .map(|t| t.each_ref().map(p))
        .filter(|t| t.iter().all(|q| q.z - lo < 0.05))
        .map(|t| (t[1] - t[0]).cross(t[2] - t[0]).z.abs() / 2.0)
        .sum();
    let footprint = (max - min).x * (max - min).y;
    bed >= 2.0 + 0.5 * height && bed >= 0.1 * footprint
}

/// Tunes every support setting to the `parts` on a plate of `plate_mm2`
/// (R106), starting from `base` (the user's settings, whose own choices -
/// overhang angle from the resin, symmetric supports, trees, branch angle,
/// base size - are kept). Settings apply to the whole plate, so they follow
/// its most demanding part: its largest layer (the peel force on it), height
/// (leverage) and volume (weight) give a load from 0 to 1, and each
/// load-bearing setting moves from the Light preset's value to the Heavy
/// preset's with it - tips, tip length, contact depth (never under two 50 um
/// layers, R84), shafts and column limits, contact spacing (closer when
/// heavier), lift and raft thickness. The raft is the lattice once all the
/// solid rafts together would cover LATTICE_PLATE_SHARE of the plate (each
/// estimated from the part's outline plus the raft margin, as the solid raft
/// follows its feet), so more parts lean to the lattice. Only parts lifted
/// onto a raft count: a part printing on the plate has the plate carry its
/// weight and peel, and its supports hold only overhangs (R107: a Benchy got
/// 9 heavy contacts for its whole 1000 mm2 hull, against 4 before). With no
/// lifted part the user's settings are returned unchanged.
pub fn choose_supports(
    parts: &[IndexedMesh],
    plate_mm2: f64,
    base: &SupportSettings,
) -> SupportChoice {
    let mut load: f64 = 0.0;
    let mut raft = 0.0;
    let mut lifted = 0;
    for mesh in parts {
        let Ok(tris) = triangles(mesh) else { continue };
        let part = part_load(&tris);
        if prints_on_plate(&tris, part.height_mm) {
            continue;
        }
        lifted += 1;
        load = load.max(
            (part.peak_layer_mm2 / 2000.0)
                .max(part.height_mm / 150.0)
                .max(part.volume_mm3 / 150_000.0),
        );
        let outline: Vec<[f64; 2]> = mesh
            .vertices
            .iter()
            .map(|v| [f64::from(v[0]), f64::from(v[1])])
            .collect();
        let rim = rounded_offset(
            &convex_hull(outline),
            base.base_diameter_mm / 2.0 + base.raft_margin_mm,
        );
        raft += polygon_area(&rim);
    }
    let load = load.clamp(0.0, 1.0);
    let share = if plate_mm2 > 0.0 { raft / plate_mm2 } else { 0.0 };
    if lifted == 0 {
        return SupportChoice {
            settings: *base,
            load: 0.0,
            lattice: base.lattice_raft,
            raft_share: 0.0,
            parts: parts.len(),
            lifted: 0,
        };
    }
    let lerp = |light: f64, heavy: f64| light + (heavy - light) * load;
    let mut settings = *base;
    settings.tip_contact_diameter_mm = lerp(0.25, 0.7);
    settings.tip_length_mm = lerp(1.5, 2.5);
    settings.contact_depth_mm = lerp(0.1, 0.2);
    settings.shaft_diameter_mm = lerp(0.8, 2.0);
    settings.min_column_diameter_mm = settings.shaft_diameter_mm;
    settings.max_column_diameter_mm = (settings.shaft_diameter_mm * 1.6).min(settings.base_diameter_mm);
    settings.spacing_mm = lerp(4.5, 3.0);
    settings.lift_mm = lerp(4.0, 6.0);
    settings.raft_thickness_mm = lerp(1.0, 1.2);
    settings.raft = true;
    settings.lattice_raft = share >= LATTICE_PLATE_SHARE;
    settings.skate_raft = false;
    SupportChoice {
        settings,
        load,
        lattice: settings.lattice_raft,
        raft_share: share,
        parts: parts.len(),
        lifted,
    }
}

fn polygon_area(polygon: &[[f64; 2]]) -> f64 {
    (0..polygon.len())
        .map(|i| {
            let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

/// The lattice raft, computed for the least area (R105). Every raft layer has
/// the same outline, so its peel force each layer goes with its area: the
/// least area that still does a raft's jobs peels least. Those jobs, and what
/// each costs:
/// - hold the part to the plate: plate contact of at least `target`
///   (raft_target_area);
/// - carry every column: a round pad covering each foot's base;
/// - come off the plate in one stiff piece: struts tie the feet together
///   along the Gabriel graph (no crossings, contains the shortest tree, mostly
///   triangles so it does not flex), straight (the shortest path is the
///   least area) and LATTICE_STRUT_MM wide.
/// When struts and pads are short of the target, the pads grow evenly until
/// it is met. Every piece keeps the solid raft's bevelled edge.
fn lattice_raft(
    points: &[SupportPoint],
    settings: &SupportSettings,
    plate: Option<[f64; 2]>,
    target: f64,
) -> Result<IndexedMesh, SupportError> {
    let mut feet: Vec<[f64; 2]> = Vec::new();
    for p in points {
        let f = [f64::from(p.position[0]), f64::from(p.position[1])];
        if !feet.iter().any(|g| (g[0] - f[0]).hypot(g[1] - f[1]) < 1e-3) {
            feet.push(f);
        }
    }
    let foot = settings.base_diameter_mm / 2.0;
    let thickness = settings.raft_thickness_mm;
    let half = LATTICE_STRUT_MM / 2.0;
    let struts = raft_struts(&feet);
    let pad = lattice_pad_radius(&feet, &struts, foot, half, target);
    let at = |f: [f64; 2]| SupportPoint {
        position: [f[0] as f32, f[1] as f32, 0.0],
        normal: [0.0, 0.0, -1.0],
    };
    let mut pieces = Vec::new();
    for &(a, b) in &struts {
        pieces.push(web_bar(feet[a], feet[b], half, thickness)?);
    }
    for f in &feet {
        pieces.push(bevelled_slab(&[at(*f)], pad, foot, thickness, plate, 40)?);
    }
    merge_meshes(pieces.iter()).ok_or(SupportError::InvalidContact)
}

/// One strut of a lattice raft: a straight bar `half` mm wide either side of
/// a-b with round ends, its edge bevelled like the solid raft's. Built as a
/// true stadium (resampling a thin bar by angle around its centre made it a
/// spiky diamond).
fn web_bar(a: [f64; 2], b: [f64; 2], half: f64, thickness: f64) -> Result<IndexedMesh, SupportError> {
    const END: usize = 8;
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    let d = if len < 1e-9 { [1.0, 0.0] } else { [(b[0] - a[0]) / len, (b[1] - a[1]) / len] };
    let n = [-d[1], d[0]];
    let lip = RAFT_LIP_MM.min(thickness / 2.0);
    let chamfer = (thickness - lip).min(half / 2.0);
    let rings = [(0.0, half), (thickness - chamfer, half), (thickness, half - chamfer)];
    let outline = |r: f64| -> Vec<[f64; 2]> {
        let mut out = Vec::with_capacity(2 * (END + 1));
        for (centre, from) in [(b, -0.5), (a, 0.5)] {
            for k in 0..=END {
                let t = (from + k as f64 / END as f64) * std::f64::consts::PI;
                out.push([
                    centre[0] + r * (t.cos() * d[0] + t.sin() * n[0]),
                    centre[1] + r * (t.cos() * d[1] + t.sin() * n[1]),
                ]);
            }
        }
        out
    };
    let sides = (2 * (END + 1)) as u32;
    let vertices = rings
        .iter()
        .flat_map(|&(z, r)| {
            outline(r)
                .into_iter()
                .map(move |[x, y]| Vertex::new([x as f32, y as f32, z as f32]))
        })
        .collect();
    closed_rings_centre_capped(vertices, sides)
}

/// The pad radius that brings a lattice raft's plate contact up to `target`:
/// at least a foot's base and half a millimetre, more (the same for all) when
/// struts and minimal pads fall short. Overlaps are not counted, so the
/// estimate stays on the safe side only where pads and struts are apart.
pub(crate) fn lattice_pad_radius(
    feet: &[[f64; 2]],
    struts: &[(usize, usize)],
    foot: f64,
    half: f64,
    target: f64,
) -> f64 {
    let least = foot + 0.5;
    if feet.is_empty() {
        return least;
    }
    let strut_area: f64 = struts
        .iter()
        .map(|&(a, b)| {
            let length = (feet[a][0] - feet[b][0]).hypot(feet[a][1] - feet[b][1]);
            // Only the part outside the two minimal pads adds contact.
            (length - 2.0 * least).max(0.0) * 2.0 * half
        })
        .sum();
    let per_pad = ((target - strut_area) / feet.len() as f64).max(0.0);
    (per_pad / std::f64::consts::PI).sqrt().max(least)
}

/// The struts of a lattice raft between `feet`: the Gabriel graph (two feet
/// are joined when no other foot lies inside the circle across them), which
/// is planar, holds the minimum spanning tree and is mostly triangles; the
/// tree's edges are kept as well, so the raft is always one piece.
pub(crate) fn raft_struts(feet: &[[f64; 2]]) -> Vec<(usize, usize)> {
    let n = feet.len();
    if n < 2 {
        return Vec::new();
    }
    let d = |a: usize, b: usize| (feet[a][0] - feet[b][0]).hypot(feet[a][1] - feet[b][1]);
    let mut edges = std::collections::BTreeSet::new();
    // Minimum spanning tree (Prim).
    let mut joined = vec![false; n];
    let mut best: Vec<(f64, usize)> = (0..n).map(|j| (d(0, j), 0)).collect();
    joined[0] = true;
    for _ in 1..n {
        let Some(next) = (0..n)
            .filter(|&j| !joined[j])
            .min_by(|&a, &b| best[a].0.total_cmp(&best[b].0))
        else {
            break;
        };
        joined[next] = true;
        let from = best[next].1;
        edges.insert((from.min(next), from.max(next)));
        for j in 0..n {
            if !joined[j] && d(next, j) < best[j].0 {
                best[j] = (d(next, j), next);
            }
        }
    }
    for a in 0..n {
        for b in a + 1..n {
            let mid = [(feet[a][0] + feet[b][0]) / 2.0, (feet[a][1] + feet[b][1]) / 2.0];
            let radius = d(a, b) / 2.0;
            if (0..n).all(|c| {
                c == a || c == b || (feet[c][0] - mid[0]).hypot(feet[c][1] - mid[1]) > radius + 1e-6
            }) {
                edges.insert((a, b));
            }
        }
    }
    edges.into_iter().collect()
}

/// The filled cells of a raft grid as one welded slab: exposed faces only,
/// adjacent cells share vertices and have no interior walls.
fn extrude_cells(
    lo: [f64; 2],
    hi: [f64; 2],
    pitch: f64,
    nx: usize,
    ny: usize,
    filled: &[bool],
    thickness: f64,
) -> Result<IndexedMesh, SupportError> {
    let mut indices = Vec::new();
    let mut vertices = Vec::new();
    let mut vertex_map = std::collections::BTreeMap::new();
    for y in 0..ny {
        for x in 0..nx {
            if !filled[y * nx + x] {
                continue;
            }
            let corners = [
                [x, y, 0],
                [x + 1, y, 0],
                [x + 1, y + 1, 0],
                [x, y + 1, 0],
                [x, y, 1],
                [x + 1, y, 1],
                [x + 1, y + 1, 1],
                [x, y + 1, 1],
            ];
            let faces = [
                ([0, 3, 2, 1], true),
                ([4, 5, 6, 7], true),
                ([0, 1, 5, 4], y == 0 || !filled[(y - 1) * nx + x]),
                ([1, 2, 6, 5], x + 1 == nx || !filled[y * nx + x + 1]),
                ([2, 3, 7, 6], y + 1 == ny || !filled[(y + 1) * nx + x]),
                ([3, 0, 4, 7], x == 0 || !filled[y * nx + x - 1]),
            ];
            for (face, exposed) in faces {
                if exposed {
                    let quad = face.map(|i| {
                        let key = corners[i];
                        *vertex_map.entry(key).or_insert_with(|| {
                            let id = vertices.len() as u32;
                            vertices.push(Vertex::new([
                                (lo[0] + (key[0] as f64 * pitch).min(hi[0] - lo[0])) as f32,
                                (lo[1] + (key[1] as f64 * pitch).min(hi[1] - lo[1])) as f32,
                                (key[2] as f64 * thickness) as f32,
                            ]));
                            id
                        })
                    });
                    indices.extend([[quad[0], quad[1], quad[2]], [quad[0], quad[2], quad[3]]]);
                }
            }
        }
    }
    let faces = indices
        .into_iter()
        .map(|ids| {
            let normal = compute_normal_from_triangle_vertices(&ids.map(|i| vertices[i as usize]))
                .map_err(|_| SupportError::UnrepresentableGeometry)?;
            Ok(IndexedTriangle::from_raw_indices(normal, ids))
        })
        .collect::<Result<_, SupportError>>()?;
    Ok(IndexedMesh::new(None, vertices, faces))
}

/// Like [`closed_rings`], but the bottom and top caps fan from an added
/// centre vertex: densely sampled outlines have collinear runs, and a fan from
/// a boundary vertex would make zero-area triangles along them.
fn closed_rings_centre_capped(
    mut vertices: Vec<Vertex>,
    sides: u32,
) -> Result<IndexedMesh, SupportError> {
    if vertices.iter().any(|v| v.iter().any(|c| !c.is_finite())) {
        return Err(SupportError::UnrepresentableGeometry);
    }
    let rings = vertices.len() as u32 / sides;
    let centre_of = |ring: u32, vertices: &[Vertex]| {
        let ring = &vertices[(ring * sides) as usize..((ring + 1) * sides) as usize];
        let n = ring.len() as f32;
        let sum = ring.iter().fold([0.0f32; 3], |acc, v| {
            let c = v.coords();
            [acc[0] + c[0], acc[1] + c[1], acc[2] + c[2]]
        });
        Vertex::new([sum[0] / n, sum[1] / n, sum[2] / n])
    };
    let bottom_centre = centre_of(0, &vertices);
    let top_centre = centre_of(rings - 1, &vertices);
    let bottom = vertices.len() as u32;
    vertices.push(bottom_centre);
    vertices.push(top_centre);
    let top = bottom + 1;
    let mut indices = Vec::new();
    for ring in 0..rings - 1 {
        for i in 0..sides {
            let a = ring * sides + i;
            let b = ring * sides + (i + 1) % sides;
            indices.extend([[a, b, b + sides], [a, b + sides, a + sides]]);
        }
    }
    let last = (rings - 1) * sides;
    for i in 0..sides {
        let j = (i + 1) % sides;
        indices.extend([[bottom, j, i], [top, last + i, last + j]]);
    }
    let faces = indices
        .into_iter()
        .filter_map(|ids| {
            compute_normal_from_triangle_vertices(&ids.map(|i| vertices[i as usize]))
                .ok()
                .map(|normal| IndexedTriangle::from_raw_indices(normal, ids))
        })
        .collect::<Vec<_>>();
    Ok(IndexedMesh::new(None, vertices, faces))
}

fn closed_rings(vertices: Vec<Vertex>, sides: u32) -> Result<IndexedMesh, SupportError> {
    if vertices.iter().any(|v| v.iter().any(|c| !c.is_finite())) {
        return Err(SupportError::UnrepresentableGeometry);
    }
    let rings = vertices.len() as u32 / sides;
    let mut indices = Vec::new();
    for ring in 0..rings - 1 {
        for i in 0..sides {
            let a = ring * sides + i;
            let b = ring * sides + (i + 1) % sides;
            indices.extend([[a, b, b + sides], [a, b + sides, a + sides]]);
        }
    }
    let top = (rings - 1) * sides;
    for i in 1..sides - 1 {
        indices.extend([[0, i + 1, i], [top, top + i, top + i + 1]]);
    }
    let faces = indices
        .into_iter()
        .map(|ids| {
            let normal = compute_normal_from_triangle_vertices(&ids.map(|i| vertices[i as usize]))
                .map_err(|_| SupportError::UnrepresentableGeometry)?;
            Ok(IndexedTriangle::from_raw_indices(normal, ids))
        })
        .collect::<Result<_, SupportError>>()?;
    Ok(IndexedMesh::new(None, vertices, faces))
}

/// The nearest point (within 1 mm in plan) over a model's surface to
/// `position`, nudged just inside that face, at the same height; a contact
/// there has an owner. `None` when no model is that close.
pub(crate) fn snap_onto_model(scene: &sc_scene::Scene, position: [f32; 3]) -> Option<[f32; 3]> {
    const REACH_MM: f64 = 1.0;
    let [x, y, z] = position.map(f64::from);
    let closest_on_segment = |a: [f64; 2], b: [f64; 2]| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let t =
            (((x - a[0]) * dx + (y - a[1]) * dy) / (dx * dx + dy * dy).max(1e-30)).clamp(0.0, 1.0);
        [a[0] + t * dx, a[1] + t * dy]
    };
    let mut best: Option<(f64, [f64; 2])> = None;
    for id in scene
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .ok()?
    {
        if scene
            .node(id)
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.comment.as_deref())
            .is_some_and(|s| s.starts_with("sc-generated-support"))
        {
            continue;
        }
        let Ok(mesh) = sc_scene::bake::bake_leaf_to_world(scene, id) else {
            continue;
        };
        for face in &mesh.faces {
            let [a, b, c] = face
                .vertices
                .map(|v| mesh.vertices[usize::from(v)].coords().map(f64::from));
            let det = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if det.abs() < 1e-12 {
                continue;
            }
            let near = [(a, b), (b, c), (c, a)]
                .map(|(p, q)| closest_on_segment([p[0], p[1]], [q[0], q[1]]))
                .into_iter()
                .min_by(|p, q| {
                    let d = |r: &[f64; 2]| (r[0] - x).powi(2) + (r[1] - y).powi(2);
                    d(p).total_cmp(&d(q))
                })?;
            let plan = ((near[0] - x).powi(2) + (near[1] - y).powi(2)).sqrt();
            if plan > REACH_MM {
                continue;
            }
            // Nudge the edge point a little toward the face's centre.
            let centre = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
            let inside = [
                near[0] + (centre[0] - near[0]) * 0.02,
                near[1] + (centre[1] - near[1]) * 0.02,
            ];
            let u = ((inside[0] - a[0]) * (c[1] - a[1]) - (inside[1] - a[1]) * (c[0] - a[0])) / det;
            let v = ((b[0] - a[0]) * (inside[1] - a[1]) - (b[1] - a[1]) * (inside[0] - a[0])) / det;
            let height = a[2] + u * (b[2] - a[2]) + v * (c[2] - a[2]);
            let score = plan * plan + (height - z).powi(2);
            if best.is_none_or(|(s, _)| score < s) {
                best = Some((score, inside));
            }
        }
    }
    best.map(|(_, [px, py])| [px as f32, py as f32, position[2]])
}

/// Associate contacts with their source model by the nearest surface at the
/// contact's XY position. Bakes each model once; preserves stable scene IDs.
pub fn support_owners(
    scene: &sc_scene::Scene,
    points: &[SupportPoint],
) -> Vec<Option<sc_scene::NodeId>> {
    let mut owners = vec![None; points.len()];
    let mut distances = vec![f64::INFINITY; points.len()];
    for id in scene
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .unwrap_or_default()
    {
        if scene
            .node(id)
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.comment.as_deref())
            .is_some_and(|s| s.starts_with("sc-generated-support"))
        {
            continue;
        }
        let Ok(mesh) = sc_scene::bake::bake_leaf_to_world(scene, id) else {
            continue;
        };
        // Only the points over this model's footprint: every point used to
        // be tested against every face of every model, over a billion tests
        // per call on a filled plate (Sleptic: auto supports very slow there).
        let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
        for v in &mesh.vertices {
            for axis in 0..2 {
                lo[axis] = lo[axis].min(v[axis]);
                hi[axis] = hi[axis].max(v[axis]);
            }
        }
        let over: Vec<usize> = (0..points.len())
            .filter(|&i| {
                (0..2).all(|axis| {
                    let c = points[i].position[axis];
                    c >= lo[axis] - 1e-3 && c <= hi[axis] + 1e-3
                })
            })
            .collect();
        if over.is_empty() {
            continue;
        }
        for face in &mesh.faces {
            let [a, b, c] = face
                .vertices
                .map(|v| mesh.vertices[usize::from(v)].coords().map(f64::from));
            let det = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if det.abs() < 1e-15 {
                continue;
            }
            for &i in &over {
                let point = &points[i];
                let [x, y, z] = point.position.map(f64::from);
                let u = ((x - a[0]) * (c[1] - a[1]) - (y - a[1]) * (c[0] - a[0])) / det;
                let v = ((b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0])) / det;
                if u < -1e-5 || v < -1e-5 || u + v > 1.00001 {
                    continue;
                }
                let distance = (a[2] + u * (b[2] - a[2]) + v * (c[2] - a[2]) - z).abs();
                if distance < distances[i] {
                    distances[i] = distance;
                    owners[i] = Some(id);
                }
            }
        }
    }
    owners
}

/// Clearance (mm) a column keeps from the parts. Light bleeds past a lit
/// pixel and edges round to whole pixels, so a column passing a wall closer
/// than a few pixels cures into it: a Benchy's wheel column fused to the
/// cabin wall 27 layers below its contact (Sleptic, R83).
const COLUMN_GAP_MM: f64 = 0.25;

/// Height over diameter a leaning column down to the plate may have.
const LEAN_MAX_SLENDERNESS: f64 = 20.0;
/// The thickest a leaning column gets (mm), whatever the pillar limit.
const LEAN_MAX_DIAMETER_MM: f64 = 3.0;

/// A leaning column: from the contact it runs straight down and away to a
/// knee, then plumb to the plate (`lower` None) or onto the part.
struct Lean {
    knee: [f64; 3],
    top: [f64; 3],
    lower: Option<f64>,
    diameter: f64,
    tip_length: f64,
}

/// The shortest leaning path from `contact` that stays clear of the parts.
/// First within the branch angle (at most 40 degrees from vertical): to the
/// plate if any path gets there, else onto the part unless `plate_only`.
/// When none of those reach the plate, a side approach at 50 to 80 degrees
/// is tried, to the plate only: one contact on the part instead of two
/// (Sleptic, R85: a Benchy's strip in front of its cabin got a column on the
/// deck, when a rod over the bow rail reaches it from the plate). Each of
/// `diameters` is tried in turn.
#[allow(clippy::too_many_arguments)]
fn lean_route(
    solid: &crate::hollow::Tree,
    heights: &HeightTree,
    tris: &[[Vertex; 3]],
    contact: [f64; 3],
    diameters: &[f64],
    settings: &SupportSettings,
    floor: f64,
    plate: Option<[f64; 2]>,
    plate_only: bool,
) -> Option<Lean> {
    use glam::DVec3;
    let p = DVec3::from_array(contact);
    let tip_length = settings.tip_length_mm;
    let limit = settings.branch_angle_deg.min(40.0).min(MAX_LEAN_DEG);
    // A leaning rod bends where a pillar only carries: it may be thicker
    // than the thickest pillar (R86, when pillars were capped at 2 mm).
    let (min_d, max_d) = (
        settings.min_column_diameter_mm,
        settings.max_column_diameter_mm.max(LEAN_MAX_DIAMETER_MM),
    );
    // A column down to the plate is at least 1/LEAN_MAX_SLENDERNESS of its
    // length thick (lean and drop), or it sways and snaps under the peel: a
    // 0.8 mm column 47 mm long reached past a part to the plate (R83).
    let needed = |path: f64| (path / LEAN_MAX_SLENDERNESS).clamp(min_d, max_d);
    // One direction: the first knee from which the column drops clear, if
    // the lean gets there clear. Returns (cost, lean).
    let walk = |theta: f64, phi: f64, diameter: f64, plate_only: bool| -> Option<(f64, Lean)> {
        let radius = diameter / 2.0;
        let (s, c) = theta.to_radians().sin_cos();
        let dir = DVec3::new(s * phi.cos(), s * phi.sin(), -c);
        let top = p + dir * tip_length;
        let clear_from = tip_length.max((radius + COLUMN_GAP_MM) / c + 0.05);
        // The checks are balls along the axis. Near the contact a ball wider
        // than the depth below the surface being held would hit that surface
        // itself, so there each ball grows with the depth below the contact;
        // nothing on the way is left unchecked (a steep side approach runs
        // millimetres before the full width fits).
        let mut d = tip_length * 0.5;
        while d < clear_from {
            let ball = (0.9 * d * c).min(radius + COLUMN_GAP_MM);
            if solid.near(p + dir * d, ball) {
                return None;
            }
            d += 0.1;
        }
        let mut length = clear_from + 0.5;
        while length <= 25.0 {
            let knee = p + dir * length;
            // A foot past the plate's edge would be cut off by the screen.
            let off_plate = plate.is_some_and(|half| {
                knee.x.abs() + radius + 0.1 > half[0] || knee.y.abs() + radius + 0.1 > half[1]
            });
            if off_plate {
                return None;
            }
            if knee.z < floor + settings.base_height_mm + 0.5
                || !bar_clear(
                    solid,
                    (p + dir * clear_from).to_array(),
                    knee.to_array(),
                    radius + COLUMN_GAP_MM,
                )
            {
                return None;
            }
            let mut lower: Option<f64> = None;
            heights.visit(tris, knee.x, knee.y, &mut |_, h| {
                if h > floor + 1e-5 && h < knee.z {
                    lower = Some(lower.map_or(h, |m: f64| m.max(h)));
                }
            });
            let bottom = lower.map_or(floor + settings.base_height_mm, |h| h + tip_length);
            let too_thin = lower.is_none() && diameter + 1e-9 < needed(length + knee.z - floor);
            if !(plate_only && lower.is_some())
                && !too_thin
                && knee.z > bottom + 0.1
                && bar_clear(
                    solid,
                    [knee.x, knee.y, bottom],
                    knee.to_array(),
                    radius + COLUMN_GAP_MM,
                )
            {
                let cost = length
                    + (knee.z - lower.unwrap_or(floor))
                    + if lower.is_some() { 1000.0 } else { 0.0 };
                let lean = Lean {
                    knee: knee.to_array(),
                    top: top.to_array(),
                    lower,
                    diameter,
                    tip_length,
                };
                return Some((cost, lean));
            }
            length += 0.5;
        }
        None
    };
    let best_of = |thetas: &[f64], diameter: f64, plate_only: bool| -> Option<(f64, Lean)> {
        let mut best: Option<(f64, Lean)> = None;
        for &theta in thetas {
            for k in 0..16 {
                let phi = f64::from(k) * std::f64::consts::TAU / 16.0;
                if let Some((cost, lean)) = walk(theta, phi, diameter, plate_only)
                    && best.as_ref().is_none_or(|(b, _)| cost < *b)
                {
                    best = Some((cost, lean));
                }
            }
        }
        best
    };
    // Thicker first (a plate column as thick as its height wants), then the
    // diameters asked for; thinner ones may still stand on the part.
    let mut sizes = vec![needed(p.z - floor).max(diameters[0])];
    for &d in diameters {
        if !sizes.contains(&d) {
            sizes.push(d);
        }
    }
    let within_branch_angle: Vec<f64> = [10.0f64, 20.0, 30.0, 40.0]
        .into_iter()
        .filter(|t| *t <= limit + 1e-9)
        .collect();
    let mut on_part = None;
    for &diameter in &sizes {
        if let Some((_, lean)) = best_of(&within_branch_angle, diameter, plate_only) {
            if lean.lower.is_none() {
                return Some(lean);
            }
            on_part = Some(lean);
            break;
        }
    }
    // Side approach: only to the plate, thickest that fits first.
    // No side approach past the branch angle: a rod leaning 50 to 80
    // degrees (R85) was itself an overhang (R91). The contact stands on the
    // part instead, or the island pass holds it another way.
    on_part
}

/// The closed mesh of a leaning column (five rings: foot, foot top, knee,
/// tip start, contact).
fn lean_column(
    lean: &Lean,
    contact: [f64; 3],
    solid: &crate::hollow::Tree,
    settings: &SupportSettings,
    floor: f64,
) -> Result<IndexedMesh, SupportError> {
    let radius = lean.diameter / 2.0;
    let tip_radius = settings.tip_contact_diameter_mm / 2.0;
    let [kx, ky, kz] = lean.knee;
    let foot = match lean.lower {
        Some(h) => [
            (h + 0.02 - settings.contact_depth_mm, tip_radius),
            (h + lean.tip_length, tip_end_radius(settings, lean.tip_length, radius)),
        ],
        None => {
            let height = settings.base_height_mm.min((kz - floor) / 3.0);
            let mut base = settings.base_diameter_mm / 2.0;
            while base > radius + 1e-9
                && !bar_clear(
                    solid,
                    [kx, ky, floor + 0.01],
                    [kx, ky, floor + height],
                    base + COLUMN_GAP_MM,
                )
            {
                base = (base * 0.8).max(radius);
            }
            [(floor, base), (floor + height, radius)]
        }
    };
    let rings = [
        (kx, ky, foot[0].0, foot[0].1),
        (kx, ky, foot[1].0, radius),
        (kx, ky, kz, radius),
        (
            lean.top[0],
            lean.top[1],
            lean.top[2],
            tip_end_radius(settings, lean.tip_length, radius),
        ),
        (
            contact[0],
            contact[1],
            contact[2] + settings.contact_depth_mm,
            tip_radius,
        ),
    ];
    let vertices = rings
        .iter()
        .flat_map(|&(cx, cy, cz, r)| {
            (0..ROUND_SIDES).map(move |i| {
                let a = f64::from(i) * std::f64::consts::TAU / f64::from(ROUND_SIDES);
                Vertex::new([
                    (cx + r * a.cos()) as f32,
                    (cy + r * a.sin()) as f32,
                    cz as f32,
                ])
            })
        })
        .collect();
    // A ball at the bend (R84): the knee is where a leaning column breaks.
    let parts = [
        closed_rings(vertices, ROUND_SIDES)?,
        ball(lean.knee, radius * 1.15)?,
    ];
    merge_meshes(parts.iter()).ok_or(SupportError::UnrepresentableGeometry)
}

/// A closed ball (stacked rings with centre caps) at `centre`.
fn ball(centre: [f64; 3], radius: f64) -> Result<IndexedMesh, SupportError> {
    let sides = sides_for(radius);
    let rings = (sides / 3).max(3);
    let vertices = (0..rings)
        .flat_map(|k| {
            let lat = -std::f64::consts::FRAC_PI_2
                + std::f64::consts::PI * f64::from(k + 1) / f64::from(rings + 1);
            let (z, r) = (centre[2] + radius * lat.sin(), radius * lat.cos());
            (0..sides).map(move |i| {
                let a = f64::from(i) * std::f64::consts::TAU / f64::from(sides);
                Vertex::new([
                    (centre[0] + r * a.cos()) as f32,
                    (centre[1] + r * a.sin()) as f32,
                    z as f32,
                ])
            })
        })
        .collect();
    closed_rings_centre_capped(vertices, sides)
}

/// Where the columns stand on the raft (each plate-standing column's foot
/// centre): a raft is laid under the feet, and a leaning column's foot is
/// away from its contact (R84; rafts followed the contacts before).
pub(crate) fn raft_feet(columns: &[IndexedMesh], floor: f64) -> Vec<SupportPoint> {
    columns
        .iter()
        .flat_map(|column| {
            let low = column
                .vertices
                .iter()
                .map(|v| v[2])
                .fold(f32::INFINITY, f32::min);
            if !low.is_finite() || f64::from(low) > floor + 1e-3 {
                return Vec::new();
            }
            // A column may stand on more than one foot (a lone tall pillar
            // and its companion, R87): each cluster of foot vertices is a
            // foot of its own.
            // A foot is the foot vertices joined by the mesh's own edges, so
            // feet are told apart however wide or close they are. Grouping by
            // distance merged a companion's foot with its pillar's, or split
            // a wide foot into single vertices (R89).
            let on_floor: Vec<bool> = column.vertices.iter().map(|v| v[2] <= low + 1e-4).collect();
            let mut parent: Vec<usize> = (0..column.vertices.len()).collect();
            fn root(parent: &mut [usize], mut i: usize) -> usize {
                while parent[i] != i {
                    parent[i] = parent[parent[i]];
                    i = parent[i];
                }
                i
            }
            for face in &column.faces {
                let ids = face.vertices.map(usize::from);
                for (a, b) in [(ids[0], ids[1]), (ids[1], ids[2]), (ids[2], ids[0])] {
                    if on_floor[a] && on_floor[b] {
                        let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                        parent[ra] = rb;
                    }
                }
            }
            let mut by_root: std::collections::BTreeMap<usize, Vec<[f32; 3]>> =
                std::collections::BTreeMap::new();
            for (i, v) in column.vertices.iter().enumerate() {
                if on_floor[i] {
                    let r = root(&mut parent, i);
                    by_root.entry(r).or_default().push(v.coords());
                }
            }
            let feet: Vec<Vec<[f32; 3]>> = by_root.into_values().collect();
            feet.into_iter()
                .map(|ring| {
                    let n = ring.len() as f32;
                    SupportPoint {
                        position: [
                            ring.iter().map(|v| v[0]).sum::<f32>() / n,
                            ring.iter().map(|v| v[1]).sum::<f32>() / n,
                            low,
                        ],
                        normal: [0.0, 0.0, -1.0],
                    }
                })
                .collect()
        })
        .collect()
}

/// A skate raft (R84): a square pad under each column's foot,
/// pads closer than about three pad widths joined by strips, holes the
/// strips enclose filled. Less resin than a full raft where columns are few.
fn skate_raft(
    feet: &[SupportPoint],
    settings: &SupportSettings,
    plate: Option<[f64; 2]>,
) -> Result<IndexedMesh, SupportError> {
    let pitch = 0.5;
    let half = settings.base_diameter_mm / 2.0 + 0.5;
    let link = half * 3.2;
    let xy: Vec<[f64; 2]> = feet
        .iter()
        .map(|p| [f64::from(p.position[0]), f64::from(p.position[1])])
        .collect();
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in &xy {
        for axis in 0..2 {
            lo[axis] = lo[axis].min(p[axis] - half - pitch);
            hi[axis] = hi[axis].max(p[axis] + half + pitch);
        }
    }
    if let Some(edge) = plate {
        for axis in 0..2 {
            lo[axis] = lo[axis].max(-edge[axis]);
            hi[axis] = hi[axis].min(edge[axis]);
        }
    }
    if !(lo[0] < hi[0] && lo[1] < hi[1]) {
        return Err(SupportError::UnrepresentableGeometry);
    }
    let nx = ((hi[0] - lo[0]) / pitch).ceil() as usize;
    let ny = ((hi[1] - lo[1]) / pitch).ceil() as usize;
    if nx.saturating_mul(ny) > 4_000_000 {
        return Err(SupportError::SamplingLimit);
    }
    let mut filled = vec![false; nx * ny];
    let mut stamp = |c: [f64; 2], h: f64| {
        let cell = |v: f64, axis: usize| ((v - lo[axis]) / pitch).floor();
        let (x0, x1) = (
            cell(c[0] - h, 0).max(0.0),
            cell(c[0] + h, 0).min(nx as f64 - 1.0),
        );
        let (y0, y1) = (
            cell(c[1] - h, 1).max(0.0),
            cell(c[1] + h, 1).min(ny as f64 - 1.0),
        );
        if x0 > x1 || y0 > y1 {
            return;
        }
        for y in y0 as usize..=y1 as usize {
            for x in x0 as usize..=x1 as usize {
                filled[y * nx + x] = true;
            }
        }
    };
    for p in &xy {
        stamp(*p, half);
    }
    for (i, a) in xy.iter().enumerate() {
        for b in &xy[i + 1..] {
            let d = (b[0] - a[0]).hypot(b[1] - a[1]);
            if d < 0.2 || d > link {
                continue;
            }
            let steps = (d / (pitch * 0.5)).ceil().max(1.0) as usize;
            for s in 0..=steps {
                let t = s as f64 / steps as f64;
                stamp(
                    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
                    half * 0.5,
                );
            }
        }
    }
    // Holes the strips enclose are filled: a ring of strips around an
    // empty pocket would trap resin under the raft.
    let mut outside = vec![false; nx * ny];
    let mut stack: Vec<usize> = (0..nx * ny)
        .filter(|&k| {
            let (x, y) = (k % nx, k / nx);
            (x == 0 || y == 0 || x + 1 == nx || y + 1 == ny) && !filled[k]
        })
        .collect();
    while let Some(k) = stack.pop() {
        if outside[k] || filled[k] {
            continue;
        }
        outside[k] = true;
        let (x, y) = (k % nx, k / nx);
        if x > 0 {
            stack.push(k - 1);
        }
        if x + 1 < nx {
            stack.push(k + 1);
        }
        if y > 0 {
            stack.push(k - nx);
        }
        if y + 1 < ny {
            stack.push(k + nx);
        }
    }
    for k in 0..nx * ny {
        filled[k] = filled[k] || !outside[k];
    }
    let filled = close_corner_touches(filled, nx, ny, |_, _| true);
    extrude_cells(lo, hi, pitch, nx, ny, &filled, settings.raft_thickness_mm)
}

/// Whether a round bar of `radius` from `a` to `b` stays clear of the model:
/// points along it are checked at under half a radius apart.
fn bar_clear(solid: &crate::hollow::Tree, a: [f64; 3], b: [f64; 3], radius: f64) -> bool {
    let (a, b) = (glam::DVec3::from_array(a), glam::DVec3::from_array(b));
    let length = (b - a).length();
    if !length.is_finite() || length <= 1e-9 {
        return true;
    }
    let steps = (length / (radius * 0.45).max(0.05)).ceil().max(1.0) as usize;
    (0..=steps).all(|i| !solid.near(a + (b - a) * (i as f64 / steps as f64), radius))
}

fn recompute_mesh(
    vertices: Vec<Vertex>,
    indices: Vec<[u32; 3]>,
) -> Result<IndexedMesh, SupportError> {
    let faces = indices
        .into_iter()
        .map(|ids| {
            let normal = compute_normal_from_triangle_vertices(&ids.map(|i| vertices[i as usize]))
                .map_err(|_| SupportError::UnrepresentableGeometry)?;
            Ok(IndexedTriangle::from_raw_indices(normal, ids))
        })
        .collect::<Result<_, SupportError>>()?;
    Ok(IndexedMesh::new(None, vertices, faces))
}

/// A brush dab stored in the model's local coordinates. Avoid always wins
/// over keep where brushes overlap. Comments survive clipboard metadata copies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintedRegion {
    /// Brush centre in model coordinates.
    pub centre: [f32; 3],
    /// Brush radius in millimetres.
    pub radius: f32,
    /// True forces contacts; false excludes contacts.
    pub keep: bool,
}

/// Parse only finite, bounded support brush records, preserving other metadata.
pub fn painted_regions(comment: &str) -> Vec<PaintedRegion> {
    comment
        .lines()
        .filter_map(|line| {
            let data = line.strip_prefix("sc-support-paint ")?;
            let values = data
                .split_whitespace()
                .map(str::parse::<f32>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            if values.len() != 5
                || values.iter().any(|v| !v.is_finite())
                || values[3] <= 0.0
                || values[3] > 100.0
            {
                return None;
            }
            Some(PaintedRegion {
                centre: [values[0], values[1], values[2]],
                radius: values[3],
                keep: values[4] > 0.0,
            })
        })
        .take(10000)
        .collect()
}

fn apply_painted_regions(
    scene: &sc_scene::Scene,
    settings: &SupportSettings,
    points: &mut Vec<SupportPoint>,
) -> Result<(), SupportError> {
    let mut regions = Vec::new();
    scene
        .for_each_expanded_leaf::<sc_scene::SceneError, _>(
            sc_scene::CsgPolicy::Descend,
            |id, transform, _| {
                if let Some(comment) = scene
                    .node(id)
                    .and_then(|n| n.meta.as_ref())
                    .and_then(|m| m.comment.as_deref())
                {
                    let painted = painted_regions(comment);
                    if painted.is_empty() {
                        return Ok(());
                    }
                    // Brush records do not force an automatic lift/raft on a
                    // model already certified for direct printing. Explicit
                    // manual contact placement remains a separate action.
                    let model = sc_scene::bake_leaf_to_world(scene, id)?;
                    if crate::orient::prints_without_supports(&model, settings.overhang_angle_deg) {
                        return Ok(());
                    }
                    for mut region in painted {
                        let centre = transform.apply_to_vertex(Vertex::new(region.centre));
                        let edge = transform.apply_to_vertex(Vertex::new([
                            region.centre[0] + region.radius,
                            region.centre[1],
                            region.centre[2],
                        ]));
                        region.radius = centre
                            .coords()
                            .iter()
                            .zip(edge.coords())
                            .map(|(a, b)| (a - b).powi(2))
                            .sum::<f32>()
                            .sqrt();
                        region.centre = centre.coords();
                        regions.push((id, region));
                    }
                }
                Ok(())
            },
        )
        .map_err(SupportError::Scene)?;
    if regions.is_empty() {
        return Ok(());
    }
    for (id, r) in regions.iter().filter(|(_, r)| r.keep) {
        let model = sc_scene::bake_leaf_to_world(scene, *id).map_err(SupportError::Scene)?;
        let tris = triangles(&model)?;
        // Project dense brush samples onto the nearby downward surface.
        for ix in -2..=2 {
            for iy in -2..=2 {
                let x = f64::from(r.centre[0] + ix as f32 * r.radius / 2.0);
                let y = f64::from(r.centre[1] + iy as f32 * r.radius / 2.0);
                let candidate = tris
                    .iter()
                    .filter_map(|t| {
                        let normal = compute_normal_from_triangle_vertices(t).ok()?;
                        if normal[2] >= 0.0 {
                            return None;
                        }
                        let z = height_at(t, x, y)?;
                        let p = SupportPoint {
                            position: [x as f32, y as f32, z as f32],
                            normal: normal.coords(),
                        };
                        within_region(r, p.position).then_some(p)
                    })
                    .min_by(|a, b| {
                        (a.position[2] - r.centre[2])
                            .abs()
                            .total_cmp(&(b.position[2] - r.centre[2]).abs())
                    });
                if let Some(p) = candidate {
                    add_spaced(points, p, f64::from(r.radius) / 2.0);
                }
            }
        }
    }
    let owners = support_owners(scene, points);
    let mut index = 0;
    points.retain(|p| {
        let owner = owners[index];
        index += 1;
        !regions
            .iter()
            .any(|(id, r)| owner == Some(*id) && !r.keep && within_region(r, p.position))
    });
    Ok(())
}

fn within_region(r: &PaintedRegion, p: [f32; 3]) -> bool {
    p.iter()
        .zip(r.centre)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f32>()
        <= r.radius * r.radius + 1e-5
}

// Connected downward-facing triangle patches define the islands whose projected
// area is shared by their accepted contacts. Welding coordinates also handles
// STL inputs that repeat vertices for every triangle.
fn load_vertex_key(v: Vertex) -> [u32; 3] {
    // Signed zeros describe the same vertex; every other bit stays distinct.
    v.coords().map(|x| if x == 0.0 { 0 } else { x.to_bits() })
}

fn contact_island_loads(
    tris: &[[Vertex; 3]],
    heights: &HeightTree,
    contacts: &[SupportPoint],
    connect_all: bool,
) -> Vec<(usize, f64)> {
    use std::collections::BTreeMap;
    let mut parent: Vec<_> = (0..tris.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    // This table is only queried, never iterated: hashing preserves the first
    // face for each exact vertex without paying for ordered insertion.
    let mut vertices = std::collections::HashMap::new();
    let mut areas = vec![0.0; tris.len()];
    for (i, t) in tris.iter().enumerate() {
        let [a, b, c] = t.map(|v| v.coords().map(f64::from));
        let signed = ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) / 2.0;
        if signed >= -1e-10 && !connect_all {
            continue;
        }
        areas[i] = (-signed).max(0.0);
        for v in t {
            let key = load_vertex_key(*v);
            if let Some(&old) = vertices.get(&key) {
                let a = root(&mut parent, i);
                let b = root(&mut parent, old);
                parent[a] = b;
            } else {
                vertices.insert(key, i);
            }
        }
    }
    let mut totals = BTreeMap::<usize, f64>::new();
    for (i, area) in areas.iter().enumerate() {
        if *area > 0.0 {
            let id = root(&mut parent, i);
            *totals.entry(id).or_default() += area;
        }
    }
    let ids: Vec<_> = contacts
        .iter()
        .enumerate()
        .map(|(fallback, p)| {
            // Nearest downward face on the contact's vertical line; ties go
            // to the lower triangle index, as the linear scan picked.
            let mut best: Option<(usize, f64)> = None;
            heights.visit(
                tris,
                f64::from(p.position[0]),
                f64::from(p.position[1]),
                &mut |i, h| {
                    if areas[i] > 0.0 {
                        let d = (h - f64::from(p.position[2])).abs();
                        if best.is_none_or(|(bi, bd)| d < bd || (d == bd && i < bi)) {
                            best = Some((i, d));
                        }
                    }
                },
            );
            best.map_or(tris.len() + fallback, |(i, _)| root(&mut parent, i))
        })
        .collect();
    let mut counts = BTreeMap::<usize, usize>::new();
    for &id in &ids {
        *counts.entry(id).or_default() += 1;
    }
    ids.into_iter()
        .map(|id| {
            (
                id,
                totals.get(&id).copied().unwrap_or(0.0) / counts[&id] as f64,
            )
        })
        .collect()
}

/// Straight-drop obstruction and lateral removal access for orientation scoring.
/// Four horizontal rays at 0.25 mm below each contact model an 8 mm tool
/// approach. All four blocked means a recessed contact. This is a point-tool
/// estimate; it does not promise swept shaft or plier clearance.
#[cfg(test)]
pub(crate) fn orientation_access(
    mesh: &IndexedMesh,
    points: &[SupportPoint],
) -> (usize, usize, Vec<bool>) {
    let result = orientation_access_indexed(
        &crate::hollow::Tree::from_mesh(mesh),
        points,
        glam::DQuat::IDENTITY,
        glam::DVec3::ZERO,
        1e-4,
    );
    (result.0, result.1, result.2)
}
/// Transform queries back into the original mesh, avoiding per-pose indexes.
pub(crate) fn orientation_access_indexed(
    tree: &crate::hollow::Tree,
    points: &[SupportPoint],
    rotation: glam::DQuat,
    shift: glam::DVec3,
    surface_tolerance: f64,
) -> (usize, usize, Vec<bool>, usize) {
    use glam::DVec3;
    let inverse = rotation.inverse();
    let mut blocked = Vec::with_capacity(points.len());
    let mut recessed = 0;
    let mut plate_recessed = 0;
    for point in points {
        let p = DVec3::from_array(point.position.map(f64::from));
        let origin = inverse * (p - shift);
        blocked.push(tree.ray_any(origin, inverse * -DVec3::Z, surface_tolerance, p.z - 1e-5));
        let approach = inverse * (p - shift - DVec3::Z * 0.25);
        if [DVec3::X, -DVec3::X, DVec3::Y, -DVec3::Y]
            .into_iter()
            .all(|direction| tree.ray_any(approach, inverse * direction, 0.05, 8.0))
        {
            recessed += 1;
            if !blocked.last().copied().unwrap_or(false) {
                plate_recessed += 1;
            }
        }
    }
    (
        blocked.iter().filter(|b| **b).count(),
        recessed,
        blocked,
        plate_recessed,
    )
}

/// Classify generated contacts against the lifted model, excluding support
/// geometry. `surface_tolerance` is the generator's layer-height allowance for
/// raster contacts, so a contact rounded above its own face is not a shadow.
/// Returns blocked straight drops and recessed removal locations, respectively.
pub fn analyze_contact_access(
    mesh: &IndexedMesh,
    points: &[SupportPoint],
    surface_tolerance: f64,
) -> Result<(usize, usize), SupportError> {
    if !surface_tolerance.is_finite() || surface_tolerance < 0.0 {
        return Err(SupportError::InvalidContact);
    }
    triangles(mesh)?;
    let result = orientation_access_indexed(
        &crate::hollow::Tree::from_mesh(mesh),
        points,
        glam::DQuat::IDENTITY,
        glam::DVec3::ZERO,
        surface_tolerance,
    );
    Ok((result.0, result.1))
}

/// One shaft segment in the support generator's ten-sided ring geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BranchPath {
    pub(crate) length_mm: f64,
    pub(crate) rise_mm: f64,
    pub(crate) inclination_deg: f64,
}
/// The branches of the generated trees.
pub(crate) fn branch_paths(structure: &SupportStructure) -> Vec<BranchPath> {
    structure.paths.clone()
}
/// Projected contact area of the generated mesh's lowest horizontal caps.
pub(crate) fn base_contact_area(mesh: &IndexedMesh) -> f64 {
    let floor = mesh
        .vertices
        .iter()
        .map(|v| f64::from(v[2]))
        .fold(f64::INFINITY, f64::min);
    let bottom: Vec<[glam::DVec3; 3]> = mesh
        .faces
        .iter()
        .filter_map(|f| {
            let t = f.vertices.map(|i| {
                glam::DVec3::from_array(mesh.vertices[usize::from(i)].coords().map(f64::from))
            });
            t.iter().all(|p| (p.z - floor).abs() < 1e-5).then_some(t)
        })
        .collect();
    // One welded slab: its bottom faces do not overlap, sum them exactly.
    // A raft of overlapping pieces (the spider web, R105) would count each
    // overlap twice: measure the covered area on a fine grid instead.
    if mesh_pieces(mesh) <= 1 {
        return bottom
            .iter()
            .map(|t| (t[1] - t[0]).cross(t[2] - t[0]).z.abs() * 0.5)
            .sum();
    }
    const CELL: f64 = 0.05;
    let (mut lo, mut hi) = (glam::DVec2::splat(f64::INFINITY), glam::DVec2::splat(f64::NEG_INFINITY));
    for t in &bottom {
        for p in t {
            lo = lo.min(p.truncate());
            hi = hi.max(p.truncate());
        }
    }
    if bottom.is_empty() {
        return 0.0;
    }
    let nx = ((hi.x - lo.x) / CELL).ceil() as usize + 1;
    let ny = ((hi.y - lo.y) / CELL).ceil() as usize + 1;
    let mut covered = vec![false; nx * ny];
    for t in &bottom {
        let (a, b, c) = (t[0].truncate(), t[1].truncate(), t[2].truncate());
        let (tl, th) = (a.min(b).min(c), a.max(b).max(c));
        let ix0 = ((tl.x - lo.x) / CELL).floor().max(0.0) as usize;
        let iy0 = ((tl.y - lo.y) / CELL).floor().max(0.0) as usize;
        let ix1 = (((th.x - lo.x) / CELL).ceil() as usize).min(nx - 1);
        let iy1 = (((th.y - lo.y) / CELL).ceil() as usize).min(ny - 1);
        let side = |u: glam::DVec2, v: glam::DVec2, p: glam::DVec2| (v - u).perp_dot(p - u);
        for iy in iy0..=iy1 {
            for ix in ix0..=ix1 {
                let p = lo + glam::DVec2::new((ix as f64 + 0.5) * CELL, (iy as f64 + 0.5) * CELL);
                let (s1, s2, s3) = (side(a, b, p), side(b, c, p), side(c, a, p));
                if (s1 >= 0.0 && s2 >= 0.0 && s3 >= 0.0) || (s1 <= 0.0 && s2 <= 0.0 && s3 <= 0.0) {
                    covered[iy * nx + ix] = true;
                }
            }
        }
    }
    covered.iter().filter(|c| **c).count() as f64 * CELL * CELL
}

/// The number of separate pieces in a mesh (vertices joined by faces).
fn mesh_pieces(mesh: &IndexedMesh) -> usize {
    let mut parent: Vec<usize> = (0..mesh.vertices.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for f in &mesh.faces {
        let [a, b, c] = f.vertices.map(usize::from);
        for (x, y) in [(a, b), (b, c)] {
            let (rx, ry) = (root(&mut parent, x), root(&mut parent, y));
            parent[rx] = ry;
        }
    }
    let used: std::collections::BTreeSet<usize> = mesh
        .faces
        .iter()
        .flat_map(|f| f.vertices.map(usize::from))
        .collect();
    let roots: std::collections::BTreeSet<usize> = used.into_iter().map(|v| root(&mut parent, v)).collect();
    roots.len()
}

#[cfg(test)]
mod over_support_tests {
    use super::*;

    /// A downward-facing height field `h(i, j)` on a unit grid (`n` x `n`
    /// vertices), the underside of something: faces wind clockwise from
    /// above so their normals point down.
    fn underside(n: usize, h: impl Fn(usize, usize) -> f32) -> IndexedMesh {
        let mut vertices = Vec::new();
        for j in 0..n {
            for i in 0..n {
                vertices.push(Vertex::new([i as f32, j as f32, h(i, j)]));
            }
        }
        let at = |i: usize, j: usize| (j * n + i) as u32;
        let mut faces = Vec::new();
        for j in 0..n - 1 {
            for i in 0..n - 1 {
                for tri in [
                    [at(i, j), at(i + 1, j + 1), at(i + 1, j)],
                    [at(i, j), at(i, j + 1), at(i + 1, j + 1)],
                ] {
                    faces.push(IndexedTriangle::from_raw_indices(
                        sc_mesh_core::Normal::new([0.0, 0.0, -1.0]),
                        tri,
                    ));
                }
            }
        }
        IndexedMesh::new(None, vertices, faces)
    }

    /// Sleptic: auto supports "overcompensate on super small overhangs". A
    /// 0.02 mm tessellation dip is no island; a 0.5 mm one and the lowest
    /// point are.
    #[test]
    fn shallow_dips_do_not_count_as_islands() {
        let mesh = underside(11, |i, j| match (i, j) {
            (2, 2) => 9.0,
            (7, 2) => 9.5,
            (7, 7) => 9.98,
            _ => 10.0,
        });
        let p = minimum_persistence(&mesh);
        let at = |i: usize, j: usize| p[j * 11 + i];
        assert!(
            at(2, 2).is_infinite(),
            "the lowest point is always an island"
        );
        assert!((at(7, 2) - 0.5).abs() < 1e-4, "{}", at(7, 2));
        assert!(at(7, 7) < MIN_ISLAND_HEIGHT_MM, "{}", at(7, 7));
        // Every other vertex starts no patch of its own.
        assert_eq!(p.iter().filter(|v| **v > 0.0).count(), 3);
    }

    /// Sleptic: six identical arranged parts got different supports. The
    /// overhang grid moves with the part, so a copy at any offset samples
    /// the same contacts, shifted by that offset.
    #[test]
    fn a_moved_copy_gets_the_same_contacts() {
        let settings = SupportSettings::default();
        let base = underside(13, |i, j| 8.0 + 0.002 * ((i * 7 + j * 3) % 5) as f32);
        let contacts = |dx: f32, dy: f32| {
            let mut mesh = base.clone();
            for v in &mut mesh.vertices {
                *v = Vertex::new([v[0] + dx, v[1] + dy, v[2]]);
            }
            let mut points: Vec<[f32; 3]> = sample_contacts_inner(&mesh, &settings, None, None)
                .unwrap()
                .iter()
                .map(|p| [p.position[0] - dx, p.position[1] - dy, p.position[2]])
                .collect();
            points.sort_by(|a, b| {
                a.iter()
                    .zip(b)
                    .fold(std::cmp::Ordering::Equal, |o, (x, y)| {
                        o.then(x.total_cmp(y))
                    })
            });
            points
        };
        let here = contacts(0.0, 0.0);
        assert!(here.len() > 4, "{}", here.len());
        for (dx, dy) in [(1.37, 0.0), (23.9, -41.3), (-6.05, 2.75)] {
            let there = contacts(dx, dy);
            assert_eq!(here.len(), there.len(), "offset {dx}, {dy}");
            for (a, b) in here.iter().zip(&there) {
                assert!((0..3).all(|k| (a[k] - b[k]).abs() < 1e-3), "{a:?} vs {b:?}");
            }
        }
    }

    /// A 0.5 mm wide strip prints without supports; a 5 mm wide one does
    /// not count as narrow.
    #[test]
    fn narrow_overhang_strips_are_left_unsupported() {
        let strip = |width: f32, x0: f32| -> (Vec<Vertex>, [[u32; 3]; 2]) {
            (
                vec![
                    Vertex::new([x0, 0.0, 5.0]),
                    Vertex::new([x0 + width, 0.0, 5.0]),
                    Vertex::new([x0 + width, 10.0, 5.0]),
                    Vertex::new([x0, 10.0, 5.0]),
                ],
                [[0, 2, 1], [0, 3, 2]],
            )
        };
        let mut vertices = Vec::new();
        let mut faces = Vec::new();
        for (width, x0) in [(0.5, 0.0), (5.0, 3.0)] {
            let (v, f) = strip(width, x0);
            let base = vertices.len() as u32;
            vertices.extend(v);
            for t in f {
                faces.push(IndexedTriangle::from_raw_indices(
                    sc_mesh_core::Normal::new([0.0, 0.0, -1.0]),
                    t.map(|i| i + base),
                ));
            }
        }
        let mesh = IndexedMesh::new(None, vertices, faces);
        let narrow = narrow_overhang_faces(&mesh, 45f64.to_radians().cos());
        assert_eq!(narrow, vec![true, true, false, false]);
    }
    #[test]
    fn load_vertex_keys_keep_exact_welding_and_first_face() {
        let mut hashed = std::collections::HashMap::new();
        let mut ordered = std::collections::BTreeMap::new();
        let samples = [
            [0.0, 1.0, 2.0],
            [-0.0, 1.0, 2.0],
            [f32::from_bits(1), 1.0, 2.0],
            [0.0, 1.0, 2.0],
            [0.0, f32::from_bits(1.0f32.to_bits() + 1), 2.0],
        ];
        let mut first = Vec::new();
        for (face, coords) in samples.into_iter().enumerate() {
            let key = load_vertex_key(Vertex::new(coords));
            let old_key = coords.map(|x| if x == 0.0 { 0 } else { x.to_bits() });
            assert_eq!(key, old_key);
            let a = *hashed.entry(key).or_insert(face);
            let b = *ordered.entry(old_key).or_insert(face);
            assert_eq!(a, b);
            first.push(a);
        }
        assert_eq!(first, vec![0, 0, 2, 0, 4]);
    }
}
