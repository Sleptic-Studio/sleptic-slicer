// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{
    error::{LoadThreeMfError as E, WriteThreeMfError as WE},
    scene_writer::WriteSceneProgress,
};
use sc_3mf::{
    model::*,
    names::*,
    package::{Package, PartName, Relationship},
};
use sc_mesh_core::{
    IndexedMesh, IndexedTriangle, MeshMetadata, Normal, Vertex,
    transform::{AffineTransform3D, Transform3D},
};
use sc_scene::{LeafPayload, NodeId, NodeKind, Scene, SceneMetadata};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    io::{Cursor, Read, Seek, Write},
};
/// What a 3MF file held that was not loaded as is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadThreeMfWarning {
    /// The build lists nothing to print.
    EmptyBuild,
    /// An object the build never places.
    UnbuiltObjectSkipped { name: Option<String>, id: u32 },
    /// A support, surface or other object loaded as plain geometry.
    NonModelObjectTypeFlattened { object_type: ObjectType, id: u32 },
    /// Slices stored in the file; the full meshes were used instead.
    SliceDataIgnored { stack_count: usize },
    /// Material groups left out (one resin prints everything).
    MaterialsDropped { group_count: usize },
    /// The file requires the materials extension; its colours and
    /// materials were not loaded (a resin print has one material).
    MaterialsExtensionIgnored,
    /// Metadata named with a namespace prefix the model never declares
    /// (other slicers do this); read anyway.
    UndeclaredMetadataPrefix { prefix: String },
    /// A part with no content type was ignored, as the formats ask (often
    /// another program's own data).
    UntypedPartIgnored { part: String },
}

pub const MAX_COMPONENT_DEPTH: usize = 64;
pub use sc_scene::MAX_EXPANDED_LEAF_COUNT;
fn graph_error(e: sc_scene::SceneError) -> E {
    sc_3mf::error::PackageReadError::PartRead {
        part: DEFAULT_MODEL_PART_NAME.into(),
        source: std::io::Error::other(e.to_string()),
    }
    .into()
}
fn transform(m: Option<Matrix3D>, scale: f64) -> Transform3D {
    match m {
        None => Transform3D::Identity,
        Some(m) if m == Matrix3D::IDENTITY => Transform3D::Identity,
        Some(Matrix3D(a)) => Transform3D::Affine(AffineTransform3D::from_linear_and_translation(
            [[a[0], a[3], a[6]], [a[1], a[4], a[7]], [a[2], a[5], a[8]]],
            [a[9] * scale, a[10] * scale, a[11] * scale],
        )),
    }
}
fn matrix(t: &Transform3D) -> Option<Matrix3D> {
    match t {
        Transform3D::Identity => None,
        // 3MF writes the four columns one after the other.
        Transform3D::Affine(a) => Some(Matrix3D(a.columns().concat().try_into().expect("four columns of three"))),
    }
}
pub fn read_threemf(r: &mut (impl Read + Seek)) -> Result<(Scene, Vec<LoadThreeMfWarning>), E> {
    read_threemf_from_package(&Package::read_from(r)?)
}
/// Objects the production extension places in other model parts, brought
/// into `d` under fresh ids, one part at a time and each part once.
struct PartImports<'a> {
    package: &'a Package,
    /// Old id to new id, for each part already brought in.
    done: BTreeMap<String, BTreeMap<ResourceId, ResourceId>>,
    next: u32,
}

