// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use rayon::prelude::*;
use std::collections::BTreeSet;
type E = ModelWriteError;
fn planar(m: Matrix3D) -> bool {
    [2, 5, 6, 7].into_iter().all(|i| m.0[i] == 0.) && m.0[8] == 1.
}
fn validate(d: &ModelDocument) -> Result<BTreeSet<ResourceId>, E> {
    for st in &d.slice_stacks {
        if let SliceStackContent::Slices(ss) = &st.content {
            let id = st.id.get();
            if !st.z_bottom.is_finite() {
                return Err(E::SliceZBottomNotFinite { stack_id: id });
            }
            let mut prev = st.z_bottom;
            for (index, s) in ss.iter().enumerate() {
                if !s.z_top.is_finite() {
                    return Err(E::SliceZTopNotFinite {
                        stack_id: id,
                        index,
                    });
                }
                if s.z_top <= prev {
                    return Err(E::SliceZNotIncreasing {
                        stack_id: id,
                        index,
                    });
                }
                prev = s.z_top;
                for p in &s.polygons {
                    if p.segments.is_empty() {
                        return Err(E::PolygonWithoutSegments { stack_id: id });
                    }
                    for i in std::iter::once(p.start).chain(p.segments.iter().map(|v| v.end)) {
                        if i.get() as usize >= s.vertices.len() {
                            return Err(E::SliceIndexOutOfRange {
                                stack_id: id,
                                index: i.get(),
                                vertex_count: s.vertices.len(),
                            });
                        }
                    }
                }
            }
        }
    }
    let mut sliced = BTreeSet::new();
    for o in &d.objects {
        if let Some(id) = o.slice_stack {
            let st = d.slice_stack(id).ok_or(E::UnknownSliceStackReference {
                object_id: o.id.get(),
                stack_id: id.get(),
            })?;
            sliced.insert(o.id);
            if matches!(o.object_type, ObjectType::Model | ObjectType::SolidSupport)
                && let SliceStackContent::Slices(ss) = &st.content
                && ss.iter().any(|s| s.polygons.iter().any(|p| !p.is_closed()))
            {
                return Err(E::OpenPolygonOnModelObject {
                    object_id: o.id.get(),
                    stack_id: id.get(),
                });
            }
        }
        if let ObjectPayload::Components(cs) = &o.payload {
            for c in cs {
                if sliced.contains(&c.object_id) {
                    if c.transform.is_some_and(|m| !planar(m)) {
                        return Err(E::NonPlanarSlicedTransform {
                            object_id: c.object_id.get(),
                        });
                    }
                    sliced.insert(o.id);
                }
            }
        }
    }
    for b in &d.build {
        if sliced.contains(&b.object_id) && b.transform.is_some_and(|m| !planar(m)) {
            return Err(E::NonPlanarSlicedTransform {
                object_id: b.object_id.get(),
            });
        }
    }
    Ok(sliced)
}
pub(crate) fn escaped(s: &str) -> std::borrow::Cow<'_, str> {
    quick_xml::escape::escape(s)
}
struct Xml<W> {
    w: W,
}
// Workers own their buffers; the caller's sink remains single-threaded and need
// not implement Send. Ordered collection preserves exact document bytes.
fn formatted_blocks<T: Sync>(
    records: &[T],
    emit: impl Fn(&mut Xml<Vec<u8>>, &T) -> Result<(), E> + Sync,
) -> Result<Vec<Vec<u8>>, E> {
    records
        .par_chunks(2048)
        .map(|block| {
            let mut x = Xml {
                w: Vec::with_capacity(block.len() * 64),
            };
            for record in block {
                emit(&mut x, record)?;
            }
            Ok(x.w)
        })
        .collect()
}
fn vertex<W: Write>(x: &mut Xml<W>, v: &Vertex3) -> Result<(), E> {
    x.open("vertex")?;
    x.num("x", v.x)?;
    x.num("y", v.y)?;
    x.num("z", v.z)?;
    x.raw("/>")
}
fn triangle<W: Write>(x: &mut Xml<W>, t: &TriangleRef) -> Result<(), E> {
    x.open("triangle")?;
    x.id("v1", t.v1.get())?;
    x.id("v2", t.v2.get())?;
    x.id("v3", t.v3.get())?;
    x.raw("/>")
}
impl<W: Write> Xml<W> {
    fn raw(&mut self, s: &str) -> Result<(), E> {
        self.w.write_all(s.as_bytes())?;
        Ok(())
    }
    fn open(&mut self, n: &str) -> Result<(), E> {
        self.raw("<")?;
        self.raw(n)
    }
    fn attr(&mut self, n: &str, v: &str) -> Result<(), E> {
        self.raw(" ")?;
        self.raw(n)?;
        self.raw("=\"")?;
        self.raw(&escaped(v))?;
        self.raw("\"")
    }
    fn id(&mut self, n: &str, v: u32) -> Result<(), E> {
        self.attr(n, itoa::Buffer::new().format(v))
    }
    fn num(&mut self, n: &str, v: f64) -> Result<(), E> {
        self.attr(n, ryu::Buffer::new().format(v))
    }
    fn end(&mut self, n: &str) -> Result<(), E> {
        self.raw("</")?;
        self.raw(n)?;
        self.raw(">")
    }
    fn transform(&mut self, m: Option<Matrix3D>, sliced: bool) -> Result<(), E> {
        if let Some(mut m) = m {
            if sliced {
                for i in [2, 5, 6, 7] {
                    m.0[i] = 0.
                }
                m.0[8] = 1.;
            }
            self.attr("transform", &m.to_attribute())?
        }
        Ok(())
    }
}

