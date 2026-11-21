// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Sidebar rows and scene counts.

use sc_scene::node::{LeafPayload, NodeKind};
use sc_scene::{CsgPolicy, ReferencePolicy, Scene};
use slint::{ModelRc, SharedString, StandardListViewItem, VecModel};

use crate::MainWindow;
use crate::i18n::ctr;

// Names follow the same leaf order as viewport picking.
fn leaf_names(graph: &Scene) -> Vec<String> {
    let leaves = graph
        .reachable_leaves(CsgPolicy::Descend, ReferencePolicy::Skip)
        .expect("structural traversal cannot fail");
    let mut seen = std::collections::HashMap::<String, usize>::new();
    leaves
        .into_iter()
        .enumerate()
        .map(|(index, id)| {
            let node = graph.node(id).expect("reachable leaf exists");
            let NodeKind::Leaf(payload) = &node.kind else {
                unreachable!()
            };
            let mesh = match payload {
                LeafPayload::Mesh(mesh) => Some(mesh.as_ref()),
                LeafPayload::MeshResource(id) => graph.mesh_resource(*id),
            };
            let name = node
                .meta
                .as_ref()
                .and_then(|m| m.name.clone())
                .or_else(|| mesh.and_then(|m| m.meta.as_ref().and_then(|meta| meta.name.clone())))
                .unwrap_or_else(|| ctr!("ObjectList", "Object {}", index + 1).to_string());
            let occurrence = seen.entry(name.clone()).or_default();
            *occurrence += 1;
            if *occurrence == 1 {
                name
            } else {
                format!("{name} ({occurrence})")
            }
        })
        .collect()
}

/// One object-list row maps to one model leaf, or all supports owned by it.
#[derive(Debug, PartialEq)]
pub(super) struct ObjectRow {
    pub name: String,
    pub leaf_indices: Vec<usize>,
    pub support: bool,
}

pub(super) fn object_rows(graph: &Scene) -> Vec<ObjectRow> {
    let leaves = graph
        .reachable_leaves(CsgPolicy::Descend, ReferencePolicy::Skip)
        .unwrap_or_default();
    let names = leaf_names(graph);
    let comment = |i: usize| {
        graph
            .node(leaves[i])
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.comment.as_deref())
            .unwrap_or("")
    };
    let models: Vec<_> = (0..leaves.len())
        .filter(|&i| !comment(i).starts_with("sc-generated-support"))
        .collect();
    if models.is_empty() && !leaves.is_empty() {
        let count = (0..leaves.len())
            .filter(|&i| !comment(i).starts_with("sc-generated-support-raft"))
            .count();
        return vec![ObjectRow {
            name: ctr!("ObjectList", "Supports ({})", count).to_string(),
            leaf_indices: (0..leaves.len()).collect(),
            support: true,
        }];
    }
    let mut groups = vec![Vec::new(); models.len()];
    let mut counts = vec![0; models.len()];
    let mut small = vec![0; models.len()];
    for i in 0..leaves.len() {
        let meta = comment(i);
        if !meta.starts_with("sc-generated-support") {
            continue;
        }
        let owner = meta.lines().find_map(|line| {
            line.strip_prefix("owner = ")
                .and_then(|v| v.parse::<u32>().ok())
        });
        // A shared plate raft and legacy supports without ownership belong to
        // the first model's support row. Counts describe columns, not rafts.
        let group = models
            .iter()
            .position(|&m| Some(leaves[m].get()) == owner)
            .unwrap_or(0);
        if let Some(indices) = groups.get_mut(group) {
            indices.push(i);
            if !meta.starts_with("sc-generated-support-raft") {
                counts[group] += 1;
                if meta.lines().any(|l| l == "small = true") {
                    small[group] += 1;
                }
            }
        }
    }
    let mut rows = Vec::new();
    for (g, &i) in models.iter().enumerate() {
        rows.push(ObjectRow {
            name: names[i].clone(),
            leaf_indices: vec![i],
            support: false,
        });
        if !groups[g].is_empty() {
            rows.push(ObjectRow {
                name: if small[g] > 0 {
                    ctr!("ObjectList", "Supports ({}, {} short)", counts[g], small[g]).to_string()
                } else {
                    ctr!("ObjectList", "Supports ({})", counts[g]).to_string()
                },
                leaf_indices: std::mem::take(&mut groups[g]),
                support: true,
            });
        }
    }
    rows
}

/// Which object-list rows are support rows (their trash icon removes them).
pub(super) fn support_row_flags(graph: &Scene) -> Vec<bool> {
    object_rows(graph)
        .into_iter()
        .map(|row| row.support)
        .collect()
}

/// The model that owns the support row at `row`: support rows always follow
/// their model's row. `None` for model rows and out-of-range indices.
pub(crate) fn support_row_owner(graph: &Scene, row: usize) -> Option<sc_scene::NodeId> {
    let rows = object_rows(graph);
    if !rows.get(row)?.support {
        return None;
    }
    let leaves = graph
        .reachable_leaves(CsgPolicy::Descend, ReferencePolicy::Skip)
        .ok()?;
    let model = rows.get(row.checked_sub(1)?)?;
    (!model.support).then(|| leaves[model.leaf_indices[0]])
}

