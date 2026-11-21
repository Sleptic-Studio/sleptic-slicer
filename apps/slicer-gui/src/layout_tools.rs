// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Undoable placement tools; all calculations use baked world coordinates.
use crate::build_volume::BuildVolume;
use glam::DVec3;
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_scene::{Scene, node::NodeId};
use sc_scene_edit::SceneEdit;

/// A micron of slack on every "does it fit" check: parts are baked to world
/// in f32, so a part exactly as deep as the plate measured a few microns
/// over and arrange refused it (Sleptic's motor mount, turned by Auto prepare
/// to the plate's full 120 mm). Auto orient's copy count has the same slack.
const FIT_SLACK_MM: f64 = 1e-3;

fn points(scene: &Scene, id: NodeId) -> Option<Vec<DVec3>> {
    let mesh = sc_scene::bake_leaf_to_world(scene, id).ok()?;
    Some(
        mesh.vertices
            .iter()
            .map(|v| DVec3::from_array(v.coords().map(f64::from)))
            .collect(),
    )
}

fn bounds(points: &[DVec3]) -> Option<(DVec3, DVec3)> {
    let first = *points.first()?;
    Some(
        points
            .iter()
            .fold((first, first), |(min, max), p| (min.min(*p), max.max(*p))),
    )
}

/// Explain a direct-bed orientation, including actual plate duration.
pub(crate) fn direct_orient_summary(minutes: f64, copies: usize) -> String {
    crate::i18n::tr!(
        "Oriented: prints flat on the plate with no supports; {}; fits {} per plate.",
        sc_print_job::format_duration(minutes * 60.0),
        copies
    )
    .to_string()
}

/// Status line after auto-orient, in plain words: how the print time changed,
/// the change in estimated supports, any hard-to-reach supports, and copies per
/// plate. Small time changes read as "same print time"; zero awkward contacts
/// before and after are not mentioned at all.
pub(crate) fn orient_summary(
    extra_minutes: f64,
    fewer_supports: f64,
    awkward_before: f64,
    awkward_after: f64,
    copies: usize,
) -> String {
    use crate::i18n::tr;
    let time = if extra_minutes.abs() < 1.0 {
        tr!("same print time").to_string()
    } else if extra_minutes > 0.0 {
        tr!(
            "{} longer print",
            sc_print_job::format_duration(extra_minutes * 60.0)
        )
        .to_string()
    } else {
        tr!(
            "{} shorter print",
            sc_print_job::format_duration(-extra_minutes * 60.0)
        )
        .to_string()
    };
    let n = fewer_supports.abs().round() as u64;
    let supports = match (fewer_supports >= 0.0, n) {
        (_, 0) => tr!("same number of supports").to_string(),
        (true, 1) => tr!("1 fewer support").to_string(),
        (true, _) => tr!("{} fewer supports", n).to_string(),
        (false, 1) => tr!("1 more support").to_string(),
        (false, _) => tr!("{} more supports", n).to_string(),
    };
    let removed = (awkward_before - awkward_after).round() as i64;
    let left = awkward_after.round() as u64;
    let access = if removed >= 1 {
        Some(if removed == 1 {
            tr!("1 fewer hard-to-reach support").to_string()
        } else {
            tr!("{} fewer hard-to-reach supports", removed).to_string()
        })
    } else if left == 1 {
        Some(tr!("1 hard-to-reach support").to_string())
    } else if left > 1 {
        Some(tr!("{} hard-to-reach supports", left).to_string())
    } else {
        None
    };
    let mut parts = vec![time, supports];
    parts.extend(access);
    tr!("Oriented: {}; fits {} per plate.", parts.join(", "), copies).to_string()
}

pub(crate) fn world_edit(scene: &Scene, id: NodeId, delta: Transform3D) -> Option<SceneEdit> {
    let before = scene.node(id)?.local_transform;
    let world = scene.world_transform_of(id).ok()?;
    let parent = world.compose(&before.inverse().ok()?);
    let after = parent.inverse().ok()?.compose(&delta).compose(&world);
    Some(SceneEdit::SetLocalTransform {
        node_id: id,
        before,
        after,
    })
}

