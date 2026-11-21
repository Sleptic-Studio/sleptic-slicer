// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Profile packages: a zip with `manifest.toml` and the profiles' TOML
//! files, copied byte for byte from and into a user library.
use crate::*;
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read, Seek, Write},
};
use zip::{ZipArchive, ZipWriter, result::ZipError, write::SimpleFileOptions};
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportSelection {
    All,
    Printers,
    Materials,
    Ids(Vec<String>),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportManifest {
    pub package: PackageMetadata,
    pub contents: PackageContents,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub exported_at: String,
    pub sc_slicer_version: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PackageContents {
    pub printers: Vec<String>,
    pub materials: Vec<String>,
    pub material_extensions: Vec<String>,
}
impl ExportManifest {
    pub fn stamped_now(version: &str) -> Self {
        Self {
            package: PackageMetadata {
                name: None,
                description: None,
                author: None,
                exported_at: chrono::Local::now().to_rfc3339(),
                sc_slicer_version: version.to_owned(),
            },
            contents: PackageContents::default(),
        }
    }
}
#[derive(Debug)]
pub enum ExportError {
    InvalidId { id: String, cause: InvalidProfileId },
    UnknownId { id: String },
    List(ListLibraryError),
    ReadProfile { path: PathBuf, cause: io::Error },
    SerializeManifest(toml_edit::ser::Error),
    Zip(ZipError),
}
impl fmt::Display for ExportError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::List(inner) => fmt::Display::fmt(inner, out),
            Self::InvalidId { id, cause } => write!(out, "invalid profile id {id:?}: {cause}"),
            Self::UnknownId { id } => write!(out, "profile id {id:?} matched no printer or material in the library"),
            Self::ReadProfile { path, cause } => {
                write!(out, "failed to read profile {} for export: {cause}", path.display())
            }
            Self::SerializeManifest(why) => write!(out, "failed to serialise the package manifest: {why}"),
            Self::Zip(why) => write!(out, "failed to write the package archive: {why}"),
        }
    }
}
impl std::error::Error for ExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::UnknownId { .. } => return None,
            Self::InvalidId { cause, .. } => cause,
            Self::List(why) => why,
            Self::ReadProfile { cause, .. } => cause,
            Self::SerializeManifest(why) => why,
            Self::Zip(why) => why,
        })
    }
}

/// The library's three folders, in the order a package lists them.
const KIND_FOLDERS: [&str; 3] = ["printers", "materials", "materials/extensions"];

/// Writes a package of the profiles `selection` names from the library at
/// `root` to `writer`: the manifest (with its contents filled in) first,
/// then each profile's file as it is on disk, printers, materials and
/// material extensions in turn, each kind sorted by id. An id given by
/// name is taken from every kind that has it; an id no kind has fails the
/// export, as does a malformed one, in the order the ids were given.
pub fn export_package<W: Write + Seek>(
    root: &Path,
    selection: &ExportSelection,
    manifest: &ExportManifest,
    writer: W,
) -> Result<(), ExportError> {
    fn as_set<Id: ToString>(ids: Result<Vec<Id>, ListLibraryError>) -> Result<BTreeSet<String>, ExportError> {
        Ok(ids.map_err(ExportError::List)?.iter().map(ToString::to_string).collect())
    }
    let in_library = [
        as_set(list_printer_ids(root))?,
        as_set(list_material_ids(root))?,
        as_set(list_material_extension_ids(root))?,
    ];
    let chosen: [BTreeSet<String>; 3] = match selection {
        ExportSelection::All => in_library,
        ExportSelection::Printers => {
            let [printers, _, _] = in_library;
            [printers, BTreeSet::new(), BTreeSet::new()]
        }
        ExportSelection::Materials => {
            let [_, materials, extensions] = in_library;
            [BTreeSet::new(), materials, extensions]
        }
        ExportSelection::Ids(ids) => {
            let mut picked: [BTreeSet<String>; 3] = Default::default();
            for id in ids {
                if let Err(cause) = PrinterId::new(id) {
                    return Err(ExportError::InvalidId { id: id.clone(), cause });
                }
                let kinds: Vec<usize> = (0..3).filter(|&kind| in_library[kind].contains(id)).collect();
                if kinds.is_empty() {
                    return Err(ExportError::UnknownId { id: id.clone() });
                }
                kinds.into_iter().for_each(|kind| {
                    picked[kind].insert(id.clone());
                });
            }
            picked
        }
    };
    let listed = |kind: usize| chosen[kind].iter().cloned().collect::<Vec<_>>();
    let mut stamped = manifest.clone();
    stamped.contents = PackageContents { printers: listed(0), materials: listed(1), material_extensions: listed(2) };
    let manifest_text = toml_edit::ser::to_string_pretty(&stamped).map_err(ExportError::SerializeManifest)?;

    let deflated = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut package = ZipWriter::new(writer);
    let mut add = |name: String, bytes: &[u8]| -> Result<(), ExportError> {
        package.start_file(name, deflated).map_err(ExportError::Zip)?;
        package.write_all(bytes).map_err(|why| ExportError::Zip(why.into()))
    };
    add("manifest.toml".to_owned(), manifest_text.as_bytes())?;
    for (folder, ids) in KIND_FOLDERS.into_iter().zip(&chosen) {
        for id in ids {
            let name = format!("{folder}/{id}.toml");
            let path = root.join(&name);
            let bytes = fs::read(&path).map_err(|cause| ExportError::ReadProfile { path, cause })?;
            add(name, &bytes)?;
        }
    }
    package.finish().map_err(ExportError::Zip)?;
    Ok(())
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collision {
    pub id: String,
    pub library_path: PathBuf,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollisionReport {
    pub printers: Vec<Collision>,
    pub materials: Vec<Collision>,
    pub material_extensions: Vec<Collision>,
}
impl CollisionReport {
    pub fn len(&self) -> usize {
        self.printers.len() + self.materials.len() + self.material_extensions.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
#[derive(Debug)]
pub enum ImportError {
    Archive(ZipError),
    /// The ZIP archive's structure was refused: it could read differently
    /// in another program (see `sc_3mf::zip_check`).
    Structure(sc_3mf::zip_check::ZipStructureError),
    /// A profile far larger than any real one.
    ProfileTooLarge { name: String, cap_bytes: u64 },
    ReadEntry { name: String, cause: io::Error },
    Collisions(CollisionReport),
    CreateSubdir { path: PathBuf, cause: io::Error },
    WriteProfile { path: PathBuf, cause: io::Error },
}
impl fmt::Display for ImportError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Archive(why) => write!(out, "failed to read the package archive: {why}"),
            Self::Structure(why) => write!(out, "the profile package was refused: {why}"),
            Self::ProfileTooLarge { name, cap_bytes } => {
                write!(out, "package entry {name:?} is larger than {cap_bytes} bytes, far beyond any profile")
            }
            Self::Collisions(report) => write!(out, "import would overwrite {} existing profile(s)", report.len()),
            Self::ReadEntry { name, cause } => write!(out, "failed to read package entry {name:?}: {cause}"),
            Self::CreateSubdir { path, cause } => {
                write!(out, "failed to create the destination directory {}: {cause}", path.display())
            }
            Self::WriteProfile { path, cause } => write!(out, "failed to write profile {}: {cause}", path.display()),
        }
    }
}
impl std::error::Error for ImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Collisions(_) | Self::ProfileTooLarge { .. } => return None,
            Self::Structure(why) => why,
            Self::Archive(why) => why,
            Self::ReadEntry { cause, .. } => cause,
            Self::CreateSubdir { cause, .. } => cause,
            Self::WriteProfile { cause, .. } => cause,
        })
    }
}

