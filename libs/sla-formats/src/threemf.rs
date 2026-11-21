// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{Format, WriterError, WriterOptions};
use sc_3mf::{
    model::*,
    package::{Package, PartName, Relationship},
};
use std::path::Path;
const MODEL: &str = "application/vnd.ms-package.3dmanufacturing-3dmodel+xml";
const START: &str = "http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel";
fn part(s: &str) -> Result<PartName, WriterError> {
    PartName::new(s).map_err(|e| crate::error::encoding(Format::ThreeMf, e))
}
fn id(i: usize) -> Result<ResourceId, WriterError> {
    u32::try_from(i)
        .ok()
        .and_then(ResourceId::new)
        .ok_or_else(|| crate::error::encoding(Format::ThreeMf, "resource ID exceeds 3MF range"))
}
fn index(i: usize) -> Result<ResourceIndex, WriterError> {
    u32::try_from(i)
        .ok()
        .and_then(ResourceIndex::new)
        .ok_or_else(|| crate::error::encoding(Format::ThreeMf, "vertex index exceeds 3MF range"))
}
pub fn write_scene_slices(
    scene: &sc_scene::Scene,
    leaves: &[sc_sl1cer::LeafSlices],
    p: &Path,
    o: &WriterOptions,
) -> Result<(), WriterError> {
    if leaves.is_empty() {
        return Err(WriterError::NoModelsToWrite);
    }
    let n = o.layer_schedule().total_layers() as usize;
    for l in leaves {
        if l.stack().layers().len() != n {
            return Err(WriterError::LayerGridMismatch {
                leaf: l.leaf(),
                schedule_layers: n,
                stack_layers: l.stack().layers().len(),
            });
        }
    }
    crate::write::validate(p, Format::ThreeMf)?;
    let mut package = Package::new();
    let mut model = ModelDocument::default();
    if let Some(m) = scene.meta() {
        for (name, value) in [("Title", &m.name), ("Description", &m.comment)] {
            if let Some(value) = value {
                model.metadata.push(Metadata {
                    name: name.into(),
                    value: value.clone(),
                    preserve: false,
                    value_type: None,
                });
            }
        }
    }
    let main = part("/3D/3dmodel.model")?;
    for (i, l) in leaves.iter().enumerate() {
        let ordinal = i + 1;
        let stack_id = id(2 * ordinal - 1)?;
        let object_id = id(2 * ordinal)?;
        let path = format!("/2D/slices-{ordinal}.model");
        let mesh =
            sc_scene::bake_leaf_to_world(scene, l.leaf()).map_err(WriterError::SceneBakeFailed)?;
        model.slice_stacks.push(SliceStack {
            id: stack_id,
            z_bottom: 0.0,
            content: SliceStackContent::Refs(vec![SliceRef {
                slice_stack: ResourceId::MIN,
                slice_path: path.clone(),
            }]),
        });
        model.objects.push(ObjectResource {
            id: object_id,
            object_type: ObjectType::Model,
            name: l.leaf_name().map(str::to_owned),
            partnumber: None,
            property_ref: None,
            slice_stack: Some(stack_id),
            mesh_resolution: MeshResolution::Fullres,
            payload: ObjectPayload::Mesh(sc_mesh_formats::threemf::mesh_to_model_data(&mesh)),
        });
        model.build.push(BuildItem {
            object_id,
            transform: None,
            partnumber: None,
            path: None,
        });
        let mut slices = Vec::with_capacity(n);
        for (layer, top) in l
            .stack()
            .layers()
            .iter()
            .zip(o.layer_schedule().iter_slab_tops())
        {
            let mut s = Slice {
                z_top: top,
                vertices: vec![],
                polygons: vec![],
            };
            for p in layer.polygons() {
                for r in std::iter::once(p.outer()).chain(p.holes()) {
                    if r.is_empty() {
                        continue;
                    }
                    let start = index(s.vertices.len())?;
                    let mut segments = vec![];
                    for (k, v) in r.vertices().iter().enumerate() {
                        let ix = index(s.vertices.len())?;
                        s.vertices.push(SliceVertex { x: v.x, y: v.y });
                        if k > 0 {
                            segments.push(SliceSegment { end: ix })
                        }
                    }
                    segments.push(SliceSegment { end: start });
                    s.polygons.push(SlicePolygon { start, segments });
                }
            }
            slices.push(s);
        }
        let doc = ModelDocument {
            slice_stacks: vec![SliceStack {
                id: ResourceId::MIN,
                z_bottom: 0.0,
                content: SliceStackContent::Slices(slices),
            }],
            ..Default::default()
        };
        let mut bytes = vec![];
        write_model(&doc, &mut bytes)?;
        package.add_part(part(&path)?, MODEL, bytes)?;
        package.add_part_relationship(
            &main,
            Relationship { external: false,
                id: format!("rel-{ordinal}"),
                rel_type: START.into(),
                target: path,
            },
        );
    }
    let mut bytes = vec![];
    write_model(&model, &mut bytes)?;
    package.add_part(main, MODEL, bytes)?;
    package.add_root_relationship(Relationship { external: false,
        id: "rel-1".into(),
        rel_type: START.into(),
        target: "/3D/3dmodel.model".into(),
    });
    if let Some(mesh) = scene.merge_meshes().map_err(WriterError::SceneBakeFailed)? {
        let rgb = sc_mesh_formats::preview::render_mesh(&mesh, 256, 256, [242; 3]);
        let mut bytes = vec![];
        crate::debug::png::encode(&mut bytes, 256, 256, true, &rgb, vec![], Format::ThreeMf)?;
        package.add_part(part("/Metadata/thumbnail.png")?, "image/png", bytes)?;
        package.add_root_relationship(Relationship { external: false,
            id: "rel-thumbnail".into(),
            rel_type:
                "http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail"
                    .into(),
            target: "/Metadata/thumbnail.png".into(),
        });
    }
    package.write_to(std::fs::File::create(p)?)?;
    Ok(())
}