/// World-space orientation changes for `ids`, searching each distinct part
/// once: an identical copy (same mesh, turn and scale) takes the first
/// copy's change, moved with it. `search` gives a part's change, or `None`
/// to keep it as it is. Searching every copy on its own let a near tie fall
/// differently on one of them (Sleptic: Auto prepare left five fill copies
/// upright and turned the sixth 37 degrees).
pub(crate) fn orient_once_per_part(
    scene: &Scene,
    ids: &[NodeId],
    mut search: impl FnMut(NodeId) -> Option<Transform3D>,
) -> Vec<(NodeId, Transform3D)> {
    let pose = |id: NodeId| {
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
    let mut searched: Vec<((usize, [[f64; 3]; 3], [f64; 3]), Option<Transform3D>)> = Vec::new();
    let mut out = Vec::new();
    for &id in ids {
        let Some(key) = pose(id) else {
            if let Some(delta) = search(id) {
                out.push((id, delta));
            }
            continue;
        };
        let same = searched.iter().find(|((mesh, linear, _), _)| {
            *mesh == key.0
                && (0..3).all(|r| (0..3).all(|c| (linear[r][c] - key.1[r][c]).abs() < 1e-9))
        });
        let delta = match same {
            Some(((_, _, from), delta)) => delta.map(|delta| {
                let d = [key.2[0] - from[0], key.2[1] - from[1], key.2[2] - from[2]];
                Transform3D::Affine(AffineTransform3D::from_translation(d))
                    .compose(&delta)
                    .compose(&Transform3D::Affine(AffineTransform3D::from_translation([
                        -d[0], -d[1], -d[2],
                    ])))
            }),
            None => {
                let delta = search(id);
                searched.push((key, delta));
                delta
            }
        };
        if let Some(delta) = delta {
            out.push((id, delta));
        }
    }
    out
}

/// Pack model/support groups onto existing unlocked plates, appending plates
/// when a group would overflow. Planning is atomic and preserves locked plates.
pub(crate) fn arrange_all(
    scene: &Scene,
    volume: BuildVolume,
    gap: f64,
) -> Result<SceneEdit, Vec<NodeId>> {
    let before = scene.plates().clone();
    let mut after = before.clone();
    let leaves = scene
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .map_err(|_| Vec::new())?;
    let models: Vec<_> = leaves
        .iter()
        .copied()
        .filter(|id| {
            !scene
                .node(*id)
                .and_then(|n| n.meta.as_ref())
                .and_then(|m| m.comment.as_deref())
                .is_some_and(|c| c.starts_with("sc-generated-support"))
                && !before
                    .order
                    .iter()
                    .any(|p| p.id == scene.plate_of(*id) && p.locked)
        })
        .collect();
    if models.is_empty() {
        return Ok(SceneEdit::Compound(Vec::new()));
    }
    let mut destinations: Vec<_> = after
        .order
        .iter()
        .filter(|p| !p.locked)
        .map(|p| p.id)
        .collect();
    if destinations.is_empty() {
        destinations.push(after.add());
    }
    let mut trial = scene.clone();
    // Temporarily park movable groups on a separate plate, so each fit test
    // includes only groups accepted for the destination so far.
    let parked = after.add();
    for id in &models {
        after.assignments.insert(*id, parked);
        for support in crate::scene_document::owned_support_ids(scene, *id) {
            after.assignments.insert(support, parked);
        }
    }
    trial.set_plates(after.clone());
    let mut index = 0;
    for id in models {
        loop {
            let target = destinations[index];
            let mut candidate = trial.plates().clone();
            candidate.assignments.insert(id, target);
            for support in crate::scene_document::owned_support_ids(scene, id) {
                candidate.assignments.insert(support, target);
            }
            trial.set_plates(candidate);
            let plate = trial.plate_scene(target).map_err(|_| vec![id])?;
            if arrange_with_gap(&plate, volume, gap).is_ok() {
                break;
            }
            let count = leaves
                .iter()
                .filter(|leaf| trial.plate_of(**leaf) == target)
                .count();
            let group_count = 1 + crate::scene_document::owned_support_ids(scene, id).len();
            if count == group_count {
                return Err(vec![id]);
            }
            let mut state = trial.plates().clone();
            state.assignments.insert(id, parked);
            for support in crate::scene_document::owned_support_ids(scene, id) {
                state.assignments.insert(support, parked);
            }
            index += 1;
            if index == destinations.len() {
                destinations.push(state.add());
            }
            trial.set_plates(state);
        }
    }
    after = trial.plates().clone();
    after.order.retain(|p| p.id != parked);
    trial.set_plates(after.clone());
    let mut edits = vec![SceneEdit::SetPlates { before, after }];
    for target in destinations {
        edits.push(arrange_with_gap(
            &trial.plate_scene(target).map_err(|_| Vec::new())?,
            volume,
            gap,
        )?);
    }
    Ok(SceneEdit::Compound(edits))
}

/// Reflect in a world-axis plane through the bounding-box centre, then drop to plate.
pub(crate) fn mirror(scene: &Scene, id: NodeId, axis: usize) -> Option<SceneEdit> {
    if axis > 2 {
        return None;
    }
    let (min, max) = bounds(&points(scene, id)?)?;
    let mut scale = [1.0; 3];
    scale[axis] = -1.0;
    let mut shift = [0.0; 3];
    shift[axis] = (min + max)[axis];
    shift[2] -= min.z;
    world_edit(
        scene,
        id,
        Transform3D::Affine(AffineTransform3D::from_translation(shift)).compose(
            &Transform3D::Affine(AffineTransform3D::from_non_uniform_scale(scale)),
        ),
    )
}

/// Comment line Fill plate puts on each copy it makes, followed by the id of
/// the part it copies; support generation pastes that part's supports onto it.
pub(crate) const FILL_COPY_TAG: &str = "sc-fill-copy-of = ";

/// Plan a deterministic shelf-packed batch, including owned supports in each copy.
/// IDs are allocated on the supplied scene; the returned edit installs the leaves.
pub(crate) fn fill_plate(
    scene: &mut Scene,
    selected: NodeId,
    volume: BuildVolume,
    gap: f64,
) -> Option<(SceneEdit, usize)> {
    use sc_scene::node::{LeafAttachment, LeafPayload};
    use std::sync::Arc;
    let leaves = scene
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .ok()?;
    let mut owned: Vec<_> = leaves
        .into_iter()
        .filter(|&id| {
            id == selected
                || scene
                    .node(id)
                    .and_then(|n| n.meta.as_ref())
                    .and_then(|m| m.comment.as_deref())
                    .is_some_and(|c| {
                        c.starts_with("sc-generated-support")
                            && c.lines()
                                .any(|l| l == format!("owner = {}", selected.get()))
                    })
        })
        .collect();
    owned.sort_by_key(|&id| (id != selected, id));
    let group_points: Vec<_> = owned
        .iter()
        .flat_map(|&id| points(scene, id).unwrap_or_default())
        .collect();
    let (group_min, group_max) = bounds(&group_points)?;
    let group_size = group_max - group_min;
    let capacity = sc_print_glue::orient::copies_per_plate(
        group_size.x,
        group_size.y,
        sc_print_primitives::BuildVolume::new(
            f64::from(volume.width_mm()),
            f64::from(volume.depth_mm()),
            f64::from(volume.height_mm()),
        )
        .ok()?,
        gap,
    );
    // The part with `n` copies added (ids allocated in order on a clone of
    // `scene`, exactly as the final allocation below will hand them out).
    // Each copy is tagged with the part it copies, so support generation
    // can work the part out once and paste the result (Sleptic).
    let with_copies = |n: usize| -> Option<(Scene, Vec<SceneEdit>)> {
        let mut candidate = scene.clone();
        let mut batch = Vec::new();
        for _ in 0..n {
            let new_owner = candidate.allocate_unbound_node_id();
            for (i, &source) in owned.iter().enumerate() {
                let node_id = if i == 0 {
                    new_owner
                } else {
                    candidate.allocate_unbound_node_id()
                };
                let mut meta = scene.node(source)?.meta.clone();
                if let Some(comment) = meta.as_mut().and_then(|m| m.comment.as_mut()) {
                    *comment = comment.replace(
                        &format!("owner = {}", selected.get()),
                        &format!("owner = {}", new_owner.get()),
                    );
                }
                if i == 0 {
                    let meta = meta.get_or_insert_with(|| sc_scene::SceneMetadata {
                        name: None,
                        comment: None,
                    });
                    let comment = meta.comment.get_or_insert_with(String::new);
                    if !comment.lines().any(|l| l.starts_with(FILL_COPY_TAG)) {
                        if !comment.is_empty() && !comment.ends_with('\n') {
                            comment.push('\n');
                        }
                        comment.push_str(&format!("{FILL_COPY_TAG}{}\n", selected.get()));
                    }
                }
                let edit = SceneEdit::AddLeaf(sc_scene_edit::LeafRecord {
                    node_id,
                    attachment: LeafAttachment::Root {
                        position: candidate.roots().len(),
                    },
                    payload: LeafPayload::Mesh(Arc::clone(scene.leaf_mesh_arc(source)?)),
                    local_transform: scene.world_transform_of(source).ok()?,
                    meta,
                });
                edit.apply_to_scene(&mut candidate).ok()?;
                batch.push(edit);
            }
        }
        Some((candidate, batch))
    };
    let fits = |n: usize| {
        let (candidate, batch) = with_copies(n)?;
        let arrangement = arrange_with_gap(&candidate, volume, gap).ok()?;
        Some((batch, arrangement))
    };
    // Positive footprints and a positive gap bound the batch; cap protects corrupt/tiny meshes.
    // The most copies that still arrange, found by halving (Sleptic: Fill plate
    // was slow enough to freeze the app; it re-arranged the whole plate once
    // per added copy). `capacity` is the footprint estimate, so it usually
    // fits at once.
    let (mut additions, mut arrangement) = fits(0)?;
    let mut count = 0;
    let (mut lo, mut hi) = (0, capacity.saturating_sub(1).min(10000));
    if hi > 0 {
        if let Some(found) = fits(hi) {
            (additions, arrangement) = found;
            count = hi;
        } else {
            while hi - lo > 1 {
                let mid = (lo + hi) / 2;
                if let Some(found) = fits(mid) {
                    (additions, arrangement) = found;
                    count = mid;
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
        }
    }
    for edit in &additions {
        if let SceneEdit::AddLeaf(sc_scene_edit::LeafRecord { node_id, .. }) = edit
            && scene.allocate_unbound_node_id() != *node_id
        {
            return None;
        }
    }
    additions.push(arrangement);
    Some((SceneEdit::Compound(additions), count))
}

/// Add geometry with a deterministic bottom-left packing search against fixed obstacles.
/// Existing leaves never move. Refuse a batch instead of silently overlapping objects.
pub(crate) fn import_geometry(
    scene: &mut Scene,
    sources: &[Scene],
    volume: BuildVolume,
    plate: u32,
) -> Result<(SceneEdit, Vec<NodeId>), String> {
    import_geometry_positioned(scene, sources, volume, plate, false)
}
/// Machine patterns retain their calibrated plate-centred coordinates.
pub(crate) fn import_machine_geometry(
    scene: &mut Scene,
    sources: &[Scene],
    volume: BuildVolume,
    plate: u32,
) -> Result<(SceneEdit, Vec<NodeId>), String> {
    import_geometry_positioned(scene, sources, volume, plate, true)
}
fn import_geometry_positioned(
    scene: &mut Scene,
    sources: &[Scene],
    volume: BuildVolume,
    plate: u32,
    keep_origin: bool,
) -> Result<(SceneEdit, Vec<NodeId>), String> {
    use sc_scene::node::{LeafAttachment, LeafPayload};
    let plate_before = scene.plates().clone();
    let mut plate_after = plate_before.clone();
    let mut current_plate = plate;
    let mut obstacles = Vec::new();
    for id in scene
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .map_err(|e| e.to_string())?
    {
        if scene.plate_of(id) == plate
            && let Some(b) = points(scene, id).and_then(|p| bounds(&p))
        {
            obstacles.push(b);
        }
    }
    let mut edits = Vec::new();
    let mut ids = Vec::new();
    let left = -f64::from(volume.width_mm()) / 2.0;
    let bottom = -f64::from(volume.depth_mm()) / 2.0;
    for source in sources {
        for id in source
            .reachable_leaves(
                sc_scene::CsgPolicy::Descend,
                sc_scene::ReferencePolicy::Skip,
            )
            .map_err(|e| e.to_string())?
        {
            let (min, max) = points(source, id)
                .and_then(|p| bounds(&p))
                .ok_or_else(|| crate::i18n::tr!("The model has no geometry.").to_string())?;
            let size = max - min;
            let mut xs = vec![left];
            let mut ys = vec![bottom];
            for (_, hi) in &obstacles {
                xs.push(hi.x + 3.0);
                ys.push(hi.y + 3.0);
            }
            xs.sort_by(f64::total_cmp);
            ys.sort_by(f64::total_cmp);
            let mut placement = None;
            'search: for y in ys {
                for &x in &xs {
                    let lo = if keep_origin {
                        DVec3::new(min.x, min.y, 0.0)
                    } else {
                        DVec3::new(x, y, 0.0)
                    };
                    let hi = lo + size;
                    if hi.x > -left + FIT_SLACK_MM
                        || hi.y > -bottom + FIT_SLACK_MM
                        || hi.z > f64::from(volume.height_mm()) + FIT_SLACK_MM
                    {
                        continue;
                    }
                    if obstacles.iter().any(|(a, b)| {
                        lo.x < b.x + 3.0 && hi.x + 3.0 > a.x && lo.y < b.y + 3.0 && hi.y + 3.0 > a.y
                    }) {
                        continue;
                    }
                    placement = Some((lo, hi));
                    break 'search;
                }
            }
            if placement.is_none()
                && !keep_origin
                && size.x <= -2.0 * left + FIT_SLACK_MM
                && size.y <= -2.0 * bottom + FIT_SLACK_MM
                && size.z <= f64::from(volume.height_mm()) + FIT_SLACK_MM
            {
                current_plate = plate_after.add();
                obstacles.clear();
                let lo = DVec3::new(left, bottom, 0.0);
                placement = Some((lo, lo + size));
            }
            let (lo, hi) = placement.ok_or_else(|| {
                crate::i18n::tr!("The objects could not be placed within the build volume.")
                    .to_string()
            })?;
            let node_id = scene.allocate_unbound_node_id();
            let transform =
                Transform3D::Affine(AffineTransform3D::from_translation((lo - min).to_array()))
                    .compose(&source.world_transform_of(id).map_err(|e| e.to_string())?);
            let edit = SceneEdit::AddLeaf(sc_scene_edit::LeafRecord {
                node_id,
                attachment: LeafAttachment::Root {
                    position: scene.roots().len(),
                },
                payload: LeafPayload::Mesh(source.leaf_mesh_arc(id).ok_or("Missing mesh")?.clone()),
                local_transform: transform,
                meta: source.node(id).and_then(|n| n.meta.clone()),
            });
            edit.apply_to_scene(scene).map_err(|e| e.to_string())?;
            obstacles.push((lo, hi));
            plate_after.assignments.insert(node_id, current_plate);
            ids.push(node_id);
            edits.push(edit);
        }
    }
    edits.push(SceneEdit::SetPlates {
        before: plate_before,
        after: plate_after,
    });
    Ok((SceneEdit::Compound(edits), ids))
}

/// Place the largest triangle's outward normal against the plate and drop to Z=0.
#[cfg(test)]
pub(crate) fn lay_flat(scene: &Scene, id: NodeId) -> Option<SceneEdit> {
    let mesh = sc_scene::bake_leaf_to_world(scene, id).ok()?;
    let vertices = points(scene, id)?;
    let normal = mesh
        .faces
        .iter()
        .filter_map(|face| {
            let [a, b, c] = face.vertices.map(|i| vertices.get(usize::from(i)).copied());
            Some((b? - a?).cross(c? - a?))
        })
        .filter(|n| n.is_finite() && n.length_squared() > 1e-16)
        .max_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))?
        .normalize();
    place_face_down(scene, id, normal)
}