/// Far larger than any profile's TOML.
const PROFILE_CAP: u64 = 16 * 1024 * 1024;

/// The kind (an index into [`KIND_FOLDERS`]) and id of a package entry
/// that is a profile: exactly `<folder>/<id>.toml` with a valid id.
/// Anything else (the manifest, other files, nested or odd paths) is not.
fn profile_entry(name: &str) -> Option<(usize, String)> {
    let (folder, file) = name.rsplit_once('/')?;
    let kind = KIND_FOLDERS.iter().position(|known| *known == folder)?;
    let id = file.strip_suffix(".toml")?;
    PrinterId::new(id).is_ok().then(|| (kind, id.to_owned()))
}

/// Copies the package's profiles into the library at `root`. Unless
/// `force`, nothing is written when any of them would replace a file
/// already there: the error lists those, per kind, sorted by id.
pub fn import_package<R: Read + Seek>(mut reader: R, root: &Path, force: bool) -> Result<(), ImportError> {
    // The archive's structure is checked before anything in it is read,
    // as for 3MF files: a package that two programs would unpack
    // differently is refused, as is a profile too large to be one.
    let mut packed = Vec::new();
    reader.seek(io::SeekFrom::Start(0)).map_err(|why| ImportError::Archive(ZipError::Io(why)))?;
    reader.read_to_end(&mut packed).map_err(|why| ImportError::Archive(ZipError::Io(why)))?;
    let listing = sc_3mf::zip_check::check_zip_structure(&packed).map_err(ImportError::Structure)?;
    for entry in &listing {
        if profile_entry(&entry.name).is_some() && entry.uncompressed_size > PROFILE_CAP {
            return Err(ImportError::ProfileTooLarge { name: entry.name.clone(), cap_bytes: PROFILE_CAP });
        }
    }
    let names: Vec<String> = listing.into_iter().map(|entry| entry.name).collect();
    let mut archive = ZipArchive::new(io::Cursor::new(packed)).map_err(ImportError::Archive)?;
    if !force {
        let mut report = CollisionReport::default();
        let mut counted = BTreeSet::new();
        for name in &names {
            let Some((kind, id)) = profile_entry(name) else { continue };
            let library_path = root.join(name);
            if !library_path.is_file() || !counted.insert((kind, id.clone())) {
                continue;
            }
            let list = match kind {
                0 => &mut report.printers,
                1 => &mut report.materials,
                _ => &mut report.material_extensions,
            };
            list.push(Collision { id, library_path });
        }
        if !report.is_empty() {
            for list in [&mut report.printers, &mut report.materials, &mut report.material_extensions] {
                list.sort_by(|a, b| a.id.cmp(&b.id));
            }
            return Err(ImportError::Collisions(report));
        }
    }
    for (index, name) in names.iter().enumerate() {
        if profile_entry(name).is_none() {
            continue;
        }
        let mut bytes = Vec::new();
        let mut entry = archive.by_index(index).map_err(ImportError::Archive)?;
        if entry.name_raw() != name.as_bytes() {
            let mismatch = sc_3mf::zip_check::ZipStructureError::HeadersDisagree { name: name.clone(), field: "position in the index" };
            return Err(ImportError::Structure(mismatch));
        }
        entry.read_to_end(&mut bytes).map_err(|cause| ImportError::ReadEntry { name: name.clone(), cause })?;
        let path = root.join(name);
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder).map_err(|cause| ImportError::CreateSubdir { path: folder.into(), cause })?;
        }
        fs::write(&path, bytes).map_err(|cause| ImportError::WriteProfile { path, cause })?;
    }
    Ok(())
}
