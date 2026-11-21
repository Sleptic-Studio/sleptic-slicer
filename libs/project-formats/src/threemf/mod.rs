// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
mod metadata;
mod settings;
use crate::{
    error::{ReadThreeMfProjectError, WriteThreeMfProjectError},
    *,
};
use sc_3mf::{
    model::*,
    package::{Package, PartName, Relationship},
};
use std::io::{Read, Seek, Write};
const MODEL: &str = "application/vnd.ms-package.3dmanufacturing-3dmodel+xml";
const START: &str = "http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel";
const PRESERVE: &str = "http://schemas.openxmlformats.org/package/2006/relationships/mustpreserve";
const PROJECT: &str = "/Metadata/sleptic-project.toml";
/// Where projects saved before R187 keep their settings; still read.
const OLD_PROJECT: &str = "/Metadata/soulcrafted-project.toml";
const SLICES: &str = "/3D/slices.model";
pub use crate::slices::SliceMappingError;
fn part(name: &str) -> PartName {
    // All callers supply format-defined ASCII names or validated profile IDs.
    match PartName::new(name) {
        Ok(p) => p,
        Err(_) => unreachable!("format-defined part name"),
    }
}
fn data<'a>(p: &'a Package, name: &str) -> Option<&'a [u8]> {
    p.part_data(&part(name))
}
fn add(
    p: &mut Package,
    name: &str,
    ty: &str,
    bytes: Vec<u8>,
    relationship: Option<&str>,
) -> Result<(), WriteThreeMfProjectError> {
    p.add_part(part(name), ty, bytes)?;
    if let Some(ty) = relationship {
        rel(p, name, ty)
    }
    if name != "/3D/3dmodel.model" && name != "/Metadata/thumbnail.png" {
        rel(p, name, PRESERVE)
    }
    Ok(())
}
fn rel(p: &mut Package, name: &str, ty: &str) {
    let id = format!(
        "rel-{}",
        p.root_relationships()
            .iter()
            .filter(|r| r.id != "rThumbnail")
            .count()
            + 1
    );
    p.add_root_relationship(Relationship { external: false,
        id,
        rel_type: ty.into(),
        target: name.into(),
    });
}
fn estimate(m: &ModelDocument) -> u64 {
    let mut n = 0;
    for o in &m.objects {
        if let ObjectPayload::Mesh(m) = &o.payload {
            n += m.vertices.len() as u64 * 64 + m.triangles.len() as u64 * 48;
        }
    }
    for s in &m.slice_stacks {
        if let SliceStackContent::Slices(s) = &s.content {
            for s in s {
                n += s.vertices.len() as u64 * 40
                    + s.polygons
                        .iter()
                        .map(|p| p.segments.len() as u64 * 24)
                        .sum::<u64>();
            }
        }
    }
    n
}
struct Serialization<'a, F> {
    bytes: Vec<u8>,
    done: &'a mut u64,
    total: u64,
    reported: u64,
    progress: &'a mut F,
}
impl<F: FnMut(WriteProgress)> Write for Serialization<'_, F> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.bytes.extend_from_slice(b);
        *self.done += b.len() as u64;
        if *self.done - self.reported >= 256 * 1024 {
            (self.progress)(WriteProgress::Serialize {
                done: *self.done,
                total: self.total,
            });
            self.reported = *self.done;
        }
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn serialize(
    m: &ModelDocument,
    done: &mut u64,
    total: u64,
    progress: &mut impl FnMut(WriteProgress),
) -> Result<Vec<u8>, WriteThreeMfProjectError> {
    let reported = *done;
    let mut w = Serialization {
        bytes: vec![],
        done,
        total,
        reported,
        progress,
    };
    write_model(m, &mut w)?;
    (w.progress)(WriteProgress::Serialize {
        done: *w.done,
        total,
    });
    Ok(w.bytes)
}
pub fn write_threemf_project(
    s: &sc_scene::Scene,
    p: &ProjectPayload,
    e: EmbedProfiles,
    l: Option<&PlateSlices>,
    fp: Option<&str>,
    w: &mut impl Write,
) -> Result<(), WriteThreeMfProjectError> {
    write_threemf_project_with_progress(s, p, e, l, fp, w, &mut |_| {})
}
pub fn write_threemf_project_with_progress(
    s: &sc_scene::Scene,
    p: &ProjectPayload,
    e: EmbedProfiles,
    l: Option<&PlateSlices>,
    fp: Option<&str>,
    w: &mut impl Write,
    progress: &mut impl FnMut(WriteProgress),
) -> Result<(), WriteThreeMfProjectError> {
    let model = sc_mesh_formats::threemf::scene_to_model_document(s)?;
    let slices = l.map(slices::to_model).transpose()?;
    let total = estimate(&model) + slices.as_ref().map(estimate).unwrap_or(0);
    let mut done = 0;
    let model_bytes = serialize(&model, &mut done, total, progress)?;
    let slice_bytes = slices
        .as_ref()
        .map(|m| serialize(m, &mut done, total, progress))
        .transpose()?;
    let mut package = Package::new();
    if let Ok(Some(mesh)) = s.merge_meshes() {
        let rgb = sc_mesh_formats::preview::render_mesh(&mesh, 256, 256, [242; 3]);
        let mut bytes = vec![];
        let mut encoder = png::Encoder::new(&mut bytes, 256, 256);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
        writer
            .write_image_data(&rgb)
            .map_err(std::io::Error::other)?;
        writer.finish().map_err(std::io::Error::other)?;
        package.add_part(part("/Metadata/thumbnail.png"), "image/png", bytes)?;
        package.add_root_relationship(Relationship { external: false,
            id: "rThumbnail".into(),
            rel_type:
                "http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail"
                    .into(),
            target: "/Metadata/thumbnail.png".into(),
        });
    }
    add(
        &mut package,
        "/3D/3dmodel.model",
        MODEL,
        model_bytes,
        Some(START),
    )?;
    add(
        &mut package,
        PROJECT,
        "application/vnd.sleptic.project+toml",
        settings::encode(p, fp)?,
        Some("urn:sleptic:slicer:2026:project"),
    )?;
    metadata::save(s, &mut package)?;
    if let Some(bytes) = slice_bytes {
        add(
            &mut package,
            SLICES,
            MODEL,
            bytes,
            Some("urn:sleptic:slicer:2026:slices"),
        )?;
    }
    if e == EmbedProfiles::Yes {
        for e in &p.embedded {
            let name = format!("/Metadata/profiles/{}.toml", settings::embedded_path(e));
            let bytes = &e.bytes;
            add(&mut package, &name, "application/toml", bytes.clone(), None)?;
        }
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    package.write_to_with_progress(&mut bytes, |done, total| {
        progress(WriteProgress::Compress { done, total })
    })?;
    w.write_all(bytes.get_ref())?;
    Ok(())
}
pub fn read_threemf_project(
    r: &mut (impl Read + Seek),
) -> Result<LoadedProject, ReadThreeMfProjectError> {
    read_threemf_project_with_progress(r, |_| {})
}
pub fn read_threemf_project_with_progress(
    r: &mut (impl Read + Seek),
    progress: impl FnMut(sc_3mf::package::DecompressionProgress),
) -> Result<LoadedProject, ReadThreeMfProjectError> {
    let package = Package::read_from_with_progress(r, progress)?;
    let (mut scene, w) = sc_mesh_formats::threemf::read_threemf_from_package(&package)?;
    let mut warnings: Vec<_> = w.into_iter().map(ReadProjectWarning::ThreeMf).collect();
    let mut seen = vec![];
    for n in package.part_names().chain(package.unlisted_part_names()) {
        let dialect = match n.as_str() {
            "/Metadata/Slic3r_PE.config" => Some(ForeignDialect::PrusaSlicer),
            "/Metadata/project_settings.config" => Some(ForeignDialect::BambuStudioOrca),
            s if s.starts_with("/Cura/") => Some(ForeignDialect::Cura),
            _ => None,
        };
        if let Some(dialect) = dialect
            && !seen.contains(&dialect)
        {
            seen.push(dialect);
            warnings.push(ReadProjectWarning::OtherSlicersData { dialect });
        }
    }
    let (mut payload, slice_fingerprint) = if let Some(bytes) = data(&package, PROJECT).or_else(|| data(&package, OLD_PROJECT)) {
        settings::decode(bytes, &mut warnings)?
    } else {
        (None, None)
    };
    if let Some(p) = &mut payload {
        for n in package.part_names() {
            let name = n.as_str();
            if let Some(tail) = name.strip_prefix("/Metadata/profiles/") {
                if let Some(e) = settings::embedded(tail, package.part_data(n).unwrap_or_default())
                {
                    p.embedded.push(e)
                } else {
                    warnings.push(ReadProjectWarning::UnknownProfileFile {
                        part_name: name.into(),
                    })
                }
            }
        }
    }
    metadata::restore(&mut scene, &package);
    let slices = data(&package, SLICES)
        .and_then(|b| parse_model(b).ok())
        .and_then(|m| slices::from_model(&m).ok());
    Ok(LoadedProject {
        scene,
        warnings,
        payload,
        slices,
        slice_fingerprint,
    })
}