/// The flat sides of the model's convex hull it can stand on (Place on
/// face), in the scene's world millimetres, largest first.
pub(crate) fn place_faces(scene: &Scene, id: NodeId) -> Vec<crate::place_on_face::Face> {
    let (Ok(mesh), Some(vertices)) = (sc_scene::bake_leaf_to_world(scene, id), points(scene, id))
    else {
        return Vec::new();
    };
    // Centre of mass of the closed solid (signed tetrahedra from the
    // origin); the vertex average for a mesh with no volume.
    let (mut volume, mut moment) = (0.0, DVec3::ZERO);
    for face in &mesh.faces {
        let [Some(a), Some(b), Some(c)] =
            face.vertices.map(|i| vertices.get(usize::from(i)).copied())
        else {
            continue;
        };
        let v = a.dot(b.cross(c)) / 6.0;
        volume += v;
        moment += (a + b + c) * (v / 4.0);
    }
    let centre = if volume.abs() > 1e-9 {
        moment / volume
    } else {
        vertices.iter().copied().sum::<DVec3>() / vertices.len().max(1) as f64
    };
    crate::place_on_face::faces(&vertices, centre)
}

/// Scale to fit (Sleptic, from Orca's object menu): grow or shrink the model
/// uniformly to the largest size that fits the build volume, leaving 2%
/// clearance to the walls, centred on its plate and resting on it.
pub(crate) fn scale_to_fit(scene: &Scene, id: NodeId, volume: BuildVolume) -> Option<SceneEdit> {
    const FIT: f64 = 0.98;
    let vertices = points(scene, id)?;
    let (min, max) = bounds(&vertices)?;
    let size = max - min;
    let room = DVec3::new(
        f64::from(volume.width_mm()),
        f64::from(volume.depth_mm()),
        f64::from(volume.height_mm()),
    ) * FIT;
    let factor = [room.x / size.x, room.y / size.y, room.z / size.z]
        .into_iter()
        .filter(|f| f.is_finite() && *f > 0.0)
        .reduce(f64::min)?;
    let centre = (min + max) * 0.5;
    let delta = Transform3D::Affine(AffineTransform3D::from_translation([
        0.0,
        0.0,
        factor * size.z * 0.5,
    ]))
    .compose(&Transform3D::Affine(AffineTransform3D::from_uniform_scale(
        factor,
    )))
    .compose(&Transform3D::Affine(AffineTransform3D::from_translation(
        (-centre).to_array(),
    )));
    world_edit(scene, id, delta)
}