impl PartImports<'_> {
    /// The new id of object `requested` from part `path`, bringing the part's
    /// objects into `d` the first time. The part may not point to further
    /// parts. Its own unit attribute is ignored: the root model's unit holds
    /// for the whole build (production extension).
    fn resolve(&mut self, d: &mut ModelDocument, path: &str, requested: ResourceId) -> Result<ResourceId, E> {
        let failure = || E::ProductionReference { path: path.to_owned(), object_id: requested.get() };
        if !self.done.contains_key(path) {
            let name = PartName::new(path).map_err(|_| failure())?;
            let imported = parse_model(self.package.part_data(&name).ok_or_else(failure)?)?;
            let nested = |o: &ObjectResource| matches!(&o.payload, ObjectPayload::Components(cs) if cs.iter().any(|c| c.path.is_some()));
            if imported.objects.iter().any(nested) {
                return Err(failure());
            }
            let mut ids = BTreeMap::new();
            for o in &imported.objects {
                self.next = self.next.checked_add(1).ok_or_else(failure)?;
                ids.insert(o.id, ResourceId::new(self.next).ok_or_else(failure)?);
            }
            for mut o in imported.objects {
                o.id = ids[&o.id];
                if let ObjectPayload::Components(cs) = &mut o.payload {
                    for c in cs {
                        c.object_id = *ids.get(&c.object_id).ok_or_else(failure)?;
                    }
                }
                d.objects.push(o);
            }
            self.done.insert(path.to_owned(), ids);
        }
        self.done[path].get(&requested).copied().ok_or_else(failure)
    }
}

