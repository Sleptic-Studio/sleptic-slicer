// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{error::*, names::*};
use rayon::prelude::*;
use std::{
    collections::BTreeMap,
    io::{Read, Seek, Write},
};
/// A part's name, an absolute path in the package. OPC treats names that
/// differ only in ASCII case as the same part, so equality and order here
/// ignore ASCII case; the name keeps the spelling it was written with.
#[derive(Clone, Debug)]
pub struct PartName(String);
impl PartName {
    fn folded(&self) -> impl Iterator<Item = u8> + '_ {
        self.0.bytes().map(|b| b.to_ascii_lowercase())
    }
}
impl PartialEq for PartName {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}
impl Eq for PartName {}
impl PartialOrd for PartName {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for PartName {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.folded().cmp(other.folded())
    }
}
impl PartName {
    pub fn new(s: &str) -> Result<Self, MalformedPartNameError> {
        use MalformedPartNameError::*;
        if s.is_empty() {
            return Err(Empty);
        }
        if !s.starts_with('/') {
            return Err(NotAbsolute);
        }
        if s[1..].split('/').any(str::is_empty) {
            return Err(EmptySegment);
        }
        if s.ends_with('.') {
            return Err(TrailingDot);
        }
        Ok(Self(s.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Display for PartName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Relationship {
    pub id: String,
    pub rel_type: String,
    pub target: String,
    /// `TargetMode="External"`: the target is outside the package (a web
    /// address, say), never a part of it.
    pub external: bool,
}

/// Something a package does that the formats allow or that is harmless,
/// but worth reporting (the reader goes on).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageWarning {
    /// A part `[Content_Types].xml` gives no type: other programs' extra
    /// files often are. The formats say to ignore it, and it is.
    UnlistedPart { part: String },
}
impl std::fmt::Display for PackageWarning {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnlistedPart { part } => {
                write!(out, "part \"{part}\" has no content type, so it was ignored (often another program's own data)")
            }
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecompressionProgress {
    pub decompressed_bytes: u64,
    pub total_bytes: u64,
}
#[derive(Debug, Default)]
pub struct Package {
    parts: Vec<(PartName, String, Vec<u8>)>,
    root: Vec<Relationship>,
    rels: BTreeMap<PartName, Vec<Relationship>>,
    /// Parts with no content type, left out of `parts` (see
    /// [`PackageWarning::UnlistedPart`]).
    unlisted: Vec<PartName>,
    warnings: Vec<PackageWarning>,
}

/// The largest decompressed part, and the largest package in total.
const PART_CAP: u64 = 2 * 1024 * 1024 * 1024;
const PACKAGE_CAP: u64 = 4 * 1024 * 1024 * 1024;
/// `[Content_Types].xml` and relationship parts are parsed whole into
/// small records; far beyond what any real package needs, so a crafted one
/// cannot make the reader hold gigabytes.
const METADATA_CAP: u64 = 16 * 1024 * 1024;

/// `name` as an absolute part name (archive entry names have no leading
/// slash, though some writers add one).
fn absolute(name: &str) -> String {
    if name.starts_with('/') { name.to_owned() } else { ["/", name].concat() }
}

/// `[Content_Types].xml` and relationship parts, which are parsed whole.
fn is_metadata(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "/[content_types].xml" || lower.ends_with(".rels")
}

/// A piece of an interleaved part: `<part>/[n].piece`, the last one
/// `<part>/[n].last.piece` (case does not matter). The part's name, the
/// piece's number, and whether it is the last.
fn piece_of(name: &str) -> Option<(&str, u64, bool)> {
    let (part, file) = name.rsplit_once('/')?;
    let lower = file.to_ascii_lowercase();
    let (number, last) = if let Some(n) = lower.strip_suffix("].last.piece") {
        (n, true)
    } else {
        (lower.strip_suffix("].piece")?, false)
    };
    let digits = number.strip_prefix('[')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) || (digits.len() > 1 && digits.starts_with('0')) {
        return None;
    }
    Some((part, digits.parse().ok()?, last))
}
fn escape(s: &str) -> String {
    quick_xml::escape::escape(s).into_owned()
}
fn ext(s: &str) -> Option<String> {
    let seg = s.rsplit('/').next()?;
    let (stem, e) = seg.rsplit_once('.')?;
    if stem.is_empty() {
        None
    } else {
        Some(e.to_ascii_lowercase())
    }
}
const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>";
fn relxml(rs: &[Relationship]) -> Vec<u8> {
    let mut s = format!("{DECL}<Relationships xmlns=\"{RELATIONSHIPS_NS}\">");
    for r in rs {
        let mode = if r.external { " TargetMode=\"External\"" } else { "" };
        s.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"{mode}/>",
            escape(&r.id),
            escape(&r.rel_type),
            escape(&r.target)
        ))
    }
    s.push_str("</Relationships>");
    s.into_bytes()
}
type XmlEntry = (String, BTreeMap<String, String>);
fn entries(bytes: &[u8]) -> Result<Vec<XmlEntry>, quick_xml::Error> {
    use quick_xml::events::Event;
    let mut r = quick_xml::Reader::from_reader(bytes);
    let mut out = Vec::new();
    loop {
        match r.read_event()? {
            Event::Start(t) | Event::Empty(t) => {
                let name = String::from_utf8_lossy(t.local_name().as_ref()).into_owned();
                let mut a = BTreeMap::new();
                for at in t.attributes() {
                    let at = at.map_err(quick_xml::Error::from)?;
                    a.insert(
                        String::from_utf8_lossy(at.key.local_name().as_ref()).into_owned(),
                        at.decoded_and_normalized_value(
                            quick_xml::XmlVersion::Explicit1_0,
                            r.decoder(),
                        )?
                        .into_owned(),
                    );
                }
                out.push((name, a));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}
// Independent raw DEFLATE blocks end in a byte-aligned sync flush. None has
// BFINAL set; one final empty block terminates the concatenated stream. A small
// single-entry ZIP lets ZipWriter copy these bytes while owning the directory
// bookkeeping for the complete package.
fn parallel_entry(name: &str, data: &[u8]) -> Result<Vec<u8>, PackageWriteError> {
    let blocks: Result<Vec<_>, std::io::Error> = data
        .par_chunks(1024 * 1024)
        .map(|chunk| {
            let mut compressor = flate2::Compress::new(flate2::Compression::new(6), false);
            let mut out = Vec::with_capacity(chunk.len() + chunk.len() / 8 + 1024);
            compressor
                .compress_vec(chunk, &mut out, flate2::FlushCompress::Sync)
                .map_err(std::io::Error::other)?;
            if compressor.total_in() != chunk.len() as u64 {
                return Err(std::io::Error::other("incomplete DEFLATE block"));
            }
            Ok(out)
        })
        .collect();
    let mut compressed = Vec::new();
    for block in blocks? {
        compressed.extend(block);
    }
    compressed.extend([3, 0]);
    let size = u32::try_from(data.len()).map_err(std::io::Error::other)?;
    let csize = u32::try_from(compressed.len()).map_err(std::io::Error::other)?;
    let nlen = u16::try_from(name.len()).map_err(std::io::Error::other)?;
    let crc = crc32fast::hash(data);
    let mut zip = Vec::with_capacity(compressed.len() + name.len() * 2 + 100);
    fn word(b: &mut Vec<u8>, v: u16) {
        b.extend(v.to_le_bytes());
    }
    fn dword(b: &mut Vec<u8>, v: u32) {
        b.extend(v.to_le_bytes());
    }
    dword(&mut zip, 0x04034b50);
    for v in [20, 0x800, 8, 0, 33] {
        word(&mut zip, v);
    }
    for v in [crc, csize, size] {
        dword(&mut zip, v);
    }
    word(&mut zip, nlen);
    word(&mut zip, 0);
    zip.extend(name.as_bytes());
    zip.extend(compressed);
    let offset = u32::try_from(zip.len()).map_err(std::io::Error::other)?;
    dword(&mut zip, 0x02014b50);
    for v in [20, 20, 0x800, 8, 0, 33] {
        word(&mut zip, v);
    }
    for v in [crc, csize, size] {
        dword(&mut zip, v);
    }
    for v in [nlen, 0, 0, 0, 0] {
        word(&mut zip, v);
    }
    dword(&mut zip, 0);
    dword(&mut zip, 0);
    zip.extend(name.as_bytes());
    let directory_size = zip.len() as u32 - offset;
    dword(&mut zip, 0x06054b50);
    for v in [0, 0, 1, 1] {
        word(&mut zip, v);
    }
    dword(&mut zip, directory_size);
    dword(&mut zip, offset);
    word(&mut zip, 0);
    Ok(zip)
}
impl Package {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add_part(
        &mut self,
        name: PartName,
        content_type: &str,
        data: Vec<u8>,
    ) -> Result<(), PackageWriteError> {
        if self.parts.iter().any(|p| p.0 == name) {
            return Err(PackageWriteError::DuplicatePartName { name: name.0 });
        }
        self.parts.push((name, content_type.into(), data));
        Ok(())
    }
    pub fn add_root_relationship(&mut self, r: Relationship) {
        self.root.push(r)
    }
    pub fn add_part_relationship(&mut self, p: &PartName, r: Relationship) {
        self.rels.entry(p.clone()).or_default().push(r)
    }
    pub fn part_names(&self) -> impl Iterator<Item = &PartName> {
        self.parts.iter().map(|p| &p.0)
    }
    pub fn part_content_type(&self, n: &PartName) -> Option<&str> {
        self.parts.iter().find(|p| &p.0 == n).map(|p| p.1.as_str())
    }
    pub fn part_data(&self, n: &PartName) -> Option<&[u8]> {
        self.parts
            .iter()
            .find(|p| &p.0 == n)
            .map(|p| p.2.as_slice())
    }
    pub fn root_relationships(&self) -> &[Relationship] {
        &self.root
    }
    pub fn part_relationships(&self, n: &PartName) -> &[Relationship] {
        self.rels.get(n).map(Vec::as_slice).unwrap_or(&[])
    }
    /// Parts the reader ignored because no content type covers them
    /// (another program's extra files, typically).
    pub fn unlisted_part_names(&self) -> impl Iterator<Item = &PartName> {
        self.unlisted.iter()
    }
    /// What the reader noticed but let pass.
    pub fn warnings(&self) -> &[PackageWarning] {
        &self.warnings
    }
    /// The model part the package's StartPart relationship names. Exactly
    /// one such relationship, internal, may exist: with two, or one that
    /// points outside the package, programs could open different models.
    pub fn start_part(&self) -> Result<PartName, PackageReadError> {
        let mut starts = self.root.iter().filter(|relationship| relationship.rel_type == START_PART_REL_TYPE);
        let start = starts.next().ok_or(PackageReadError::MissingStartPartRelationship)?;
        if starts.next().is_some() {
            return Err(PackageReadError::SeveralStartParts);
        }
        let target = start.target.as_str();
        if start.external {
            return Err(PackageReadError::ExternalStartPart { target: target.to_owned() });
        }
        let absolute = absolute(target);
        let part = PartName::new(&absolute)
            .map_err(|source| PackageReadError::MalformedPartName { name: absolute, source })?;
        if self.part_data(&part).is_some() {
            return Ok(part);
        }
        if let Some(untyped) = self.unlisted.iter().find(|name| **name == part) {
            return Err(PackageReadError::PartWithoutContentType { part: untyped.as_str().to_owned() });
        }
        Err(PackageReadError::StartPartNotFound { target: target.to_owned() })
    }
    pub fn read_from<R: Read + Seek>(r: R) -> Result<Self, PackageReadError> {
        Self::read_from_with_progress(r, &mut |_| {})
    }
    /// Reads a package. The archive's structure is checked before anything
    /// is unpacked (`zip_check`), then every name and size, and only then
    /// are parts decompressed.
    pub fn read_from_with_progress<R: Read + Seek>(
        mut reader: R,
        mut progress: impl FnMut(DecompressionProgress),
    ) -> Result<Self, PackageReadError> {
        let mut bytes = Vec::new();
        reader.seek(std::io::SeekFrom::Start(0)).map_err(PackageReadError::ArchiveRead)?;
        reader.read_to_end(&mut bytes).map_err(PackageReadError::ArchiveRead)?;
        let listing = crate::zip_check::check_zip_structure(&bytes).map_err(PackageReadError::Structure)?;

        // Names and sizes, before decompressing anything.
        let mut wanted = Vec::new();
        let mut total = 0u64;
        for (index, entry) in listing.iter().enumerate() {
            if entry.name.ends_with('/') {
                continue;
            }
            let name = absolute(&entry.name);
            let part = PartName::new(&name).map_err(|source| PackageReadError::MalformedPartName { name: name.clone(), source })?;
            if entry.uncompressed_size > PART_CAP {
                return Err(PackageReadError::OversizedPart { part: name, cap_bytes: PART_CAP });
            }
            if is_metadata(&name) && entry.uncompressed_size > METADATA_CAP {
                return Err(PackageReadError::MetadataPartTooLarge { part: name, cap_bytes: METADATA_CAP });
            }
            total = total.saturating_add(entry.uncompressed_size);
            wanted.push((index, part, entry.uncompressed_size));
        }
        if total > PACKAGE_CAP {
            return Err(PackageReadError::OversizedPackage { cap_bytes: PACKAGE_CAP });
        }
        // Pieces of one part come together under the part's name; whole
        // parts and those may not share a name, case aside.
        let mut seen: BTreeMap<PartName, String> = BTreeMap::new();
        for (_, part, _) in &wanted {
            let logical = piece_of(part.as_str()).map_or(part.as_str(), |(whole, _, _)| whole);
            let key = PartName(logical.to_owned());
            let first_piece = piece_of(part.as_str()).is_some_and(|(_, n, _)| n == 0);
            if piece_of(part.as_str()).is_some() && !first_piece {
                continue;
            }
            if let Some(earlier) = seen.insert(key, part.as_str().to_owned()) {
                return Err(PackageReadError::EquivalentPartNames { first: earlier, second: part.as_str().to_owned() });
            }
        }

        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes))?;
        let mut decompressed = Vec::with_capacity(wanted.len());
        let mut done = 0u64;
        let mut buffer = vec![0; 262144];
        for (index, part, declared) in wanted {
            let mut entry = archive.by_index(index)?;
            // The library's index order is the directory's, which the
            // structure check read; make sure both mean the same entry.
            if entry.name_raw() != listing[index].name.as_bytes() {
                let name = listing[index].name.clone();
                return Err(PackageReadError::Structure(crate::zip_check::ZipStructureError::HeadersDisagree { name, field: "position in the index" }));
            }
            let read_failed = |source| PackageReadError::PartRead { part: part.as_str().to_owned(), source };
            let mut data = Vec::with_capacity((declared as usize).min(16 * 1024 * 1024));
            loop {
                let count = entry.read(&mut buffer).map_err(read_failed)?;
                if count == 0 {
                    break;
                }
                if data.len() as u64 + count as u64 > declared {
                    let message = "the part decompresses to more than its declared size";
                    return Err(read_failed(std::io::Error::new(std::io::ErrorKind::InvalidData, message)));
                }
                done += count as u64;
                data.extend_from_slice(&buffer[..count]);
                progress(DecompressionProgress { decompressed_bytes: done, total_bytes: total });
            }
            if data.len() as u64 != declared {
                let message = "the part decompresses to less than its declared size";
                return Err(read_failed(std::io::Error::new(std::io::ErrorKind::InvalidData, message)));
            }
            decompressed.push((part, data));
        }
        let raw = join_pieces(decompressed)?;

        let content_types = PartName("/[Content_Types].xml".to_owned());
        let types = raw.iter().find(|(name, _)| *name == content_types).ok_or(PackageReadError::MissingContentTypes)?;
        let mut defaults = BTreeMap::new();
        let mut overrides: BTreeMap<PartName, String> = BTreeMap::new();
        for (element, attributes) in entries(&types.1)? {
            let (key_attribute, is_default) = match element.as_str() {
                "Default" => ("Extension", true),
                "Override" => ("PartName", false),
                _ => continue,
            };
            let missing = |attribute| PackageReadError::ContentTypeMissingAttribute { attribute };
            let key = attributes.get(key_attribute).ok_or(missing(key_attribute))?;
            let content_type = attributes.get("ContentType").ok_or(missing("ContentType"))?.clone();
            // Conformance N_*_0207: an Override names a valid part.
            if !is_default && PartName::new(key).is_err() {
                let reason = format!("an Override names \"{key}\", which is not a valid part name");
                return Err(PackageReadError::InvalidContentTypes { reason });
            }
            if is_default && key.is_empty() {
                let reason = "a Default declares an empty extension".to_owned();
                return Err(PackageReadError::InvalidContentTypes { reason });
            }
            // A second declaration for the same extension or part leaves
            // its type to whichever one a program reads: refused.
            let repeated = if is_default {
                defaults.insert(key.to_ascii_lowercase(), content_type).is_some()
            } else {
                overrides.insert(PartName(key.clone()), content_type).is_some()
            };
            if repeated {
                return Err(PackageReadError::DuplicateContentType { key: key.clone() });
            }
        }

        let existing: std::collections::BTreeSet<PartName> = raw.iter().map(|(part, _)| part.clone()).collect();
        let mut package = Self::new();
        for (part, data) in raw {
            if part == content_types {
                continue;
            }
            let name = part.as_str().to_owned();
            let (folder, file) = name.rsplit_once('/').unwrap_or(("", name.as_str()));
            let in_rels = folder.rsplit('/').next().is_some_and(|last| last.eq_ignore_ascii_case("_rels"));
            if in_rels && file.len() >= 5 && file[file.len() - 5..].eq_ignore_ascii_case(".rels") {
                let declared = overrides.get(&part).or_else(|| defaults.get("rels"));
                if declared.map(String::as_str) != Some(RELATIONSHIPS_CONTENT_TYPE) {
                    let shown = declared.map_or("none".to_owned(), |t| format!("\"{t}\""));
                    let reason = format!("relationship part \"{name}\" has content type {shown}");
                    return Err(PackageReadError::InvalidContentTypes { reason });
                }
                let relationships = read_relationships(&name, &data)?;
                if name.eq_ignore_ascii_case("/_rels/.rels") {
                    package.root = relationships;
                } else {
                    let parent = &folder[..folder.len() - "_rels".len()];
                    let source = format!("{parent}{}", &file[..file.len() - 5]);
                    let source_part = PartName::new(&source)
                        .map_err(|cause| PackageReadError::MalformedPartName { name: source, source: cause })?;
                    // Relationships of a part that is not there belong to
                    // nothing (a misnamed .rels part, say).
                    if !existing.contains(&source_part) {
                        return Err(PackageReadError::OrphanRelationships { part: name });
                    }
                    package.rels.insert(source_part, relationships);
                }
                continue;
            }
            let by_extension = ext(&name).and_then(|suffix| defaults.get(&suffix));
            match overrides.get(&part).or(by_extension) {
                Some(content_type) => package.parts.push((part, content_type.clone(), data)),
                None => {
                    package.warnings.push(PackageWarning::UnlistedPart { part: name });
                    package.unlisted.push(part);
                }
            }
        }
        package.check_thumbnails()?;
        Ok(package)
    }

    /// Thumbnail relationships (conformance N_*_0403, N_*_0405_01,
    /// N_*_0404_04): inside the package, at a part that is there, whose
    /// type is an image.
    fn check_thumbnails(&self) -> Result<(), PackageReadError> {
        let sources = std::iter::once(("/", &self.root))
            .chain(self.rels.iter().map(|(source, list)| (source.as_str(), list)));
        for (source, relationships) in sources {
            for thumbnail in relationships.iter().filter(|r| r.rel_type == THUMBNAIL_REL_TYPE) {
                let fail = |reason: String| PackageReadError::InvalidRelationship { part: source.to_owned(), reason };
                if thumbnail.external {
                    return Err(fail(format!("the thumbnail \"{}\" is outside the package", thumbnail.target)));
                }
                let target = resolve_target(source, &thumbnail.target);
                let found = PartName::new(&target).ok().and_then(|part| self.part_content_type(&part));
                match found {
                    None => return Err(fail(format!("the thumbnail \"{target}\" is not in the package"))),
                    Some("image/png" | "image/jpeg") => {}
                    Some(other) => {
                        return Err(fail(format!("the thumbnail \"{target}\" has content type \"{other}\", not an image")));
                    }
                }
            }
        }
        Ok(())
    }
    pub fn write_to<W: Write + Seek>(&self, w: W) -> Result<(), PackageWriteError> {
        self.write_to_with_progress(w, &mut |_, _| {})
    }
    pub fn write_to_with_progress<W: Write + Seek>(
        &self,
        w: W,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<(), PackageWriteError> {
        let mut defaults: BTreeMap<String, Option<&str>> = BTreeMap::new();
        for (n, t, _) in &self.parts {
            if let Some(e) = ext(n.as_str()) {
                defaults
                    .entry(e)
                    .and_modify(|v| {
                        if *v != Some(t) {
                            *v = None
                        }
                    })
                    .or_insert(Some(t));
            }
        }
        defaults.insert("rels".into(), Some(RELATIONSHIPS_CONTENT_TYPE));
        let mut ct = format!("{DECL}<Types xmlns=\"{CONTENT_TYPES_NS}\">");
        for (e, t) in &defaults {
            if let Some(t) = t {
                ct.push_str(&format!(
                    "<Default Extension=\"{}\" ContentType=\"{}\"/>",
                    escape(e),
                    escape(t)
                ))
            }
        }
        for (n, t, _) in &self.parts {
            if ext(n.as_str()).and_then(|e| defaults.get(&e).copied().flatten()) != Some(t) {
                ct.push_str(&format!(
                    "<Override PartName=\"{}\" ContentType=\"{}\"/>",
                    escape(n.as_str()),
                    escape(t)
                ))
            }
        }
        ct.push_str("</Types>");
        let mut z = zip::ZipWriter::new(w);
        let opt = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .compression_level(Some(6));
        z.start_file("[Content_Types].xml", opt)?;
        z.write_all(ct.as_bytes())?;
        z.start_file("_rels/.rels", opt)?;
        z.write_all(&relxml(&self.root))?;
        for (n, rs) in &self.rels {
            if rs.is_empty() {
                continue;
            }
            let (dir, file) = n.as_str().rsplit_once('/').unwrap_or(("", n.as_str()));
            z.start_file(
                format!("{}/_rels/{file}.rels", dir.trim_start_matches('/'))
                    .trim_start_matches('/'),
                opt,
            )?;
            z.write_all(&relxml(rs))?;
        }
        let total = self.parts.iter().map(|p| p.2.len() as u64).sum();
        let mut done = 0;
        for (n, _, data) in &self.parts {
            if data.len() >= 4 * 1024 * 1024 && data.len() < u32::MAX as usize {
                let archive = parallel_entry(&n.as_str()[1..], data)?;
                let mut archive = zip::ZipArchive::new(std::io::Cursor::new(archive))?;
                z.raw_copy_file(archive.by_index(0)?)?;
            } else {
                z.start_file(&n.as_str()[1..], opt)?;
                for chunk in data.chunks(262144) {
                    z.write_all(chunk)?;
                }
            }
            for chunk in data.chunks(262144) {
                done += chunk.len() as u64;
                progress(done, total)
            }
        }
        z.finish()?;
        Ok(())
    }
}

/// `target` as an absolute part name: as it is when it starts with `/`,
/// otherwise relative to the folder of `source` (the package root for the
/// root relationships).
fn resolve_target(source: &str, target: &str) -> String {
    if target.starts_with('/') {
        return target.to_owned();
    }
    let folder = source.rsplit_once('/').map_or("", |(folder, _)| folder);
    let mut parts: Vec<&str> = folder.split('/').filter(|s| !s.is_empty()).collect();
    for step in target.split('/') {
        match step {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            name => parts.push(name),
        }
    }
    format!("/{}", parts.join("/"))
}

/// Whether `id` is an XML ID (an NCName): a letter or `_` first, then
/// letters, digits, `.`, `-` or `_`.
fn is_xml_id(id: &str) -> bool {
    let mut chars = id.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_alphabetic() || c == '_');
    first_ok && chars.all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// One relationship part's relationships.
fn read_relationships(name: &str, data: &[u8]) -> Result<Vec<Relationship>, PackageReadError> {
    let records = entries(data).map_err(|source| PackageReadError::RelationshipsXml { part: name.into(), source })?;
    let mut relationships = Vec::new();
    for (element, attributes) in records {
        if element != "Relationship" {
            continue;
        }
        let get = |attribute| {
            attributes.get(attribute).cloned().ok_or_else(|| PackageReadError::RelationshipMissingAttribute { part: name.into(), attribute })
        };
        let external = attributes.get("TargetMode").is_some_and(|mode| mode == "External");
        let id = get("Id")?;
        // Conformance N_*_0405_04: Ids are XML IDs, unique in their part.
        let invalid = |reason: String| PackageReadError::InvalidRelationship { part: name.into(), reason };
        if !is_xml_id(&id) {
            return Err(invalid(format!("Id \"{id}\" is not a valid XML ID")));
        }
        if relationships.iter().any(|earlier: &Relationship| earlier.id == id) {
            return Err(invalid(format!("Id \"{id}\" is used twice")));
        }
        relationships.push(Relationship { id, rel_type: get("Type")?, target: get("Target")?, external });
    }
    Ok(relationships)
}

/// Whole parts as they are, and each interleaved part's pieces joined in
/// order. The pieces must number 0 to n with no gap or repeat, and only
/// the n-th may be (and must be) the last.
fn join_pieces(decompressed: Vec<(PartName, Vec<u8>)>) -> Result<Vec<(PartName, Vec<u8>)>, PackageReadError> {
    let mut whole = Vec::new();
    let mut pieced: BTreeMap<PartName, Vec<(u64, bool, Vec<u8>)>> = BTreeMap::new();
    for (part, data) in decompressed {
        match piece_of(part.as_str()) {
            None => whole.push((part, data)),
            Some((logical, number, last)) => {
                pieced.entry(PartName(logical.to_owned())).or_default().push((number, last, data));
            }
        }
    }
    for (logical, mut pieces) in pieced {
        pieces.sort_by_key(|(number, _, _)| *number);
        let count = pieces.len() as u64;
        let in_order = pieces.iter().enumerate().all(|(at, (number, _, _))| *number == at as u64);
        let last_only_at_end = pieces.iter().all(|(number, last, _)| *last == (*number + 1 == count));
        if !in_order || !last_only_at_end {
            return Err(PackageReadError::BrokenPieces { part: logical.as_str().to_owned() });
        }
        let joined = pieces.into_iter().flat_map(|(_, _, data)| data).collect();
        whole.push((logical, joined));
    }
    Ok(whole)
}