/// Turn the model so the side with outward `normal` (world) rests on the
/// plate, keeping its footprint centre where it was.
pub(crate) fn place_face_down(scene: &Scene, id: NodeId, normal: DVec3) -> Option<SceneEdit> {
    let mesh = sc_scene::bake_leaf_to_world(scene, id).ok()?;
    let vertices = points(scene, id)?;
    let normal = normal.try_normalize()?;
    let target = -DVec3::Z;
    let axis = normal.cross(target);
    let rotation = if axis.length_squared() < 1e-16 {
        if normal.dot(target) > 0.0 {
            AffineTransform3D::identity()
        } else {
            AffineTransform3D::from_rotation_x(std::f64::consts::PI)
        }
    } else {
        AffineTransform3D::from_axis_angle(
            axis.to_array(),
            normal.dot(target).clamp(-1.0, 1.0).acos(),
        )
        .ok()?
    };
    let rotate = Transform3D::Affine(rotation);
    let rotated: Vec<DVec3> = mesh
        .vertices
        .iter()
        .map(|v| DVec3::from_array(rotate.apply_to_vertex(*v).coords().map(f64::from)))
        .collect();
    let (min, max) = bounds(&rotated)?;
    let (old_min, old_max) = bounds(&vertices)?;
    let shift = (old_min + old_max - min - max) * 0.5;
    let delta = Transform3D::Affine(AffineTransform3D::from_translation([
        shift.x, shift.y, -min.z,
    ]))
    .compose(&rotate);
    world_edit(scene, id, delta)
}

