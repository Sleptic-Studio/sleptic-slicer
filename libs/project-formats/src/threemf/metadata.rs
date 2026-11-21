// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use sc_scene::{CsgPolicy, LeafPayload, NodeId, Scene, SceneMetadata};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
const OBJECTS: &str = "/Metadata/sleptic-objects.toml";
const SUPPORTS: &str = "/Metadata/sleptic-supports.toml";
/// Where projects saved before R187 keep their supports; still read.
const OLD_SUPPORTS: &str = "/Metadata/soulcrafted-supports.toml";
const HOLLOW: &str = "/Metadata/sleptic-hollow.toml";
const PLATES: &str = "/Metadata/sleptic-plates.toml";
#[derive(Serialize, Deserialize)]
struct Objects {
    version: u32,
    leaf_count: usize,
    objects: Vec<Object>,
}
#[derive(Serialize, Deserialize)]
struct Object {
    index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    comment: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Supports {
    version: u32,
    supports: Vec<Support>,
    #[serde(default)]
    painted_models: Vec<Paint>,
}
#[derive(Default, Serialize, Deserialize)]
struct Support {
    #[serde(default)]
    object_name: String,
    #[serde(default)]
    leaf_index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    normal: Option<[f32; 3]>,
    #[serde(default)]
    raft: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner_leaf_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lift_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner_linear: Option<[[f64; 3]; 3]>,
    #[serde(default)]
    geometry_stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    small: Option<bool>,
}
#[derive(Default, Deserialize)]
struct Contact {
    position: Option<[f32; 3]>,
    normal: Option<[f32; 3]>,
    owner: Option<u32>,
    lift_mm: Option<f64>,
    owner_linear: Option<[[f64; 3]; 3]>,
    small: Option<bool>,
}
#[derive(Serialize, Deserialize)]
struct Paint {
    leaf_index: usize,
    comment: String,
}
#[derive(Serialize, Deserialize)]
struct Hollows {
    version: u32,
    objects: Vec<Hollow>,
}
#[derive(Serialize, Deserialize)]
struct Hollow {
    leaf_index: usize,
    data: String,
}
#[derive(Serialize, Deserialize)]
struct Plates {
    version: u32,
    plates: Vec<Plate>,
    assignments: Vec<u32>,
}
#[derive(Serialize, Deserialize)]
struct Plate {
    id: u32,
    locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    settings: Option<String>,
}
fn leaves(s: &Scene) -> Vec<NodeId> {
    let mut ids = vec![];
    let _: Result<(), sc_scene::SceneError> =
        s.for_each_expanded_leaf(CsgPolicy::Descend, |id, _, _| {
            ids.push(id);
            Ok(())
        });
    ids
}
fn save_part(
    p: &mut Package,
    name: &str,
    v: &impl Serialize,
    rel_type: Option<&str>,
) -> Result<(), WriteThreeMfProjectError> {
    add(
        p,
        name,
        "application/toml",
        toml_edit::ser::to_string_pretty(v)?.into_bytes(),
        rel_type,
    )
}
pub(super) fn save(s: &Scene, p: &mut Package) -> Result<(), WriteThreeMfProjectError> {
    let ids = leaves(s);
    let mut objects = vec![];
    let mut supports = vec![];
    let mut painted_models = vec![];
    let mut hollow = vec![];
    for (index, &id) in ids.iter().enumerate() {
        if let Some(meta) = s.node(id).and_then(|n| n.meta.as_ref()) {
            let comment = meta.comment.as_deref().unwrap_or("");
            if !comment.starts_with("sc-generated-support") {
                objects.push(Object {
                    index,
                    name: meta.name.clone(),
                    comment: meta.comment.clone(),
                });
            }
            if comment.lines().any(|l| l.starts_with("sc-support-paint ")) {
                painted_models.push(Paint {
                    leaf_index: index,
                    comment: comment.into(),
                });
            }
            if let (Some(name), Some(comment)) = (&meta.name, &meta.comment)
                && comment.starts_with("sc-generated-support")
            {
                let raft = comment.starts_with("sc-generated-support-raft");
                let text = comment.split_once('\n').map(|(_, v)| v).unwrap_or("");
                let contact = toml_edit::de::from_str::<Contact>(text);
                if raft || contact.is_ok() {
                    let c = contact.unwrap_or_default();
                    if raft || (c.position.is_some() && c.normal.is_some()) {
                        supports.push(Support {
                            object_name: name.clone(),
                            leaf_index: index,
                            position: if raft { None } else { c.position },
                            normal: if raft { None } else { c.normal },
                            raft,
                            owner_leaf_index: c
                                .owner
                                .and_then(|o| ids.iter().position(|i| i.get() == o)),
                            lift_mm: if raft { None } else { c.lift_mm },
                            owner_linear: c.owner_linear,
                            geometry_stale: comment.lines().any(|l| l == "geometry_stale = true"),
                            small: c.small,
                        });
                    }
                }
            }
        }
        if let Ok(mesh) = s.mesh_at_leaf(id)
            && let Some(comment) = mesh.meta.as_ref().and_then(|m| m.header_comment.as_ref())
            && comment.starts_with("sleptic-hollow-v1\n")
        {
            hollow.push(Hollow {
                leaf_index: index,
                data: comment.clone(),
            });
        }
    }
    let plates = Plates {
        version: 1,
        plates: s
            .plates()
            .order
            .iter()
            .map(|p| Plate {
                id: p.id,
                locked: p.locked,
                settings: p.settings.clone(),
            })
            .collect(),
        assignments: ids.iter().map(|&id| s.plate_of(id)).collect(),
    };
    save_part(
        p,
        PLATES,
        &plates,
        Some("https://schemas.sleptic.org/slicer/2026/plates"),
    )?;
    if !objects.is_empty() {
        save_part(
            p,
            OBJECTS,
            &Objects {
                version: 1,
                leaf_count: ids.len(),
                objects,
            },
            None,
        )?;
    }
    if !hollow.is_empty() {
        save_part(
            p,
            HOLLOW,
            &Hollows {
                version: 1,
                objects: hollow,
            },
            None,
        )?;
    }
    if !supports.is_empty() || !painted_models.is_empty() {
        save_part(
            p,
            SUPPORTS,
            &Supports {
                version: 1,
                supports,
                painted_models,
            },
            None,
        )?;
    }
    Ok(())
}
fn document<T: serde::de::DeserializeOwned>(p: &Package, name: &str) -> Option<T> {
    toml_edit::de::from_str(std::str::from_utf8(data(p, name)?).ok()?).ok()
}
fn meta(s: &mut Scene, id: NodeId, name: Option<String>, comment: Option<String>) {
    let _ = s.set_node_meta(id, Some(SceneMetadata { name, comment }));
}
fn owner(c: &Support, ids: &[NodeId]) -> Option<NodeId> {
    c.owner_leaf_index
        .filter(|&i| i != c.leaf_index)
        .and_then(|i| ids.get(i).copied())
}
fn centre(s: &Scene, id: NodeId) -> Option<[f64; 2]> {
    let mesh = s.mesh_at_leaf(id).ok()?;
    let transform = s.world_transform_of(id).ok()?;
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for &v in &mesh.vertices {
        let v = transform.apply_to_vertex(v);
        for (k, value) in [v[0], v[1]].into_iter().enumerate() {
            lo[k] = lo[k].min(value as f64);
            hi[k] = hi[k].max(value as f64);
        }
    }
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    c.iter().all(|v| v.is_finite()).then_some(c)
}
pub(super) fn restore(s: &mut Scene, p: &Package) {
    let ids = leaves(s);
    if let Some(o) = document::<Objects>(p, OBJECTS)
        && o.version == 1
        && o.leaf_count == ids.len()
    {
        for o in o.objects {
            if let Some(&id) = ids.get(o.index) {
                meta(s, id, o.name, o.comment);
            }
        }
    }
    if let Some(d) = document::<Supports>(p, SUPPORTS)
        .or_else(|| document::<Supports>(p, OLD_SUPPORTS))
        && d.version == 1
    {
        for m in d.painted_models {
            if m.comment.len() <= 2_000_000
                && let Some(&id) = ids.get(m.leaf_index)
            {
                let name = s
                    .node(id)
                    .and_then(|n| n.meta.as_ref())
                    .and_then(|m| m.name.clone());
                meta(s, id, name, Some(m.comment));
            }
        }
        let support_ids: Vec<_> = d
            .supports
            .iter()
            .filter_map(|c| ids.get(c.leaf_index).copied())
            .collect();
        // Ordinary contacts and modern rafts already have owners. Only legacy
        // rafts need bounding boxes; avoid walking unrelated geometry otherwise.
        let need_nearest = d
            .supports
            .iter()
            .any(|c| c.raft && owner(c, &ids).is_none());
        let models: Vec<_> = ids
            .iter()
            .copied()
            .filter(|id| need_nearest && !support_ids.contains(id))
            .filter_map(|id| centre(s, id).map(|c| (id, c)))
            .collect();
        for c in d.supports {
            let Some(&id) = ids.get(c.leaf_index) else {
                continue;
            };
            let mut lines = vec![if c.raft {
                "sc-generated-support-raft".into()
            } else {
                "sc-generated-support".into()
            }];
            if !c.raft {
                let (Some(position), Some(normal)) = (c.position, c.normal) else {
                    continue;
                };
                if position.iter().chain(&normal).any(|v| !v.is_finite())
                    || normal.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() < 1e-12
                {
                    continue;
                }
                lines.push(format!("position = {position:?}"));
                lines.push(format!("normal = {normal:?}"));
            }
            let explicit = owner(&c, &ids);
            let owner = if c.raft && explicit.is_none() {
                centre(s, id).and_then(|a| {
                    models
                        .iter()
                        .min_by(|(_, b), (_, c)| {
                            let dist = |b: &[f64; 2]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);
                            dist(b).total_cmp(&dist(c))
                        })
                        .map(|&(id, _)| id)
                })
            } else {
                explicit
            };
            if let Some(o) = owner {
                lines.push(format!("owner = {}", o.get()));
            }
            if !c.raft
                && let Some(v) = c.lift_mm.filter(|v| v.is_finite() && *v > 0.0)
            {
                lines.push(format!("lift_mm = {v}"));
            }
            if let Some(v) = c.small {
                lines.push(format!("small = {v}"));
            }
            if c.geometry_stale {
                lines.push("geometry_stale = true".into());
            }
            if let Some(v) = c
                .owner_linear
                .filter(|v| v.iter().flatten().all(|v| v.is_finite()))
            {
                lines.push(format!("owner_linear = {v:?}"));
            }
            meta(s, id, Some(c.object_name), Some(lines.join("\n")));
        }
    }
    if let Some(d) = document::<Hollows>(p, HOLLOW)
        && d.version == 1
    {
        for h in d.objects {
            if let Some(&id) = ids.get(h.leaf_index)
                && let Some(old) = s.leaf_mesh_arc(id).cloned()
            {
                let mut mesh = (*old).clone();
                mesh.meta
                    .get_or_insert_with(Default::default)
                    .header_comment = Some(h.data);
                let mesh = Arc::new(mesh); // Detach only this occurrence: shared geometry must not spread editable metadata to siblings.
                if let Ok(old_leaf) = s.remove_leaf(id) {
                    let _ = s.install_leaf_at(
                        id,
                        old_leaf.attachment,
                        LeafPayload::Mesh(mesh),
                        old_leaf.local_transform,
                        old_leaf.meta,
                    );
                }
            }
        }
    }
    if let Some(d) = document::<Plates>(p, PLATES)
        && d.version == 1
        && d.assignments.len() == ids.len()
    {
        let assignments: BTreeMap<_, _> = ids.iter().copied().zip(d.assignments).collect();
        if let Some(plates) = sc_scene::plates::Plates::from_parts(
            d.plates
                .into_iter()
                .map(|p| sc_scene::plates::Plate {
                    id: p.id,
                    locked: p.locked,
                    settings: p.settings,
                })
                .collect(),
            assignments,
        ) {
            s.set_plates(plates);
        }
    }
}
