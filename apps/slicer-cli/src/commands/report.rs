// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Every line the mesh commands print about a file: what the reader skipped,
//! what a diagnosis found and what a repair did. The wording is part of the
//! command-line interface (scripts match on it), so it lives in one place.
use std::fmt::Display;
use std::io::Write;

use sc_mesh_core::indexed_mesh::{FaceDiagnosis, MeshDiagnosis, MeshFix, RepairRefusal};
use sc_mesh_formats::messages::{
    LoadMeshWarning, LoadObjWarning, LoadStlWarning, LoadThreeMfWarning, SceneLoadWarning,
};
use sc_scene::repair::SceneRepairOutcome;

/// One line per reader warning. A closed output is not worth failing a
/// command for, so write errors are dropped here and everywhere below.
pub(crate) fn display_warnings(warnings: &[LoadMeshWarning], out: &mut dyn Write) {
    for warning in warnings {
        let text = match warning {
            LoadMeshWarning::Stl(LoadStlWarning::HeaderNotText) => String::from(
                "WARNING: binary STL header is not readable text; no mesh name was taken from it",
            ),
            LoadMeshWarning::Obj(obj) => obj_text(obj),
            LoadMeshWarning::ThreeMf(threemf) => threemf_text(threemf),
            LoadMeshWarning::Scene(SceneLoadWarning::SharedVertices { leaves }) => {
                let mut names = Vec::with_capacity(leaves.len());
                for leaf in leaves {
                    names.push(match leaf.object_name.as_deref() {
                        Some(name) => format!("'{name}'"),
                        None => format!("node {}", leaf.node_id.get()),
                    });
                }
                format!(
                    "WARNING: Objects share at least one vertex coordinate: {}",
                    names.join(", ")
                )
            }
        };
        let _ = writeln!(out, "{text}");
    }
}

pub(crate) fn display_threemf_warning(warning: &LoadThreeMfWarning, out: &mut dyn Write) {
    let _ = writeln!(out, "{}", threemf_text(warning));
}

fn obj_text(warning: &LoadObjWarning) -> String {
    use LoadObjWarning as W;
    let (line, what) = match warning {
        W::NoVertices => return String::from("WARNING: Loaded OBJ file has no vertices"),
        W::NoFaces => return String::from("WARNING: Loaded OBJ file has no faces"),
        W::SkippedTextureCoordinates { line } => {
            (line, String::from("skipped texture coordinates"))
        }
        W::SkippedVertexNormal { line } => (line, String::from("skipped vertex normal")),
        W::SkippedGroup { line, name } => (line, format!("Skipped group '{name}'")),
        W::SkippedSmoothingGroup { line } => (line, String::from("Skipped smoothing group")),
        W::SkippedMaterialReference { line, material } => {
            (line, format!("Skipped material reference '{material}'"))
        }
        W::SkippedMaterialLibrary { line, path } => {
            (line, format!("Skipped material library '{path}'"))
        }
        W::SkippedComment { line } => (line, String::from("Skipped comment")),
        W::SkippedUnknownDirective { line, directive } => {
            (line, format!("Skipped OBJ directive '{directive}'"))
        }
    };
    format!("WARNING (line {line}): {what}")
}

fn threemf_text(warning: &LoadThreeMfWarning) -> String {
    use LoadThreeMfWarning as W;
    let body = match warning {
        W::MaterialsDropped { group_count } => {
            format!("Dropped {group_count} material group(s); materials are not supported yet")
        }
        W::UnbuiltObjectSkipped { id, name } => {
            let object = match name {
                Some(name) => format!("'{name}' (id {id})"),
                None => format!("id {id}"),
            };
            format!("Skipped object {object}: it is not referenced by the build")
        }
        W::NonModelObjectTypeFlattened { id, object_type } => format!(
            "Object {id} has type '{}'; it was loaded as plain model geometry",
            object_type.as_str()
        ),
        W::SliceDataIgnored { stack_count } => format!(
            "Ignored {stack_count} slice stack(s); the full-resolution meshes were loaded instead"
        ),
        W::EmptyBuild => String::from("The 3MF build section lists no items"),
        W::MaterialsExtensionIgnored => {
            String::from("Colours and materials were not loaded; a resin print uses one material")
        }
        W::UndeclaredMetadataPrefix { prefix } => {
            format!("Metadata uses the prefix '{prefix}:' without declaring it (other slicers do); read anyway")
        }
        W::UntypedPartIgnored { part } => {
            format!("Ignored part '{part}': the package gives it no content type (often another program's own data)")
        }
    };
    format!("WARNING: {body}")
}