/// Shelf packing with the default 3 mm gap. Supports translate with their owner.
#[cfg(test)]
pub(crate) fn arrange(scene: &Scene, volume: BuildVolume) -> Option<SceneEdit> {
    arrange_with_gap(scene, volume, 3.0).ok()
}

/// World bounds of leaves, each distinct mesh and pose measured once and
/// moved by the leaf's translation: copies (Fill plate makes hundreds) no
/// longer re-read their whole mesh on every arrange.
#[derive(Default)]
struct ShapeBounds {
    seen: std::collections::HashMap<(usize, [u64; 9]), (DVec3, DVec3)>,
}

impl ShapeBounds {
    fn of(&mut self, scene: &Scene, id: NodeId) -> Option<(DVec3, DVec3)> {
        let (linear, t) = match scene.world_transform_of(id).ok()? {
            Transform3D::Affine(a) => (*a.linear(), *a.translation()),
            Transform3D::Identity => ([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0; 3]),
        };
        let mesh = scene.leaf_mesh_arc(id)?;
        let key = (
            std::sync::Arc::as_ptr(mesh) as usize,
            [0, 1, 2, 3, 4, 5, 6, 7, 8].map(|k| linear[k / 3][k % 3].to_bits()),
        );
        let (lo, hi) = match self.seen.get(&key) {
            Some(b) => *b,
            None => {
                let points: Vec<DVec3> = mesh
                    .vertices
                    .iter()
                    .map(|v| {
                        let p = v.coords().map(f64::from);
                        DVec3::new(
                            linear[0][0] * p[0] + linear[0][1] * p[1] + linear[0][2] * p[2],
                            linear[1][0] * p[0] + linear[1][1] * p[1] + linear[1][2] * p[2],
                            linear[2][0] * p[0] + linear[2][1] * p[1] + linear[2][2] * p[2],
                        )
                    })
                    .collect();
                let b = bounds(&points)?;
                self.seen.insert(key, b);
                b
            }
        };
        let t = DVec3::from_array(t);
        Some((lo + t, hi + t))
    }
}

