// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use quick_xml::{NsReader, events::Event, name::ResolveResult};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
type E = ModelParseError;
#[derive(Debug)]
struct Tag {
    name: String,
    ns: String,
    attrs: BTreeMap<String, String>,
    empty: bool,
}
impl Tag {
    fn req(&self, e: &'static str, a: &'static str) -> Result<&str, E> {
        self.attrs
            .get(a)
            .map(String::as_str)
            .ok_or(E::MissingRequiredAttribute {
                element: e,
                attribute: a,
            })
    }
    fn num(&self, e: &'static str, a: &'static str) -> Result<f64, E> {
        let s = self.req(e, a)?;
        s.trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| E::InvalidNumber {
                element: e,
                attribute: a,
                value: s.into(),
            })
    }
    fn id(&self, e: &'static str, a: &'static str) -> Result<ResourceId, E> {
        let s = self.req(e, a)?;
        s.trim()
            .parse()
            .ok()
            .and_then(ResourceId::new)
            .ok_or_else(|| E::InvalidResourceId { value: s.into() })
    }
    fn idx(&self, e: &'static str, a: &'static str) -> Result<ResourceIndex, E> {
        let s = self.req(e, a)?;
        s.trim()
            .parse()
            .ok()
            .and_then(ResourceIndex::new)
            .ok_or_else(|| E::InvalidNumber {
                element: e,
                attribute: a,
                value: s.into(),
            })
    }
    fn matrix(&self) -> Result<Option<Matrix3D>, E> {
        self.attrs
            .get("transform")
            .map(|v| Matrix3D::parse(v).ok_or_else(|| E::InvalidMatrix { value: v.clone() }))
            .transpose()
    }
}
struct Parser<'a> {
    r: NsReader<&'a [u8]>,
    doc: ModelDocument,
    ids: BTreeSet<ResourceId>,
}
impl Parser<'_> {
    fn tag(&self, e: &quick_xml::events::BytesStart<'_>, empty: bool) -> Result<Tag, E> {
        let (ns, local) = self.r.resolver().resolve_element(e.name());
        let ns = match ns {
            ResolveResult::Bound(v) => String::from_utf8_lossy(v.as_ref()).into_owned(),
            _ => String::new(),
        };
        let mut attrs = BTreeMap::new();
        for a in e.attributes() {
            let a = a?;
            let (ans, al) = self.r.resolver().resolve_attribute(a.key);
            let key = match ans {
                ResolveResult::Bound(n) if n.as_ref() == SLICE_NS.as_bytes() => {
                    format!("s:{}", String::from_utf8_lossy(al.as_ref()))
                }
                ResolveResult::Bound(n) if n.as_ref() == PRODUCTION_NS.as_bytes() => {
                    format!("p:{}", String::from_utf8_lossy(al.as_ref()))
                }
                _ => String::from_utf8_lossy(a.key.as_ref()).into_owned(),
            };
            // Normalize literal whitespace before expanding references (XML 1.0).
            let val = a
                .decoded_and_normalized_value(quick_xml::XmlVersion::Explicit1_0, self.r.decoder())?
                .into_owned();
            attrs.insert(key, val);
        }
        let path_allowed = ns == CORE_NS && (local.as_ref() == b"component" || local.as_ref() == b"item");
        if attrs.contains_key("p:path") && !path_allowed {
            return Err(E::ProductionPathUnsupported);
        }
        Ok(Tag {
            name: String::from_utf8_lossy(local.as_ref()).into_owned(),
            ns,
            attrs,
            empty,
        })
    }
    fn next(&mut self) -> Result<Option<Tag>, E> {
        loop {
            match self.r.read_event()? {
                Event::Start(e) => return self.tag(&e, false).map(Some),
                Event::Empty(e) => return self.tag(&e, true).map(Some),
                Event::End(_) | Event::Eof => return Ok(None),
                Event::DocType(_) => return Err(E::DtdForbidden),
                _ => {}
            }
        }
    }
    fn skip(&mut self, t: &Tag) -> Result<(), E> {
        if !t.empty {
            let mut depth = 1;
            while depth > 0 {
                match self.r.read_event()? {
                    Event::Start(_) => depth += 1,
                    Event::End(_) => depth -= 1,
                    Event::Eof => break,
                    _ => {}
                }
            }
        }
        Ok(())
    }
    fn unique(&mut self, id: ResourceId) -> Result<(), E> {
        if !self.ids.insert(id) {
            Err(E::DuplicateResourceId { id: id.get() })
        } else {
            Ok(())
        }
    }
    fn metadata(&mut self, t: &Tag) -> Result<Metadata, E> {
        let mut v = String::new();
        if !t.empty {
            loop {
                match self.r.read_event()? {
                    Event::End(_) | Event::Eof => break,
                    Event::Text(x) => v.push_str(
                        &x.xml_content(quick_xml::XmlVersion::Explicit1_0)
                            .map_err(quick_xml::Error::from)?,
                    ),
                    Event::CData(x) => v.push_str(
                        &x.xml_content(quick_xml::XmlVersion::Explicit1_0)
                            .map_err(quick_xml::Error::from)?,
                    ),
                    Event::GeneralRef(x) => {
                        let n = x.decode().map_err(quick_xml::Error::from)?;
                        if let Some(c) = x.resolve_char_ref()? {
                            v.push(c)
                        } else if let Some(s) = quick_xml::escape::resolve_predefined_entity(&n) {
                            v.push_str(s)
                        } else {
                            return Err(E::UndeclaredEntity {
                                name: n.into_owned(),
                            });
                        }
                    }
                    Event::Start(e) => {
                        let t = self.tag(&e, false)?;
                        self.skip(&t)?
                    }
                    _ => {}
                }
            }
        }
        Ok(Metadata {
            name: t.req("metadata", "name")?.into(),
            value: v,
            preserve: t
                .attrs
                .get("preserve")
                .is_some_and(|v| v == "1" || v == "true"),
            value_type: t.attrs.get("type").cloned(),
        })
    }
    fn mesh(&mut self, t: &Tag, id: u32) -> Result<MeshData, E> {
        let mut m = MeshData::default();
        if t.empty {
            return Ok(m);
        }
        while let Some(c) = self.next()? {
            if c.ns == CORE_NS && (c.name == "vertices" || c.name == "triangles") {
                if !c.empty {
                    self.mesh_records(&mut m, id, c.name == "vertices")?;
                }
            } else {
                self.skip(&c)?
            }
        }
        Ok(m)
    }
    // Geometry dominates large files. Borrow attribute values from XML instead
    // of constructing a map and owned strings for every coordinate and index.
    fn mesh_records(&mut self, m: &mut MeshData, id: u32, vertices: bool) -> Result<(), E> {
        if matches!(self.r.resolver().resolve_element(quick_xml::name::QName(b"vertex")).0,
            ResolveResult::Bound(n) if n.as_ref() == CORE_NS.as_bytes())
        {
            let close: &[u8] = if vertices {
                b"</vertices>"
            } else {
                b"</triangles>"
            };
            let remaining = *self.r.get_ref();
            if let Some(end) = memchr::memmem::find(remaining, close)
                && end >= 65536
                && let Some(parsed) =
                    regular_geometry(&remaining[..end], vertices, m.vertices.len())
            {
                let name = if vertices {
                    b"vertices".as_slice()
                } else {
                    b"triangles".as_slice()
                };
                self.r.read_to_end(quick_xml::name::QName(name))?;
                m.vertices.extend(parsed.vertices);
                m.triangles.extend(parsed.triangles);
                return Ok(());
            }
        }
        loop {
            let event = self.r.read_event()?;
            let empty = matches!(event, Event::Empty(_));
            let e = match event {
                Event::Start(e) | Event::Empty(e) => e,
                Event::End(_) | Event::Eof => break,
                Event::DocType(_) => return Err(E::DtdForbidden),
                _ => continue,
            };
            let (ns, local) = self.r.resolver().resolve_element(e.name());
            let vertex = local.as_ref() == b"vertex";
            if !matches!(ns,ResolveResult::Bound(n) if n.as_ref()==CORE_NS.as_bytes())
                || (if vertices {
                    !vertex
                } else {
                    local.as_ref() != b"triangle"
                })
            {
                let t = self.tag(&e, empty)?;
                self.skip(&t)?;
                continue;
            }
            let element = if vertex { "vertex" } else { "triangle" };
            let keys = if vertex {
                ["x", "y", "z"]
            } else {
                ["v1", "v2", "v3"]
            };
            let mut attrs: [Option<std::borrow::Cow<'_, str>>; 3] = [None, None, None];
            for a in e.attributes() {
                let a = a?;
                let (ns, local) = self.r.resolver().resolve_attribute(a.key);
                if matches!(ns,ResolveResult::Bound(n) if n.as_ref()==PRODUCTION_NS.as_bytes())
                    && local.as_ref() == b"path"
                {
                    return Err(E::ProductionPathUnsupported);
                }
                if let Some(i) = keys.iter().position(|k| a.key.as_ref() == k.as_bytes()) {
                    attrs[i] = Some(a.decoded_and_normalized_value(
                        quick_xml::XmlVersion::Explicit1_0,
                        self.r.decoder(),
                    )?);
                }
            }
            let mut values = [""; 3];
            for i in 0..3 {
                values[i] = attrs[i].as_deref().ok_or(E::MissingRequiredAttribute {
                    element,
                    attribute: keys[i],
                })?;
            }
            if vertex {
                let mut xyz = [0.; 3];
                for i in 0..3 {
                    xyz[i] = values[i]
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite())
                        .ok_or_else(|| E::InvalidNumber {
                            element,
                            attribute: keys[i],
                            value: values[i].into(),
                        })?;
                }
                m.vertices.push(Vertex3 {
                    x: xyz[0],
                    y: xyz[1],
                    z: xyz[2],
                });
            } else {
                let mut indices = [ResourceIndex::ZERO; 3];
                for i in 0..3 {
                    indices[i] = values[i]
                        .trim()
                        .parse()
                        .ok()
                        .and_then(ResourceIndex::new)
                        .ok_or_else(|| E::InvalidNumber {
                            element,
                            attribute: keys[i],
                            value: values[i].into(),
                        })?;
                }
                for index in indices {
                    if index.get() as usize >= m.vertices.len() {
                        return Err(E::TriangleIndexOutOfRange {
                            object_id: id,
                            index: index.get(),
                            vertex_count: m.vertices.len(),
                        });
                    }
                }
                m.triangles.push(TriangleRef {
                    v1: indices[0],
                    v2: indices[1],
                    v3: indices[2],
                });
            }
            if !empty {
                let t = Tag {
                    name: element.into(),
                    ns: CORE_NS.into(),
                    attrs: BTreeMap::new(),
                    empty: false,
                };
                self.skip(&t)?;
            }
        }
        Ok(())
    }
    fn object(&mut self, t: &Tag) -> Result<ObjectResource, E> {
        let id = t.id("object", "id")?;
        self.unique(id)?;
        let object_type = t
            .attrs
            .get("type")
            .map(|s| ObjectType::parse(s).ok_or_else(|| E::InvalidObjectType { value: s.clone() }))
            .transpose()?
            .unwrap_or_default();
        let property_ref = if t.attrs.contains_key("pid") && t.attrs.contains_key("pindex") {
            Some(PropertyRef {
                group: t.id("object", "pid")?,
                index: t.idx("object", "pindex")?,
            })
        } else {
            None
        };
        let slice_stack = if t.attrs.contains_key("s:slicestackid") {
            let s = t.id("object", "s:slicestackid")?;
            if self.doc.slice_stack(s).is_none() {
                return Err(E::UnknownSliceStackReference {
                    object_id: id.get(),
                    stack_id: s.get(),
                });
            }
            Some(s)
        } else {
            None
        };
        let mesh_resolution = t
            .attrs
            .get("s:meshresolution")
            .map(|s| {
                MeshResolution::parse(s)
                    .ok_or_else(|| E::InvalidMeshResolution { value: s.clone() })
            })
            .transpose()?
            .unwrap_or_default();
        let mut payload = None;
        if !t.empty {
            while let Some(c) = self.next()? {
                if c.ns == CORE_NS && (c.name == "mesh" || c.name == "components") {
                    if payload.is_some() {
                        return Err(E::DuplicateGeometry { id: id.get() });
                    }
                    // Conformance N_*_0424: properties belong to meshes; an
                    // object made of components has none of its own.
                    if c.name == "components" && t.attrs.contains_key("pid") {
                        let what = format!("object {} has components and also a property (pid)", id.get());
                        return Err(E::Forbidden { what });
                    }
                    payload = Some(if c.name == "mesh" {
                        ObjectPayload::Mesh(self.mesh(&c, id.get())?)
                    } else {
                        let mut cs = Vec::new();
                        if !c.empty {
                            while let Some(v) = self.next()? {
                                if v.ns == CORE_NS && v.name == "component" {
                                    let oid = v.id("component", "objectid")?;
                                    let path = v.attrs.get("p:path").cloned();
                                    if path.is_none() && self.doc.object(oid).is_none() {
                                        return Err(E::UnknownObjectReference { id: oid.get() });
                                    }
                                    cs.push(Component {
                                        object_id: oid,
                                        transform: v.matrix()?,
                                        path,
                                    });
                                }
                                self.skip(&v)?
                            }
                        }
                        ObjectPayload::Components(cs)
                    });
                } else {
                    self.skip(&c)?
                }
            }
        }
        Ok(ObjectResource {
            id,
            object_type,
            name: t.attrs.get("name").cloned(),
            partnumber: t.attrs.get("partnumber").cloned(),
            property_ref,
            slice_stack,
            mesh_resolution,
            payload: payload.ok_or(E::ObjectWithoutGeometry { id: id.get() })?,
        })
    }
    fn slice(&mut self, t: &Tag, sid: u32) -> Result<Slice, E> {
        let mut s = Slice {
            z_top: t.num("slice", "ztop")?,
            vertices: Vec::new(),
            polygons: Vec::new(),
        };
        if t.empty {
            return Ok(s);
        }
        while let Some(c) = self.next()? {
            if c.ns != SLICE_NS {
                self.skip(&c)?;
                continue;
            }
            match c.name.as_str() {
                "vertices" => {
                    if !c.empty {
                        while let Some(v) = self.next()? {
                            if v.ns == SLICE_NS && v.name == "vertex" {
                                s.vertices.push(SliceVertex {
                                    x: v.num("vertex", "x")?,
                                    y: v.num("vertex", "y")?,
                                });
                            }
                            self.skip(&v)?
                        }
                    }
                }
                "polygon" => {
                    let start = c.idx("polygon", "startv")?;
                    let mut p = SlicePolygon {
                        start,
                        segments: Vec::new(),
                    };
                    if !c.empty {
                        while let Some(v) = self.next()? {
                            if v.ns == SLICE_NS && v.name == "segment" {
                                p.segments.push(SliceSegment {
                                    end: v.idx("segment", "v2")?,
                                });
                            }
                            self.skip(&v)?
                        }
                    }
                    if p.segments.is_empty() {
                        return Err(E::PolygonWithoutSegments { stack_id: sid });
                    }
                    for i in std::iter::once(p.start).chain(p.segments.iter().map(|v| v.end)) {
                        if i.get() as usize >= s.vertices.len() {
                            return Err(E::SliceIndexOutOfRange {
                                stack_id: sid,
                                index: i.get(),
                                vertex_count: s.vertices.len(),
                            });
                        }
                    }
                    s.polygons.push(p)
                }
                _ => self.skip(&c)?,
            }
        }
        Ok(s)
    }
    fn stack(&mut self, t: &Tag) -> Result<SliceStack, E> {
        let id = t.id("slicestack", "id")?;
        self.unique(id)?;
        let z_bottom = if t.attrs.contains_key("zbottom") {
            t.num("slicestack", "zbottom")?
        } else {
            0.
        };
        let mut slices = Vec::new();
        let mut refs = Vec::new();
        if !t.empty {
            while let Some(c) = self.next()? {
                if c.ns == SLICE_NS {
                    match c.name.as_str() {
                        "slice" => {
                            slices.push(self.slice(&c, id.get())?);
                            continue;
                        }
                        "sliceref" => refs.push(SliceRef {
                            slice_stack: c.id("sliceref", "slicestackid")?,
                            slice_path: c.req("sliceref", "slicepath")?.into(),
                        }),
                        _ => {}
                    }
                }
                self.skip(&c)?
            }
        }
        if !slices.is_empty() && !refs.is_empty() {
            return Err(E::SliceStackMixesSlicesAndRefs { id: id.get() });
        }
        Ok(SliceStack {
            id,
            z_bottom,
            content: if refs.is_empty() {
                SliceStackContent::Slices(slices)
            } else {
                SliceStackContent::Refs(refs)
            },
        })
    }
    fn resources(&mut self, t: &Tag) -> Result<(), E> {
        if t.empty {
            return Ok(());
        }
        while let Some(c) = self.next()? {
            if c.ns == CORE_NS && c.name == "object" {
                let o = self.object(&c)?;
                self.doc.objects.push(o)
            } else if c.ns == SLICE_NS && c.name == "slicestack" {
                let s = self.stack(&c)?;
                self.doc.slice_stacks.push(s)
            } else if c.ns == CORE_NS && c.name == "basematerials" {
                let id = c.id("basematerials", "id")?;
                self.unique(id)?;
                let mut bases = Vec::new();
                if !c.empty {
                    while let Some(b) = self.next()? {
                        if b.ns == CORE_NS && b.name == "base" {
                            bases.push(BaseMaterial {
                                name: b.req("base", "name")?.into(),
                                display_color: b.req("base", "displaycolor")?.into(),
                            });
                        }
                        self.skip(&b)?
                    }
                }
                self.doc
                    .base_materials
                    .push(BaseMaterialsGroup { id, bases });
            } else {
                self.skip(&c)?
            }
        }
        Ok(())
    }
}
// A conservative fast path for large self-closing numeric record lists. Any
// extension, namespace declaration, nested element, or error uses the general
// reader, which owns all compatibility and first-error decisions.
fn regular_geometry(bytes: &[u8], vertices: bool, vertex_count: usize) -> Option<MeshData> {
    let prefix: &[u8] = if vertices { b"<vertex " } else { b"<triangle " };
    let mut records = Vec::new();
    let mut rest = bytes;
    while !rest.trim_ascii().is_empty() {
        rest = rest.trim_ascii_start();
        if !rest.starts_with(prefix) {
            return None;
        }
        let end = memchr::memmem::find(rest, b"/>")? + 2;
        let record = &rest[..end];
        if record[1..record.len() - 2].contains(&b'<') {
            return None;
        }
        records.push(record);
        rest = &rest[end..];
    }
    let blocks: Vec<Option<MeshData>> = records
        .par_chunks(2048)
        .map(|records| {
            let mut result = MeshData::default();
            for record in records {
                let mut reader = quick_xml::Reader::from_reader(*record);
                let Event::Empty(tag) = reader.read_event().ok()? else {
                    return None;
                };
                let keys: [&[u8]; 3] = if vertices {
                    [b"x", b"y", b"z"]
                } else {
                    [b"v1", b"v2", b"v3"]
                };
                let mut xyz = [0.; 3];
                let mut indices = [ResourceIndex::ZERO; 3];
                let mut seen = [false; 3];
                for a in tag.attributes() {
                    let a = a.ok()?;
                    let i = keys.iter().position(|k| *k == a.key.as_ref())?;
                    if seen[i] || a.value.iter().any(|b| !b.is_ascii() || *b == b'&') {
                        return None;
                    }
                    seen[i] = true;
                    let value = std::str::from_utf8(&a.value).ok()?.trim();
                    if vertices {
                        xyz[i] = value.parse::<f64>().ok().filter(|v| v.is_finite())?;
                    } else {
                        indices[i] = ResourceIndex::new(value.parse().ok()?)?;
                        if indices[i].get() as usize >= vertex_count {
                            return None;
                        }
                    }
                }
                if !seen.into_iter().all(|v| v) {
                    return None;
                }
                if vertices {
                    result.vertices.push(Vertex3 {
                        x: xyz[0],
                        y: xyz[1],
                        z: xyz[2],
                    });
                } else {
                    result.triangles.push(TriangleRef {
                        v1: indices[0],
                        v2: indices[1],
                        v3: indices[2],
                    });
                }
            }
            Some(result)
        })
        .collect();
    let mut result = MeshData::default();
    for block in blocks {
        let block = block?;
        result.vertices.extend(block.vertices);
        result.triangles.extend(block.triangles);
    }
    Some(result)
}
pub fn parse_model(bytes: &[u8]) -> Result<ModelDocument, E> {
    let mut p = Parser {
        r: NsReader::from_reader(bytes),
        doc: ModelDocument::default(),
        ids: BTreeSet::new(),
    };
    let root = p.next()?.ok_or(E::MissingModelElement)?;
    if root.ns != CORE_NS || root.name != "model" {
        return Err(E::UnexpectedRootElement { found: root.name });
    }
    // Conformance N_*_0409: the model element takes no xml:space.
    if root.attrs.contains_key("xml:space") {
        return Err(E::Forbidden { what: "the model element has an xml:space attribute".into() });
    }
    if let Some(u) = root.attrs.get("unit") {
        p.doc.unit = Unit::parse(u).ok_or_else(|| E::InvalidUnit { value: u.clone() })?;
    }
    if let Some(req) = root.attrs.get("requiredextensions") {
        for prefix in req.split_whitespace() {
            let ns = root.attrs.get(&format!("xmlns:{prefix}")).ok_or_else(|| {
                E::UnknownRequiredExtensionPrefix {
                    prefix: prefix.into(),
                }
            })?;
            if ns == MATERIALS_NS {
                p.doc.materials_extension_required = true;
                continue;
            }
            if ![CORE_NS, SLICE_NS, PRODUCTION_NS].contains(&ns.as_str()) {
                return Err(E::UnsupportedRequiredExtension {
                    namespace: ns.clone(),
                });
            }
        }
    }
    if !root.empty {
        while let Some(t) = p.next()? {
            if t.ns == CORE_NS {
                match t.name.as_str() {
                    "metadata" => {
                        let m = p.metadata(&t)?;
                        // A prefixed name needs its namespace declared (conformance
                        // N_*_0410_01), but other slicers leave it out
                        // (`BambuStudio:3mfVersion`): the meaning is plain, so it
                        // is read with a warning. A repeated name (N_*_0410_03)
                        // is let through too, the first one counting: it only
                        // decides which title is shown.
                        if let Some((prefix, _)) = m.name.split_once(':') {
                            let uri = match root.attrs.get(&format!("xmlns:{prefix}")) {
                                Some(uri) => uri.clone(),
                                None => {
                                    p.doc.undeclared_metadata_prefixes.insert(prefix.to_owned());
                                    format!("urn:sleptic:undeclared:{prefix}")
                                }
                            };
                            p.doc.metadata_namespaces.insert(prefix.to_owned(), uri);
                        }
                        p.doc.metadata.push(m);
                        continue;
                    }
                    "resources" => {
                        p.resources(&t)?;
                        continue;
                    }
                    "build" => {
                        if !t.empty {
                            while let Some(b) = p.next()? {
                                if b.ns == CORE_NS && b.name == "item" {
                                    let id = b.id("item", "objectid")?;
                                    let path = b.attrs.get("p:path").cloned();
                                    // An object in another part is checked when
                                    // that part is read (production extension).
                                    if path.is_none() {
                                        let o = p
                                            .doc
                                            .object(id)
                                            .ok_or(E::UnknownObjectReference { id: id.get() })?;
                                        if o.object_type == ObjectType::Other {
                                            return Err(E::BuildItemReferencesOtherType {
                                                id: id.get(),
                                            });
                                        }
                                    }
                                    p.doc.build.push(BuildItem {
                                        object_id: id,
                                        transform: b.matrix()?,
                                        partnumber: b.attrs.get("partnumber").cloned(),
                                        path,
                                    });
                                }
                                p.skip(&b)?
                            }
                        }
                        continue;
                    }
                    _ => {}
                }
            }
            p.skip(&t)?
        }
    }
    Ok(p.doc)
}