/// A titled, counted, tab-indented list.
fn listing<T: Display>(out: &mut dyn Write, title: &str, items: &[T]) {
    let _ = writeln!(out, "{title} ({}):", items.len());
    for item in items {
        let _ = writeln!(out, "\t- {item}");
    }
}

/// Lists that have entries only. Normals and edges can be judged only on
/// valid faces, so a mesh shows either its face errors or those two lists.
pub(crate) fn show_diagnosis(diagnosis: &MeshDiagnosis, out: &mut dyn Write) {
    if !diagnosis.vertex_errors.is_empty() {
        listing(out, "Vertex errors", &diagnosis.vertex_errors);
    }
    match &diagnosis.faces {
        FaceDiagnosis::Invalid(faces) => listing(out, "Face errors", faces),
        FaceDiagnosis::Valid {
            normal_vector_errors: normals,
            edge_manifold_errors: edges,
        } => {
            if !normals.is_empty() {
                listing(out, "Normal vector errors", normals);
            }
            if !edges.is_empty() {
                listing(out, "Manifold errors", edges);
            }
        }
    }
}

/// A warning for each mesh the repair left alone, numbered when the scene has
/// more than one mesh; returns how many there were.
pub(crate) fn show_refusal_warnings(outcomes: &[SceneRepairOutcome], out: &mut dyn Write) -> usize {
    let mut count = 0;
    for (position, refusal) in outcomes
        .iter()
        .enumerate()
        .filter_map(|(at, outcome)| Some((at + 1, outcome.refusal.as_ref()?)))
    {
        count += 1;
        let reason = match refusal {
            RepairRefusal::InvalidVertices(errors) => {
                listing(out, "Vertex errors", errors);
                String::from("Unable to fix mesh, there are invalid vertices.")
            }
            RepairRefusal::UnfixableFaceErrors(_) => String::from(
                "Unable to fix mesh. Face errors correction is not implemented (yet).",
            ),
            RepairRefusal::OrientationFixFailed(cause) => format!("Unable to fix mesh: {cause}"),
        };
        let _ = if outcomes.len() > 1 {
            writeln!(
                out,
                "warning (mesh {position} of {}): {reason}",
                outcomes.len()
            )
        } else {
            writeln!(out, "warning: {reason}")
        };
    }
    count
}

pub(crate) fn show_applied_fixes(fixes: &[MeshFix], out: &mut dyn Write) {
    let lines: Vec<String> = fixes
        .iter()
        .map(|fix| match fix {
            MeshFix::RemovedZeroLengthEdgeFace { face_idx } => {
                format!("Removed face with ID '{face_idx}' (had zero-length edge)")
            }
            MeshFix::FlippedFaceOrientation { face_idx } => {
                format!("Flipped orientation of face with ID '{face_idx}'")
            }
            MeshFix::RecomputedNormal {
                face_idx,
                old_normal: old,
                new_normal: new,
            } => format!(
                "Recomputed normal vector for face with ID '{face_idx}', from {:?} (with norm {}) to {:?} (with norm {})",
                old.coords(),
                old.norm(),
                new.coords(),
                new.norm()
            ),
            MeshFix::RemovedDuplicateFace { face_idx } => {
                format!("Removed duplicate face with ID '{face_idx}'")
            }
            MeshFix::DetachedSharedVertex { vertex } => {
                format!("Gave a separate shell its own copy of vertex '{vertex}'")
            }
        })
        .collect();
    listing(out, "Applied fixes", &lines);
}

#[cfg(test)]
#[path = "tests_report.rs"]
mod tests;
