// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! What a file's load warnings say. They go to stderr until the app has a
//! place to show them.
use std::path::Path;

use sc_mesh_formats::messages::{
    LoadMeshWarning, LoadObjWarning, LoadStlWarning, LoadThreeMfWarning, SceneLoadWarning,
};
use sc_project_formats::{ForeignDialect, NamedProfile, ProfileRefWarning, ReadProjectWarning};

/// A load warning in words, and whether it is only a note (something the
/// file itself makes up for) rather than a warning.
pub(crate) trait Explain {
    fn explain(&self) -> String;
    fn is_note(&self) -> bool {
        false
    }
}

/// Writes every warning of a loaded file to stderr, each prefixed with its
/// level and the file.
pub(crate) fn log_load_warnings(
    path: &Path,
    mesh: &[LoadMeshWarning],
    project: &[ReadProjectWarning],
    profiles: &[ProfileRefWarning],
) {
    let shown = path.display();
    let all = mesh.iter().map(|w| w as &dyn Explain)
        .chain(project.iter().map(|w| w as &dyn Explain))
        .chain(profiles.iter().map(|w| w as &dyn Explain));
    for warning in all {
        let level = if warning.is_note() { "NOTE" } else { "WARNING" };
        eprintln!("{level} [{shown}]: {}", warning.explain());
    }
}

impl Explain for LoadMeshWarning {
    fn explain(&self) -> String {
        match self {
            LoadMeshWarning::Obj(w) => w.explain(),
            LoadMeshWarning::Stl(w) => w.explain(),
            LoadMeshWarning::ThreeMf(w) => w.explain(),
            LoadMeshWarning::Scene(w) => w.explain(),
        }
    }
}

impl Explain for LoadStlWarning {
    fn explain(&self) -> String {
    match self {
        LoadStlWarning::HeaderNotText => {
            "binary STL header is not readable text; no mesh name was taken from it".to_string()
        }
    }
}
}

impl Explain for LoadObjWarning {
    fn explain(&self) -> String {
    use LoadObjWarning as W;
    let (line, what) = match self {
        W::NoVertices => return "OBJ file has no vertices".to_string(),
        W::NoFaces => return "OBJ file has no faces".to_string(),
        W::SkippedTextureCoordinates { line } => (line, "texture coordinates".to_string()),
        W::SkippedVertexNormal { line } => (line, "vertex normal".to_string()),
        W::SkippedGroup { line, name } => (line, format!("group '{name}'")),
        W::SkippedSmoothingGroup { line } => (line, "smoothing group".to_string()),
        W::SkippedMaterialReference { line, material } => {
            (line, format!("material reference '{material}'"))
        }
        W::SkippedMaterialLibrary { line, path } => (line, format!("material library '{path}'")),
        W::SkippedComment { line } => (line, "comment".to_string()),
        W::SkippedUnknownDirective { line, directive } => {
            (line, format!("OBJ directive '{directive}'"))
        }
    };
    format!("line {line}: skipped {what}")
}
}

impl Explain for LoadThreeMfWarning {
    fn explain(&self) -> String {
    match self {
        LoadThreeMfWarning::MaterialsDropped { group_count } => {
            format!("dropped {group_count} material group(s); materials are not supported yet")
        }
        LoadThreeMfWarning::UnbuiltObjectSkipped { id, name } => {
            let object = match name {
                Some(name) => format!("'{name}' (id {id})"),
                None => format!("id {id}"),
            };
            format!("skipped object {object}: it is not referenced by the build")
        }
        LoadThreeMfWarning::NonModelObjectTypeFlattened { id, object_type } => format!(
            "object {id} has type '{}'; it was loaded as plain model geometry",
            object_type.as_str()
        ),
        LoadThreeMfWarning::SliceDataIgnored { stack_count } => format!(
            "ignored {stack_count} slice stack(s); the full-resolution meshes were loaded instead"
        ),
        LoadThreeMfWarning::EmptyBuild => "the 3MF build section lists no items".to_string(),
        LoadThreeMfWarning::MaterialsExtensionIgnored => {
            "colours and materials were not loaded; a resin print uses one material".to_string()
        }
        LoadThreeMfWarning::UndeclaredMetadataPrefix { prefix } => {
            format!("metadata uses the prefix '{prefix}:' without declaring it (other slicers do); read anyway")
        }
        LoadThreeMfWarning::UntypedPartIgnored { part } => {
            format!("ignored part '{part}': the package gives it no content type (often another program's own data)")
        }
    }
}
}

impl Explain for SceneLoadWarning {
    fn explain(&self) -> String {
    let SceneLoadWarning::SharedVertices { leaves } = self;
    let names: Vec<String> = leaves
        .iter()
        .map(|leaf| match &leaf.object_name {
            Some(name) => format!("'{name}'"),
            None => format!("node {}", leaf.node_id.get()),
        })
        .collect();
    format!(
        "objects share at least one vertex coordinate: {}",
        names.join(", ")
    )
}
}

impl Explain for ReadProjectWarning {
    fn explain(&self) -> String {
    match self {
        ReadProjectWarning::ThreeMf(w) => w.explain(),
        ReadProjectWarning::NewerProjectData { version } => format!(
            "the file carries project data with version {version}, which this build does not understand; the project settings were ignored"
        ),
        ReadProjectWarning::UnknownProfileFile { part_name } => {
            format!("ignored an unrecognized part in the embedded-profiles area: {part_name}")
        }
        ReadProjectWarning::OtherSlicersData { dialect } => format!(
            "the file carries project data written by {}; only the model data was loaded",
            dialect_name(*dialect)
        ),
    }
}
}

fn dialect_name(dialect: ForeignDialect) -> &'static str {
    match dialect {
        ForeignDialect::PrusaSlicer => "PrusaSlicer",
        ForeignDialect::BambuStudioOrca => "Bambu Studio / OrcaSlicer",
        ForeignDialect::Cura => "Cura",
    }
}

/// A profile the project names but that is not installed. It is only a
/// note when the file carries a copy of it.
impl Explain for ProfileRefWarning {
    fn explain(&self) -> String {
        let (kind, id) = match &self.profile {
            NamedProfile::Printer(id) => ("printer", id.to_string()),
            NamedProfile::Resin(id) => ("resin", id.to_string()),
        };
        let copy = match self.embedded {
            true => "which is not installed; the file carries an embedded copy",
            false => "which is not installed and has no embedded copy in the file",
        };
        format!("the project references {kind} profile \"{id}\", {copy}")
    }

    fn is_note(&self) -> bool {
        self.embedded
    }
}

#[cfg(test)]
#[path = "tests_load_warnings.rs"]
mod tests;