/// Room (mm, each side) a part's future supports take on the plate, by
/// how it will print: lifted onto a raft, its raft reaches half a support
/// base plus the raft margin past its footprint; standing on the plate with
/// overhangs, its columns' bases reach half a base past it; printing
/// directly, nothing. Worked out once per distinct mesh and pose, so
/// filling a plate with copies stays cheap.
#[derive(Default)]
struct SupportRoom {
    seen: Vec<(usize, [[f64; 3]; 3], f64)>,
}

impl SupportRoom {
    fn of(&mut self, scene: &Scene, id: NodeId) -> f64 {
        let linear = match scene.world_transform_of(id) {
            Ok(Transform3D::Affine(a)) => *a.linear(),
            _ => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        };
        let mesh_key = scene
            .leaf_mesh_arc(id)
            .map_or(0, |m| std::sync::Arc::as_ptr(m) as usize);
        if mesh_key != 0
            && let Some((_, _, pad)) = self
                .seen
                .iter()
                .find(|(m, l, _)| *m == mesh_key && *l == linear)
        {
            return *pad;
        }
        let settings = sc_print_profiles::support_settings::SupportSettings::default();
        let pad = sc_scene::bake_leaf_to_world(scene, id).map_or(0.0, |mut mesh| {
            // Judged resting on the plate, where arrange puts it.
            let floor = mesh.vertices.iter().map(|v| v[2]).fold(f32::INFINITY, f32::min);
            for v in &mut mesh.vertices {
                *v = sc_mesh_core::Vertex::new([v[0], v[1], v[2] - floor]);
            }
            match sc_print_glue::supports::placement(&mesh, &settings, None) {
                sc_print_glue::supports::Placement::Lifted => {
                    settings.base_diameter_mm / 2.0 + settings.raft_margin_mm
                }
                sc_print_glue::supports::Placement::Anchored => settings.base_diameter_mm / 2.0,
                sc_print_glue::supports::Placement::Direct => 0.0,
            }
        });
        if mesh_key != 0 {
            self.seen.push((mesh_key, linear, pad));
        }
        pad
    }
}