pub(super) fn build_object_list_names(graph: &Scene) -> Vec<String> {
    object_rows(graph).into_iter().map(|row| row.name).collect()
}

/// Replace the window model when the scene list changes.
pub(super) fn fill_object_list(ui: &MainWindow, names: Vec<String>) {
    let model = VecModel::default();
    for name in names {
        model.push(StandardListViewItem::from(SharedString::from(name)));
    }
    ui.set_parts_list(ModelRc::new(model));
}

/// Object count and total triangle count across every reachable leaf
/// mesh in `graph.roots()`. Deliberately a plain triangle sum, not a
/// unique-mesh count: an instanced/CSG-referenced mesh counted at each
/// reachable occurrence matches what the viewer actually renders and
/// what a slice would actually process, not the on-disk mesh count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct SceneStats {
    /// Models: object-list rows that are not support rows (R109: support
    /// rows counted as objects, "Objects (14)" for 9 Benchys).
    pub object_count: usize,
    /// Generated support pieces (rafts not counted).
    pub support_count: usize,
    pub triangle_count: u64,
}

/// Count each displayed mesh occurrence, including support geometry.
pub(super) fn scene_stats(graph: &Scene) -> SceneStats {
    let leaves = graph
        .reachable_leaves(CsgPolicy::Descend, ReferencePolicy::Skip)
        .expect("structural traversal cannot fail");
    let support_count = leaves
        .iter()
        .filter(|id| {
            let comment = graph
                .node(**id)
                .and_then(|n| n.meta.as_ref())
                .and_then(|m| m.comment.as_deref())
                .unwrap_or("");
            comment.starts_with("sc-generated-support")
                && !comment.starts_with("sc-generated-support-raft")
        })
        .count();
    SceneStats {
        object_count: object_rows(graph).iter().filter(|row| !row.support).count(),
        support_count,
        triangle_count: leaf_triangle_counts(graph).iter().sum(),
    }
}

/// Render the summary only when there is a model row.
pub(super) fn format_scene_stats(stats: SceneStats) -> String {
    if stats.object_count == 0 {
        return String::new();
    }
    let objects = if stats.object_count == 1 {
        "1 object".to_owned()
    } else {
        format!("{} objects", format_thousands(stats.object_count as u64))
    };
    let triangles = if stats.triangle_count == 1 {
        "1 triangle".to_owned()
    } else {
        format!("{} triangles", format_thousands(stats.triangle_count))
    };
    let support = match stats.support_count {
        0 => String::new(),
        1 => "1 support \u{b7} ".to_owned(),
        n => format!("{} supports \u{b7} ", format_thousands(n as u64)),
    };
    format!("{objects} \u{b7} {support}{triangles}")
}

fn format_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut result = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index != 0 && (digits.len() - index) % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result
}

/// Toggle a viewport-only hidden row; it never removes geometry from the print job.
pub(super) fn toggle_hidden(hidden: &mut Vec<usize>, index: usize) {
    if let Some(at) = hidden.iter().position(|i| *i == index) {
        hidden.remove(at);
    } else {
        hidden.push(index);
    }
}

fn leaf_triangle_counts(graph: &Scene) -> Vec<u64> {
    graph
        .reachable_leaves(CsgPolicy::Descend, ReferencePolicy::Skip)
        .unwrap_or_default()
        .into_iter()
        .map(|id| {
            let count = graph
                .node(id)
                .and_then(|node| match &node.kind {
                    NodeKind::Leaf(LeafPayload::Mesh(mesh)) => Some(mesh.faces().len()),
                    NodeKind::Leaf(LeafPayload::MeshResource(id)) => {
                        graph.mesh_resource(*id).map(|m| m.faces().len())
                    }
                    _ => None,
                })
                .unwrap_or(0);
            count as u64
        })
        .collect()
}

pub(super) fn triangle_counts(graph: &Scene) -> Vec<String> {
    let counts = leaf_triangle_counts(graph);
    object_rows(graph)
        .iter()
        .map(|row| {
            crate::i18n::tr!(
                "{} triangles",
                format_thousands(row.leaf_indices.iter().map(|&i| counts[i]).sum())
            )
            .to_string()
        })
        .collect()
}

/// Toggle all geometry in a displayed row together. A partially visible row
/// becomes hidden on the first click, then fully visible on the next.
pub(super) fn toggle_row_visibility(hidden: &mut Vec<usize>, indices: &[usize]) {
    let hide = indices.iter().any(|i| !hidden.contains(i));
    for &i in indices {
        if hide != hidden.contains(&i) {
            toggle_hidden(hidden, i);
        }
    }
}

#[cfg(test)]
#[path = "tests_parts_panel.rs"]
mod tests;