fn slice<W: Write>(x: &mut Xml<W>, s: &Slice) -> Result<(), E> {
    x.open("s:slice")?;
    x.num("ztop", s.z_top)?;
    if s.vertices.is_empty() && s.polygons.is_empty() {
        x.raw("/>")?;
        return Ok(());
    }
    x.raw("><s:vertices>")?;
    for v in &s.vertices {
        x.open("s:vertex")?;
        x.num("x", v.x)?;
        x.num("y", v.y)?;
        x.raw("/>")?
    }
    x.end("s:vertices")?;
    for p in &s.polygons {
        x.open("s:polygon")?;
        x.id("startv", p.start.get())?;
        x.raw(">")?;
        for seg in &p.segments {
            x.open("s:segment")?;
            x.id("v2", seg.end.get())?;
            x.raw("/>")?
        }
        x.end("s:polygon")?
    }
    x.end("s:slice")?;
    Ok(())
}

pub fn write_model<W: Write>(d: &ModelDocument, out: &mut W) -> Result<(), E> {
    let sliced = validate(d)?;
    let mut buffer = std::io::BufWriter::with_capacity(262144, out);
    let mut x = Xml { w: &mut buffer };
    x.raw("<?xml version=\"1.0\" encoding=\"UTF-8\"?>")?;
    x.open("model")?;
    x.attr("unit", d.unit.as_str())?;
    x.attr("xmlns", CORE_NS)?;
    for (prefix, uri) in &d.metadata_namespaces {
        // The slice and production prefixes are declared below as needed.
        if prefix != "s" && prefix != "p" {
            x.attr(&format!("xmlns:{prefix}"), uri)?;
        }
    }
    if !d.slice_stacks.is_empty()
        || d.objects
            .iter()
            .any(|o| o.slice_stack.is_some() || o.mesh_resolution == MeshResolution::Lowres)
    {
        x.attr("xmlns:s", SLICE_NS)?;
        x.attr(
            if d.objects
                .iter()
                .any(|o| o.mesh_resolution == MeshResolution::Lowres)
            {
                "requiredextensions"
            } else {
                "recommendedextensions"
            },
            "s",
        )?;
    }
    x.raw(">")?;
    for m in &d.metadata {
        x.open("metadata")?;
        x.attr("name", &m.name)?;
        if m.preserve {
            x.attr("preserve", "1")?
        }
        if let Some(t) = &m.value_type {
            x.attr("type", t)?
        }
        x.raw(">")?;
        x.raw(&escaped(&m.value))?;
        x.end("metadata")?
    }
    x.raw("<resources>")?;
    for st in &d.slice_stacks {
        x.open("s:slicestack")?;
        x.id("id", st.id.get())?;
        x.attr("zbottom", &st.z_bottom.to_string())?;
        x.raw(">")?;
        match &st.content {
            SliceStackContent::Refs(rs) => {
                for r in rs {
                    x.open("s:sliceref")?;
                    x.id("slicestackid", r.slice_stack.get())?;
                    x.attr("slicepath", &r.slice_path)?;
                    x.raw("/>")?
                }
            }
            SliceStackContent::Slices(ss) => {
                if ss.len() >= 64 {
                    for block in formatted_blocks(ss, slice)? {
                        x.w.write_all(&block)?;
                    }
                } else {
                    for s in ss {
                        slice(&mut x, s)?;
                    }
                }
            }
        }
        x.end("s:slicestack")?
    }
    for g in &d.base_materials {
        x.open("basematerials")?;
        x.id("id", g.id.get())?;
        x.raw(">")?;
        for b in &g.bases {
            x.open("base")?;
            x.attr("name", &b.name)?;
            x.attr("displaycolor", &b.display_color)?;
            x.raw("/>")?
        }
        x.end("basematerials")?
    }
    for o in &d.objects {
        x.open("object")?;
        x.id("id", o.id.get())?;
        if o.object_type != ObjectType::Model {
            x.attr("type", o.object_type.as_str())?
        }
        if let Some(n) = &o.name {
            x.attr("name", n)?
        }
        if let Some(n) = &o.partnumber {
            x.attr("partnumber", n)?
        }
        if let Some(p) = o.property_ref {
            x.id("pid", p.group.get())?;
            x.id("pindex", p.index.get())?
        }
        if let Some(s) = o.slice_stack {
            x.id("s:slicestackid", s.get())?
        }
        if o.mesh_resolution != MeshResolution::Fullres {
            x.attr("s:meshresolution", o.mesh_resolution.as_str())?
        }
        x.raw(">")?;
        match &o.payload {
            ObjectPayload::Mesh(m) => {
                x.raw("<mesh><vertices>")?;
                if m.vertices.len() >= 8192 {
                    for block in formatted_blocks(&m.vertices, vertex)? {
                        x.w.write_all(&block)?;
                    }
                } else {
                    for v in &m.vertices {
                        vertex(&mut x, v)?;
                    }
                }
                x.raw("</vertices><triangles>")?;
                if m.triangles.len() >= 8192 {
                    for block in formatted_blocks(&m.triangles, triangle)? {
                        x.w.write_all(&block)?;
                    }
                } else {
                    for t in &m.triangles {
                        triangle(&mut x, t)?;
                    }
                }
                x.raw("</triangles></mesh>")?
            }
            ObjectPayload::Components(cs) => {
                x.raw("<components>")?;
                for c in cs {
                    x.open("component")?;
                    x.id("objectid", c.object_id.get())?;
                    x.transform(c.transform, sliced.contains(&c.object_id))?;
                    x.raw("/>")?
                }
                x.end("components")?
            }
        }
        x.end("object")?
    }
    x.raw("</resources><build>")?;
    for b in &d.build {
        x.open("item")?;
        x.id("objectid", b.object_id.get())?;
        x.transform(b.transform, sliced.contains(&b.object_id))?;
        if let Some(p) = &b.partnumber {
            x.attr("partnumber", p)?
        }
        x.raw("/>")?
    }
    x.raw("</build></model>")?;
    // Drain our buffer without invoking the caller's flush method.
    buffer.into_inner().map_err(|e| E::Io(e.into_error()))?;
    Ok(())
}