/// Returns the stable IDs of every group that cannot fit; failure applies no partial edit.
pub(crate) fn arrange_with_gap(
    scene: &Scene,
    volume: BuildVolume,
    gap: f64,
) -> Result<SceneEdit, Vec<NodeId>> {
    let leaves = scene
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .map_err(|_| Vec::new())?;
    let comment = |id| {
        scene
            .node(id)
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.comment.as_deref())
            .unwrap_or("")
    };
    let models: Vec<_> = leaves
        .iter()
        .copied()
        .filter(|&id| !comment(id).starts_with("sc-generated-support"))
        .collect();
    let mut groups: Vec<Vec<NodeId>> = models.iter().map(|&id| vec![id]).collect();
    for &id in &leaves {
        if !comment(id).starts_with("sc-generated-support") {
            continue;
        }
        let owner = comment(id).lines().find_map(|l| {
            l.strip_prefix("owner = ")
                .and_then(|v| v.parse::<u32>().ok())
        });
        let index = models
            .iter()
            .position(|m| Some(m.get()) == owner)
            .unwrap_or(0);
        if let Some(group) = groups.get_mut(index) {
            group.push(id);
        } else {
            groups.push(vec![id]);
        }
    }
    let gap = if gap.is_finite() { gap.max(0.0) } else { 3.0 };
    let mut room = SupportRoom::default();
    let (left, bottom) = (
        -f64::from(volume.width_mm()) / 2.0,
        -f64::from(volume.depth_mm()) / 2.0,
    );
    let (right, top) = (-left, -bottom);
    let (mut x, mut y, mut row_height) = (left, bottom, 0.0f64);
    let mut edits = Vec::new();
    let mut failed = Vec::new();
    let mut packed_max = DVec3::new(left, bottom, 0.0);
    let mut shapes = ShapeBounds::default();
    for group in groups {
        let Some((mut min, mut max)) = group
            .iter()
            .filter_map(|&id| shapes.of(scene, id))
            .reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
        else {
            failed.push(group[0]);
            continue;
        };
        // A part with no supports yet that will need them gets room for
        // its raft / support bases around it (Sleptic: arrange should know
        // ahead of time which parts need supports and space them for it).
        // Parts already supported are measured with their supports.
        if group.len() == 1 {
            // Never more room than the plate has: a part that only just fits
            // keeps its place (its raft is clipped to the plate anyway).
            let size = max - min;
            let pad = room
                .of(scene, group[0])
                .min((right - left - size.x) / 2.0)
                .min((top - bottom - size.y) / 2.0)
                .max(0.0);
            min -= DVec3::new(pad, pad, 0.0);
            max += DVec3::new(pad, pad, 0.0);
        }
        let size = max - min;
        if x + size.x > right + FIT_SLACK_MM {
            x = left;
            y += row_height + gap;
            row_height = 0.0;
        }
        if x + size.x > right + FIT_SLACK_MM
            || y + size.y > top + FIT_SLACK_MM
            || size.z > f64::from(volume.height_mm()) + FIT_SLACK_MM
        {
            failed.push(group[0]);
            continue;
        }
        let delta = Transform3D::Affine(AffineTransform3D::from_translation([
            x - min.x,
            y - min.y,
            -min.z,
        ]));
        packed_max = packed_max.max(DVec3::new(x + size.x, y + size.y, size.z));
        for id in group {
            if let Some(edit) = world_edit(scene, id, delta) {
                edits.push(edit);
            } else {
                failed.push(id);
            }
        }
        x += size.x + gap;
        row_height = row_height.max(size.y);
    }
    if failed.is_empty() {
        let center = Transform3D::Affine(AffineTransform3D::from_translation([
            -(left + packed_max.x) * 0.5,
            -(bottom + packed_max.y) * 0.5,
            0.0,
        ]));
        // Compose in world space, including for leaves under transformed parents.
        for edit in &mut edits {
            if let SceneEdit::SetLocalTransform {
                node_id,
                before,
                after,
            } = edit
            {
                let world = scene
                    .world_transform_of(*node_id)
                    .map_err(|_| vec![*node_id])?;
                let parent = world.compose(&before.inverse().map_err(|_| vec![*node_id])?);
                *after = parent
                    .inverse()
                    .map_err(|_| vec![*node_id])?
                    .compose(&center)
                    .compose(&parent)
                    .compose(after);
            }
        }
        Ok(SceneEdit::Compound(edits))
    } else {
        Err(failed)
    }
}

