// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! A model part, from the top down: the document, what it builds, the
//! objects, their meshes and components, and the material references a
//! resin print reads past.
use std::collections::{BTreeMap, BTreeSet};

use super::{Matrix3D, MeshResolution, ObjectType, ResourceId, ResourceIndex, SliceStack, Unit};

/// A whole model part.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ModelDocument {
    /// What gets printed: objects placed on the plate.
    pub build: Vec<BuildItem>,
    /// The resources the build draws on.
    pub objects: Vec<ObjectResource>,
    pub slice_stacks: Vec<SliceStack>,
    pub base_materials: Vec<BaseMaterialsGroup>,
    pub unit: Unit,
    pub metadata: Vec<Metadata>,
    /// Namespaces that metadata names use as prefixes (`vendor:name`), by
    /// prefix, so a written model declares them again.
    pub metadata_namespaces: BTreeMap<String, String>,
    /// Metadata prefixes the model used without declaring them (Bambu Studio
    /// writes `BambuStudio:` names so). Read anyway, reported as a warning,
    /// and declared under a placeholder namespace when written.
    pub undeclared_metadata_prefixes: BTreeSet<String>,
    /// The model requires the materials extension. A resin print has one
    /// material, so its colours and materials are read past (the geometry
    /// is all that matters) and the reader warns.
    pub materials_extension_required: bool,
}

impl ModelDocument {
    /// The object resource with this id.
    pub fn object(&self, id: ResourceId) -> Option<&ObjectResource> {
        self.objects.iter().find(|candidate| candidate.id == id)
    }

    /// The slice stack resource with this id.
    pub fn slice_stack(&self, id: ResourceId) -> Option<&SliceStack> {
        self.slice_stacks
            .iter()
            .find(|candidate| candidate.id == id)
    }
}

/// An object placed on the build plate.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildItem {
    /// The production extension's `p:path`: the model part that holds the
    /// object, when it is not this one. Resolved (and cleared) on reading.
    pub path: Option<String>,
    pub object_id: ResourceId,
    pub transform: Option<Matrix3D>,
    pub partnumber: Option<String>,
}

/// One `<object>` resource.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectResource {
    pub id: ResourceId,
    pub name: Option<String>,
    pub object_type: ObjectType,
    pub payload: ObjectPayload,
    /// Its slices (slice extension), and whether its mesh only stands in
    /// for them.
    pub slice_stack: Option<ResourceId>,
    pub mesh_resolution: MeshResolution,
    pub partnumber: Option<String>,
    /// Its material (materials extension); read past, never used.
    pub property_ref: Option<PropertyRef>,
}

/// What an object is made of: a mesh of its own, or other objects.
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectPayload {
    Components(Vec<Component>),
    Mesh(MeshData),
}

/// Another object placed inside this one.
#[derive(Clone, Debug, PartialEq)]
pub struct Component {
    /// The production extension's `p:path`, as on [`BuildItem::path`].
    pub path: Option<String>,
    pub object_id: ResourceId,
    pub transform: Option<Matrix3D>,
}

/// A triangle mesh: corners, and triangles by the corners' positions.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MeshData {
    pub triangles: Vec<TriangleRef>,
    pub vertices: Vec<Vertex3>,
}

/// A triangle, by its corners' positions in the vertex list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriangleRef {
    pub v1: ResourceIndex,
    pub v2: ResourceIndex,
    pub v3: ResourceIndex,
}

impl TriangleRef {
    /// The three corners in order.
    pub fn corners(self) -> [ResourceIndex; 3] {
        [self.v1, self.v2, self.v3]
    }
}

/// A corner of a mesh, in model units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// One `<metadata>` entry: a name, its value, and the two optional flags
/// the specification gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub name: String,
    pub value_type: Option<String>,
    pub preserve: bool,
    pub value: String,
}

/// An object's material (materials extension): a group, and a position in
/// it. One resin prints every part, so this is read past and never used
/// for colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PropertyRef {
    pub index: ResourceIndex,
    pub group: ResourceId,
}

/// A `<basematerials>` resource and its materials, kept only so a model
/// that has them still reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseMaterialsGroup {
    pub bases: Vec<BaseMaterial>,
    pub id: ResourceId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseMaterial {
    pub display_color: String,
    pub name: String,
}