/// Brings in every object that components or build items reference in
/// another model part (`p:path`), and points them at the copies.
fn production(d: &mut ModelDocument, p: &Package) -> Result<(), E> {
    let mut imports = PartImports {
        package: p,
        done: BTreeMap::new(),
        next: d.objects.iter().map(|o| o.id.get()).max().unwrap_or(0),
    };
    for index in 0..d.objects.len() {
        let ObjectPayload::Components(components) = &d.objects[index].payload else { continue };
        let wanted: Vec<(usize, String, ResourceId)> = components
            .iter()
            .enumerate()
            .filter_map(|(at, c)| c.path.clone().map(|path| (at, path, c.object_id)))
            .collect();
        for (at, path, requested) in wanted {
            let new = imports.resolve(d, &path, requested)?;
            if let ObjectPayload::Components(components) = &mut d.objects[index].payload {
                components[at].object_id = new;
                components[at].path = None;
            }
        }
    }
    for at in 0..d.build.len() {
        let Some(path) = d.build[at].path.clone() else { continue };
        let requested = d.build[at].object_id;
        let new = imports.resolve(d, &path, requested)?;
        if d.object(new).is_some_and(|o| o.object_type == ObjectType::Other) {
            return Err(E::Model(sc_3mf::error::ModelParseError::BuildItemReferencesOtherType { id: requested.get() }));
        }
        d.build[at].object_id = new;
        d.build[at].path = None;
    }
    Ok(())
}
pub fn read_threemf_from_package(p: &Package) -> Result<(Scene, Vec<LoadThreeMfWarning>), E> {
    let start = p.start_part()?;
    let ty = p.part_content_type(&start).unwrap_or("");
    if ty != MODEL_PART_CONTENT_TYPE {
        return Err(E::StartPartNotAModel {
            content_type: ty.into(),
        });
    }
    let mut d = parse_model(p.part_data(&start).unwrap_or(&[]))?;
    if d.build.is_empty()
        && p.part_names()
            .chain(p.unlisted_part_names())
            .any(|p| p.as_str().to_ascii_lowercase().ends_with(".gcode"))
    {
        return Err(E::SlicedPrintFile);
    }
    production(&mut d, p)?;
    let mut warnings: Vec<LoadThreeMfWarning> = p
        .warnings()
        .iter()
        .map(|warning| match warning {
            sc_3mf::package::PackageWarning::UnlistedPart { part } => LoadThreeMfWarning::UntypedPartIgnored { part: part.clone() },
        })
        .collect();
    if d.materials_extension_required {
        warnings.push(LoadThreeMfWarning::MaterialsExtensionIgnored);
    }
    warnings.extend(d.undeclared_metadata_prefixes.iter().map(|prefix| LoadThreeMfWarning::UndeclaredMetadataPrefix { prefix: prefix.clone() }));
    if !d.base_materials.is_empty() {
        warnings.push(LoadThreeMfWarning::MaterialsDropped {
            group_count: d.base_materials.len(),
        })
    }
    if !d.slice_stacks.is_empty() {
        warnings.push(LoadThreeMfWarning::SliceDataIgnored {
            stack_count: d.slice_stacks.len(),
        })
    }
    if d.build.is_empty() {
        warnings.push(LoadThreeMfWarning::EmptyBuild)
    }
    let by_id: BTreeMap<_, _> = d.objects.iter().map(|o| (o.id, o)).collect();
    let mut reached = BTreeSet::new();
    let mut leaves = 0;
    fn check(
        id: ResourceId,
        depth: usize,
        by: &BTreeMap<ResourceId, &ObjectResource>,
        seen: &mut BTreeSet<ResourceId>,
        leaves: &mut usize,
        w: &mut Vec<LoadThreeMfWarning>,
    ) -> Result<(), E> {
        if depth > 64 {
            return Err(E::ComponentDepthExceeded { limit: 64 });
        }
        let Some(o) = by.get(&id) else {
            return Err(
                sc_3mf::error::ModelParseError::UnknownObjectReference { id: id.get() }.into(),
            );
        };
        if o.mesh_resolution == MeshResolution::Lowres {
            return Err(E::LowresMeshUnsupported {
                object_id: id.get(),
            });
        }
        if seen.insert(id) && o.object_type != ObjectType::Model {
            w.push(LoadThreeMfWarning::NonModelObjectTypeFlattened {
                id: id.get(),
                object_type: o.object_type,
            })
        }
        match &o.payload {
            ObjectPayload::Mesh(_) => {
                *leaves += 1;
                if *leaves > MAX_EXPANDED_LEAF_COUNT {
                    return Err(E::LeafBudgetExceeded {
                        limit: MAX_EXPANDED_LEAF_COUNT,
                    });
                }
            }
            ObjectPayload::Components(cs) => {
                for c in cs {
                    check(c.object_id, depth + 1, by, seen, leaves, w)?
                }
            }
        }
        Ok(())
    }
    for b in &d.build {
        check(
            b.object_id,
            0,
            &by_id,
            &mut reached,
            &mut leaves,
            &mut warnings,
        )?
    }
    for o in &d.objects {
        if !reached.contains(&o.id) {
            warnings.push(LoadThreeMfWarning::UnbuiltObjectSkipped {
                id: o.id.get(),
                name: o.name.clone(),
            })
        }
    }
    let mut g = Scene::new();
    let root = g.add_group(None, Transform3D::Identity);
    g.add_root(root).map_err(graph_error)?;
    let title = d
        .metadata
        .iter()
        .find(|m| m.name == "Title")
        .map(|m| m.value.clone());
    let comment = d
        .metadata
        .iter()
        .find(|m| m.name == "Description")
        .map(|m| m.value.clone());
    if title.is_some() || comment.is_some() {
        g.set_meta(Some(SceneMetadata {
            name: title,
            comment,
        }));
    }
    let scale = d.unit.scale_to_mm();
    let mut meshes = BTreeMap::new();
    for o in &d.objects {
        if !reached.contains(&o.id) {
            continue;
        }
        if let ObjectPayload::Mesh(m) = &o.payload {
            let mut vertices = Vec::with_capacity(m.vertices.len());
            for v in &m.vertices {
                let a = [
                    (v.x * scale) as f32,
                    (v.y * scale) as f32,
                    (v.z * scale) as f32,
                ];
                if a.iter().any(|v| !v.is_finite()) {
                    return Err(E::NonFiniteVertex {
                        object_id: o.id.get(),
                    });
                }
                vertices.push(Vertex::new(a));
            }
            let faces = m
                .triangles
                .iter()
                .map(|t| {
                    let ids = [t.v1.get(), t.v2.get(), t.v3.get()];
                    let vs = ids.map(|i| vertices[i as usize]);
                    let n = sc_mesh_core::geometry::compute_normal_from_triangle_vertices(&vs)
                        .unwrap_or(Normal::new([0.; 3]));
                    IndexedTriangle::from_raw_indices(n, ids)
                })
                .collect();
            let meta = o.name.clone().map(|name| MeshMetadata {
                name: Some(name),
                header_comment: None,
            });
            meshes.insert(
                o.id,
                g.add_mesh_resource(IndexedMesh::new(meta, vertices, faces)),
            );
        }
    }
    fn add(
        g: &mut Scene,
        id: ResourceId,
        t: Transform3D,
        by: &BTreeMap<ResourceId, &ObjectResource>,
        meshes: &BTreeMap<ResourceId, sc_scene::MeshResourceId>,
        scale: f64,
    ) -> Result<NodeId, E> {
        let o = by[&id];
        match &o.payload {
            ObjectPayload::Mesh(_) => g
                .add_leaf(None, t, LeafPayload::MeshResource(meshes[&id]))
                .map_err(graph_error),
            ObjectPayload::Components(cs) => {
                let n = g.add_group(None, t);
                for c in cs {
                    let child = add(
                        g,
                        c.object_id,
                        transform(c.transform, scale),
                        by,
                        meshes,
                        scale,
                    )?;
                    g.add_child(n, child).map_err(graph_error)?
                }
                Ok(n)
            }
        }
    }
    for b in &d.build {
        let n = add(
            &mut g,
            b.object_id,
            transform(b.transform, scale),
            &by_id,
            &meshes,
            scale,
        )?;
        g.add_child(root, n).map_err(graph_error)?
    }
    Ok((g, warnings))
}
pub fn mesh_to_model_data(m: &IndexedMesh) -> MeshData {
    MeshData {
        vertices: m
            .vertices
            .iter()
            .map(|v| {
                let [x, y, z] = v.coords();
                Vertex3 {
                    x: x as f64,
                    y: y as f64,
                    z: z as f64,
                }
            })
            .collect(),
        triangles: m
            .faces
            .iter()
            .map(|t| TriangleRef {
                v1: ResourceIndex::new(t.vertices[0].get())
                    .unwrap_or(ResourceIndex::new(0).unwrap_or_else(|| unreachable!())),
                v2: ResourceIndex::new(t.vertices[1].get()).unwrap_or_else(|| unreachable!()),
                v3: ResourceIndex::new(t.vertices[2].get()).unwrap_or_else(|| unreachable!()),
            })
            .collect(),
    }
}
struct Export<'a> {
    g: &'a Scene,
    d: ModelDocument,
    meshes: HashMap<usize, ResourceId>,
    nodes: HashMap<NodeId, ResourceId>,
}
impl Export<'_> {
    fn node(&mut self, n: NodeId) -> Result<(ResourceId, Option<Matrix3D>), WE> {
        let node = self
            .g
            .node(n)
            .ok_or(sc_scene::SceneError::DanglingNodeId { id: n })?;
        if let NodeKind::Reference { resource } = node.kind {
            let root = self
                .g
                .subtree_resource_root(resource)
                .ok_or(sc_scene::SceneError::DanglingSubtreeResource { id: resource })?;
            let (id, root_transform) = self.node(root)?;
            return Ok((
                id,
                matrix(&node.local_transform.compose(&transform(root_transform, 1.))),
            ));
        }
        let t = matrix(&node.local_transform);
        if let Some(id) = self.nodes.get(&n) {
            return Ok((*id, t));
        }
        let (payload, name) = match &node.kind {
            NodeKind::Leaf(_) => {
                let arc = self
                    .g
                    .leaf_mesh_arc(n)
                    .ok_or(sc_scene::SceneError::NotALeaf { node_id: n })?;
                let key = std::sync::Arc::as_ptr(arc) as usize;
                if let Some(id) = self.meshes.get(&key) {
                    self.nodes.insert(n, *id);
                    return Ok((*id, t));
                }
                (
                    ObjectPayload::Mesh(mesh_to_model_data(arc)),
                    arc.meta
                        .as_ref()
                        .and_then(|m| m.name.as_deref())
                        .and_then(|s| crate::clean_name(s, false)),
                )
            }
            NodeKind::Group { children } => {
                let mut cs = Vec::with_capacity(children.len());
                for c in children {
                    let (object_id, transform) = self.node(*c)?;
                    cs.push(Component {
                        object_id,
                        transform,
                        path: None,
                    })
                }
                (
                    ObjectPayload::Components(cs),
                    node.meta
                        .as_ref()
                        .and_then(|m| m.name.as_deref())
                        .and_then(|s| crate::clean_name(s, false)),
                )
            }
            NodeKind::Csg { .. } => return Err(WE::CsgUnrepresentable { node_id: n }),
            NodeKind::Reference { .. } => unreachable!(),
        };
        let id = ResourceId::new(self.d.objects.len() as u32 + 1)
            .ok_or_else(|| std::io::Error::other("3MF resource ID space exhausted"))?;
        self.d.objects.push(ObjectResource {
            id,
            object_type: ObjectType::Model,
            name,
            partnumber: None,
            property_ref: None,
            slice_stack: None,
            mesh_resolution: MeshResolution::Fullres,
            payload,
        });
        if let Some(arc) = self.g.leaf_mesh_arc(n) {
            self.meshes.insert(std::sync::Arc::as_ptr(arc) as usize, id);
        }
        self.nodes.insert(n, id);
        Ok((id, t))
    }
}
pub fn scene_to_model_document(g: &Scene) -> Result<ModelDocument, WE> {
    let mut e = Export {
        g,
        d: ModelDocument::default(),
        meshes: HashMap::new(),
        nodes: HashMap::new(),
    };
    if let Some(m) = g.meta() {
        if let Some(title) = m.name.as_deref().and_then(|s| crate::clean_name(s, false)) {
            e.d.metadata.push(Metadata {
                name: "Title".into(),
                value: title,
                preserve: false,
                value_type: None,
            })
        }
        if let Some(c) = &m.comment {
            e.d.metadata.push(Metadata {
                name: "Description".into(),
                value: c.clone(),
                preserve: false,
                value_type: None,
            })
        }
    }
    for root in g.roots() {
        let n = g
            .node(*root)
            .ok_or(sc_scene::SceneError::DanglingNodeId { id: *root })?;
        let ids = if let NodeKind::Group { children } = &n.kind
            && n.meta.is_none()
            && matches!(n.local_transform, Transform3D::Identity)
        {
            children.clone()
        } else {
            vec![*root]
        };
        for id in ids {
            let (object_id, transform) = e.node(id)?;
            e.d.build.push(BuildItem {
                object_id,
                transform,
                partnumber: None,
                path: None,
            });
        }
    }
    Ok(e.d)
}
pub fn write_threemf(g: &Scene, w: &mut impl Write) -> Result<(), WE> {
    write_threemf_with_progress(g, w, &mut |_| {})
}
pub fn write_threemf_with_progress(
    g: &Scene,
    w: &mut impl Write,
    cb: &mut impl FnMut(WriteSceneProgress),
) -> Result<(), WE> {
    let d = scene_to_model_document(g)?;
    let total = d
        .objects
        .iter()
        .map(|o| match &o.payload {
            ObjectPayload::Mesh(m) => 64 * m.vertices.len() as u64 + 48 * m.triangles.len() as u64,
            _ => 0,
        })
        .sum();
    cb(WriteSceneProgress::Serialize { done: 0, total });
    struct Sink<'a, F> {
        b: Vec<u8>,
        last: usize,
        total: u64,
        cb: &'a mut F,
    }
    impl<F: FnMut(WriteSceneProgress)> Write for Sink<'_, F> {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.b.extend_from_slice(b);
            if self.b.len() - self.last >= 262144 {
                self.last = self.b.len();
                (self.cb)(WriteSceneProgress::Serialize {
                    done: self.last as u64,
                    total: self.total,
                });
            }
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut sink = Sink {
        b: Vec::new(),
        last: 0,
        total,
        cb,
    };
    write_model(&d, &mut sink)?;
    let bytes = sink.b;
    cb(WriteSceneProgress::Serialize {
        done: bytes.len() as u64,
        total,
    });
    let mut p = Package::new();
    let n =
        PartName::new(DEFAULT_MODEL_PART_NAME).map_err(|e| std::io::Error::other(e.to_string()))?;
    p.add_part(n, MODEL_PART_CONTENT_TYPE, bytes)?;
    p.add_root_relationship(Relationship { external: false,
        id: "rel-1".into(),
        rel_type: START_PART_REL_TYPE.into(),
        target: DEFAULT_MODEL_PART_NAME.into(),
    });
    let mut out = Cursor::new(Vec::new());
    p.write_to_with_progress(&mut out, &mut |done, total| {
        cb(WriteSceneProgress::Compress { done, total })
    })?;
    w.write_all(out.get_ref())?;
    Ok(())
}