/// Shared resin orientation with a hard build-envelope constraint.
pub(crate) fn auto_orient(
    scene: &Scene,
    id: NodeId,
    support_angle_deg: f64,
    volume: BuildVolume,
    weights: [f64; 4],
    context: sc_print_glue::orient::Context,
) -> Option<SceneEdit> {
    let proposal = sc_print_glue::orient::search_with_context(
        scene,
        id,
        sc_print_primitives::BuildVolume::new(
            f64::from(volume.width_mm()),
            f64::from(volume.depth_mm()),
            f64::from(volume.height_mm()),
        )
        .ok()?,
        support_angle_deg,
        weights,
        context,
    )?;
    eprintln!(
        "Auto orient: support area {:.1} -> {:.1} mm2, height {:.2} -> {:.2} mm, fits {}, retained {}",
        proposal.original.support_area,
        proposal.chosen.support_area,
        proposal.original.height,
        proposal.chosen.height,
        proposal.chosen.fits,
        proposal.retained
    );
    if proposal.retained {
        let before = scene.node(id)?.local_transform;
        return Some(SceneEdit::SetLocalTransform {
            node_id: id,
            before,
            after: before,
        });
    }
    world_edit(scene, id, proposal.delta)
}

#[cfg(test)]
#[path = "tests_layout_tools.rs"]
mod tests;

/// Read the same weights for selected-model, whole-plate and batch orientation.
/// Auto orient's objective. Fixed on purpose (Sleptic, 2026-10-30): users
/// should not have to tune weights; the slicer knows the trade-offs.
pub(crate) fn orient_weights(_ui: &crate::MainWindow) -> [f64; 4] {
    sc_print_glue::orient::Preset::Balanced.weights()
}

/// Enclosed volume of a closed mesh in its own coordinates (mm³), by the
/// divergence theorem; open meshes give an approximation, never a negative.
pub(crate) fn mesh_volume_mm3(mesh: &sc_mesh_core::IndexedMesh) -> f64 {
    let p = |i: usize| -> [f64; 3] {
        let c = mesh.vertices[i].coords();
        [f64::from(c[0]), f64::from(c[1]), f64::from(c[2])]
    };
    let sum: f64 = mesh
        .faces
        .iter()
        .map(|f| {
            let [a, b, c] = f.vertices.map(|v| p(v.get() as usize));
            a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0])
        })
        .sum();
    (sum / 6.0).abs()
}
