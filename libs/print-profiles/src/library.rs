// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Profile libraries on disk. A library is a folder with `printers/` and
//! `materials/` (and `materials/extensions/`, `images/`), one TOML file per
//! profile named after its id. The user's library is searched before the
//! bundled ones, so a user's copy of a profile wins.
//!
//! Printers and resins are stored the same way, so the reading, writing
//! and searching is written once over [`LibraryProfile`]; the
//! `*_printer_*` / `*_material_*` functions name it for each kind.
use crate::{ImageAssetRef, MaterialDefaults, MaterialExtensionIssue, MaterialIntent};
use crate::{EffectiveMaterial, MaterialId, MaterialProfile, PrinterId, PrinterProfile};
use serde::{Serialize, de::DeserializeOwned};
use std::path::{Path, PathBuf};
use std::{error::Error, fmt, fs, io, marker::PhantomData, str::FromStr};

/// A profile kind kept in a library folder of its own.
pub trait LibraryProfile: Serialize + DeserializeOwned + fmt::Debug + 'static {
    type Id: fmt::Display;
    /// The folder under a library root.
    const FOLDER: &'static str;
    /// What messages call it.
    const NOUN: &'static str;
    fn library_id(&self) -> &Self::Id;
}

impl LibraryProfile for PrinterProfile {
    type Id = PrinterId;
    const FOLDER: &'static str = "printers";
    const NOUN: &'static str = "printer profile";
    fn library_id(&self) -> &PrinterId {
        &self.identity.id
    }
}

impl LibraryProfile for MaterialProfile {
    type Id = MaterialId;
    const FOLDER: &'static str = "materials";
    const NOUN: &'static str = "material profile";
    fn library_id(&self) -> &MaterialId {
        &self.identity.id
    }
}

/// Where a profile kind's file for `id` lives under `root`.
pub fn profile_path_in_library<P: LibraryProfile>(root: &Path, id: &P::Id) -> PathBuf {
    root.join(P::FOLDER).join(format!("{id}.toml"))
}

// Reading one file.

/// A profile file that would not read.
#[derive(Debug)]
pub struct LoadProfileError<P> {
    pub path: PathBuf,
    pub kind: LoadProfileErrorKind,
    of: PhantomData<fn() -> P>,
}

#[derive(Debug)]
pub enum LoadProfileErrorKind {
    Io(io::Error),
    TomlParse(toml_edit::de::Error),
}

impl<P: LibraryProfile> fmt::Display for LoadProfileError<P> {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "failed to load {} from {}: ", P::NOUN, self.path.display())?;
        match &self.kind {
            LoadProfileErrorKind::Io(cause) => cause.fmt(out),
            LoadProfileErrorKind::TomlParse(cause) => cause.fmt(out),
        }
    }
}

impl fmt::Display for LoadProfileErrorKind {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadProfileErrorKind::Io(cause) => cause.fmt(out),
            LoadProfileErrorKind::TomlParse(cause) => cause.fmt(out),
        }
    }
}

impl<P: LibraryProfile> Error for LoadProfileError<P> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            LoadProfileErrorKind::Io(cause) => Some(cause),
            LoadProfileErrorKind::TomlParse(cause) => Some(cause),
        }
    }
}

impl<P> LoadProfileError<P> {
    /// Whether the file is simply not there (as opposed to unreadable).
    fn is_missing(&self) -> bool {
        matches!(&self.kind, LoadProfileErrorKind::Io(cause) if cause.kind() == io::ErrorKind::NotFound)
    }
}

/// Reads and parses one profile file.
pub fn load_profile<P: LibraryProfile>(path: &Path) -> Result<P, LoadProfileError<P>> {
    let failed = |kind| LoadProfileError { path: path.to_path_buf(), kind, of: PhantomData };
    let text = fs::read_to_string(path).map_err(|cause| failed(LoadProfileErrorKind::Io(cause)))?;
    toml_edit::de::from_str(&text).map_err(|cause| failed(LoadProfileErrorKind::TomlParse(cause)))
}

// Writing one file.

/// A profile that would not be written.
#[derive(Debug)]
pub struct SaveProfileError<P> {
    pub path: PathBuf,
    pub kind: SaveProfileErrorKind,
    of: PhantomData<fn() -> P>,
}

#[derive(Debug)]
pub enum SaveProfileErrorKind {
    Io(io::Error),
    TomlSerialize(toml_edit::ser::Error),
}

impl<P: LibraryProfile> fmt::Display for SaveProfileError<P> {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "failed to save {} to {}: ", P::NOUN, self.path.display())?;
        match &self.kind {
            SaveProfileErrorKind::Io(cause) => cause.fmt(out),
            SaveProfileErrorKind::TomlSerialize(cause) => cause.fmt(out),
        }
    }
}

impl<P: LibraryProfile> Error for SaveProfileError<P> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            SaveProfileErrorKind::Io(cause) => Some(cause),
            SaveProfileErrorKind::TomlSerialize(cause) => Some(cause),
        }
    }
}

/// Writes `profile` as TOML to `path`, making its folder if needed.
pub fn save_profile<P: LibraryProfile>(path: &Path, profile: &P) -> Result<(), SaveProfileError<P>> {
    let failed = |kind| SaveProfileError { path: path.to_path_buf(), kind, of: PhantomData };
    let text = toml_edit::ser::to_string_pretty(profile).map_err(|cause| failed(SaveProfileErrorKind::TomlSerialize(cause)))?;
    write_with_folder(path, text.as_bytes()).map_err(|cause| failed(SaveProfileErrorKind::Io(cause)))
}

/// Writes `profile` into the library at `root`, under its own id.
pub fn save_profile_to_library<P: LibraryProfile>(root: &Path, profile: &P) -> Result<(), SaveProfileError<P>> {
    save_profile(&profile_path_in_library::<P>(root, profile.library_id()), profile)
}

fn write_with_folder(path: &Path, bytes: &[u8]) -> io::Result<()> {
    match path.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => fs::create_dir_all(folder)?,
        _ => {}
    }
    fs::write(path, bytes)
}

// Searching every library.

/// Where a profile was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSource {
    User,
    Bundled,
}

/// The libraries to search: the user's (if there is a home folder) and
/// the ones installed with the app.
#[derive(Debug, Clone, Default)]
pub struct ProfileLibraries {
    pub user: Option<PathBuf>,
    pub bundled: Vec<PathBuf>,
}

/// No profile in any library, or one that would not read.
#[derive(Debug)]
pub enum FindProfileError<P> {
    Source(LoadProfileError<P>),
    NotFoundAnywhere { attempted: Vec<PathBuf> },
}

impl<P: LibraryProfile> fmt::Display for FindProfileError<P> {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let attempted = match self {
            FindProfileError::Source(cause) => return cause.fmt(out),
            FindProfileError::NotFoundAnywhere { attempted } => attempted,
        };
        write!(out, "{} not found in any of the configured sources", P::NOUN)?;
        if attempted.is_empty() {
            return out.write_str(" (no profile library roots configured)");
        }
        let places: Vec<String> = attempted.iter().map(|path| path.display().to_string()).collect();
        write!(out, ": {}", places.join(", "))
    }
}

impl<P: LibraryProfile> Error for FindProfileError<P> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            FindProfileError::Source(cause) => Some(cause),
            FindProfileError::NotFoundAnywhere { .. } => None,
        }
    }
}

/// The folder for the user's own profiles could not be found.
#[derive(Debug, Clone, Copy)]
pub struct UserLibraryDirError;

impl fmt::Display for UserLibraryDirError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str("cannot determine the user data directory because no home directory could be resolved")
    }
}

impl Error for UserLibraryDirError {}

/// The user's library: `profiles` in the app's data folder.
pub fn user_library_dir() -> Result<PathBuf, UserLibraryDirError> {
    let dirs = sc_app_paths::project_dirs().ok_or(UserLibraryDirError)?;
    Ok(dirs.data_dir().join("profiles"))
}

impl ProfileLibraries {
    pub fn new(user: Option<PathBuf>, bundled: Vec<PathBuf>) -> ProfileLibraries {
        ProfileLibraries { user, bundled }
    }

    /// This system's user library and the app's bundled ones.
    pub fn platform_default() -> ProfileLibraries {
        let bundled = sc_app_paths::bundled_data_dirs().into_iter().map(|dir| dir.join("profiles"));
        ProfileLibraries::new(user_library_dir().ok(), bundled.collect())
    }

    pub fn user_library_dir(&self) -> Option<&Path> {
        self.user.as_deref()
    }

    /// Every library in search order, user first.
    pub fn roots(&self) -> impl Iterator<Item = (&Path, ProfileSource)> {
        let user = self.user.iter().map(|root| (root.as_path(), ProfileSource::User));
        user.chain(self.bundled.iter().map(|root| (root.as_path(), ProfileSource::Bundled)))
    }

    /// The first library's copy of profile `id`. A library without one is
    /// passed over; one whose copy will not read stops the search.
    pub fn find<P: LibraryProfile>(&self, id: &P::Id) -> Result<(P, ProfileSource), FindProfileError<P>> {
        let mut attempted = Vec::new();
        for (root, source) in self.roots() {
            let path = profile_path_in_library::<P>(root, id);
            match load_profile::<P>(&path) {
                Ok(profile) => return Ok((profile, source)),
                Err(missing) if missing.is_missing() => attempted.push(path),
                Err(unreadable) => return Err(FindProfileError::Source(unreadable)),
            }
        }
        Err(FindProfileError::NotFoundAnywhere { attempted })
    }

    pub fn printer_by_id(&self, id: &PrinterId) -> Result<(PrinterProfile, ProfileSource), LoadPrinterProfileByIdError> {
        self.find(id)
    }

    pub fn material_by_id(&self, id: &MaterialId) -> Result<(MaterialProfile, ProfileSource), LoadMaterialProfileByIdError> {
        self.find(id)
    }

    pub fn load_printer_by_id(&self, id: &PrinterId) -> Result<PrinterProfile, LoadPrinterProfileByIdError> {
        Ok(self.printer_by_id(id)?.0)
    }

    pub fn load_material_by_id(&self, id: &MaterialId) -> Result<MaterialProfile, LoadMaterialProfileByIdError> {
        Ok(self.material_by_id(id)?.0)
    }

    /// Resin `id` with the user's extension over it, if there is one.
    pub fn effective_material_by_id(
        &self,
        id: &MaterialId,
    ) -> Result<(EffectiveMaterial, Vec<MaterialExtensionIssue>), LoadMaterialProfileByIdError> {
        let base = self.load_material_by_id(id)?;
        Ok(self.compose_with_extension(base))
    }

    /// `base` with the user's extension for it applied. Extensions live
    /// only in the user's library; a missing one is not an issue, an
    /// unreadable one is reported and left out.
    pub fn compose_with_extension(&self, base: MaterialProfile) -> (EffectiveMaterial, Vec<MaterialExtensionIssue>) {
        let Some(root) = self.user.as_deref() else {
            return (base.into(), Vec::new());
        };
        let path = material_extension_path_in_library(root, &base.identity.id);
        match load_material_profile_from_path(&path) {
            Ok(extension) => EffectiveMaterial::compose(base, Some(extension)),
            Err(missing) if missing.is_missing() => (base.into(), Vec::new()),
            Err(cause) => (base.into(), vec![MaterialExtensionIssue::ExtensionUnreadable { path, cause }]),
        }
    }
}

// The names each kind is used by.

pub type LoadPrinterProfileError = LoadProfileError<PrinterProfile>;
pub type LoadPrinterProfileErrorKind = LoadProfileErrorKind;
pub type SavePrinterProfileError = SaveProfileError<PrinterProfile>;
pub type SavePrinterProfileErrorKind = SaveProfileErrorKind;
pub type LoadPrinterProfileByIdError = FindProfileError<PrinterProfile>;
pub type LoadPrinterError = LoadPrinterProfileError;
pub type LoadPrinterErrorKind = LoadProfileErrorKind;
pub type SavePrinterError = SavePrinterProfileError;
pub type SavePrinterErrorKind = SaveProfileErrorKind;

pub type LoadMaterialProfileError = LoadProfileError<MaterialProfile>;
pub type LoadMaterialProfileErrorKind = LoadProfileErrorKind;
pub type SaveMaterialProfileError = SaveProfileError<MaterialProfile>;
pub type SaveMaterialProfileErrorKind = SaveProfileErrorKind;
pub type LoadMaterialProfileByIdError = FindProfileError<MaterialProfile>;
pub type LoadMaterialError = LoadMaterialProfileError;
pub type LoadMaterialErrorKind = LoadProfileErrorKind;
pub type SaveMaterialError = SaveMaterialProfileError;
pub type SaveMaterialErrorKind = SaveProfileErrorKind;

pub fn printer_profile_path_in_library(root: &Path, id: &PrinterId) -> PathBuf {
    profile_path_in_library::<PrinterProfile>(root, id)
}

pub fn material_profile_path_in_library(root: &Path, id: &MaterialId) -> PathBuf {
    profile_path_in_library::<MaterialProfile>(root, id)
}

pub fn load_printer_profile_from_path(path: &Path) -> Result<PrinterProfile, LoadPrinterProfileError> {
    load_profile(path)
}

pub fn load_material_profile_from_path(path: &Path) -> Result<MaterialProfile, LoadMaterialProfileError> {
    load_profile(path)
}

pub fn load_printer_profile_from_library(root: &Path, id: &PrinterId) -> Result<PrinterProfile, LoadPrinterProfileError> {
    load_profile(&printer_profile_path_in_library(root, id))
}

pub fn load_material_profile_from_library(root: &Path, id: &MaterialId) -> Result<MaterialProfile, LoadMaterialProfileError> {
    load_profile(&material_profile_path_in_library(root, id))
}

pub fn save_printer_profile_to_path(path: &Path, profile: &PrinterProfile) -> Result<(), SavePrinterProfileError> {
    save_profile(path, profile)
}

pub fn save_material_profile_to_path(path: &Path, profile: &MaterialProfile) -> Result<(), SaveMaterialProfileError> {
    save_profile(path, profile)
}

pub fn save_printer_profile_to_library(root: &Path, profile: &PrinterProfile) -> Result<(), SavePrinterProfileError> {
    save_profile_to_library(root, profile)
}

pub fn save_material_profile_to_library(root: &Path, profile: &MaterialProfile) -> Result<(), SaveMaterialProfileError> {
    save_profile_to_library(root, profile)
}

// Listing a library.

/// A library folder that could not be listed.
#[derive(Debug)]
pub struct ListLibraryError {
    pub path: PathBuf,
    pub source: io::Error,
}

impl fmt::Display for ListLibraryError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "failed to list the profile library at {}: {}", self.path.display(), self.source)
    }
}

impl Error for ListLibraryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// The ids of the `.toml` files in `root/folder`, sorted; files whose
/// names are not valid ids are passed over, and a missing folder is empty.
fn ids_in<T: FromStr + Ord>(root: &Path, folder: &str) -> Result<Vec<T>, ListLibraryError> {
    let path = root.join(folder);
    let entries = match fs::read_dir(&path) {
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        listed => listed.map_err(|source| ListLibraryError { path: path.clone(), source })?,
    };
    let mut ids = Vec::new();
    for entry in entries {
        let file = entry.map_err(|source| ListLibraryError { path: path.clone(), source })?.path();
        let is_toml = file.extension().is_some_and(|extension| extension == "toml");
        if let Some(id) = file.file_stem().and_then(|stem| stem.to_str()).filter(|_| is_toml).and_then(|stem| stem.parse().ok()) {
            ids.push(id);
        }
    }
    ids.sort();
    Ok(ids)
}

pub fn list_printer_ids(root: &Path) -> Result<Vec<PrinterId>, ListLibraryError> {
    ids_in(root, PrinterProfile::FOLDER)
}

pub fn list_material_ids(root: &Path) -> Result<Vec<MaterialId>, ListLibraryError> {
    ids_in(root, MaterialProfile::FOLDER)
}

pub fn list_material_extension_ids(root: &Path) -> Result<Vec<MaterialId>, ListLibraryError> {
    ids_in(root, EXTENSIONS_FOLDER)
}

// Resin extensions: a user's overrides on top of a resin profile, kept
// apart from the profile itself.

const EXTENSIONS_FOLDER: &str = "materials/extensions";

pub fn material_extension_path_in_library(root: &Path, id: &MaterialId) -> PathBuf {
    root.join(EXTENSIONS_FOLDER).join(format!("{id}.toml"))
}

/// A file that would not read, or that is a whole resin profile rather
/// than an extension.
#[derive(Debug)]
pub enum LoadMaterialExtensionError {
    Source(LoadMaterialProfileError),
    NotAnExtension { path: PathBuf },
}

impl fmt::Display for LoadMaterialExtensionError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadMaterialExtensionError::Source(cause) => cause.fmt(out),
            LoadMaterialExtensionError::NotAnExtension { path } => {
                let shown = path.display();
                write!(out, "the file {shown} is not a material extension: its extension flag is missing or false")
            }
        }
    }
}

impl Error for LoadMaterialExtensionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            LoadMaterialExtensionError::Source(cause) => Some(cause),
            LoadMaterialExtensionError::NotAnExtension { .. } => None,
        }
    }
}

pub fn load_material_extension_from_path(path: &Path) -> Result<MaterialProfile, LoadMaterialExtensionError> {
    let profile = load_material_profile_from_path(path).map_err(LoadMaterialExtensionError::Source)?;
    match profile.extension {
        true => Ok(profile),
        false => Err(LoadMaterialExtensionError::NotAnExtension { path: path.to_path_buf() }),
    }
}

pub fn load_material_extension_from_library(root: &Path, id: &MaterialId) -> Result<MaterialProfile, LoadMaterialExtensionError> {
    load_material_extension_from_path(&material_extension_path_in_library(root, id))
}

/// An extension that would not be written.
#[derive(Debug)]
pub struct SaveMaterialExtensionError {
    pub path: PathBuf,
    pub kind: SaveMaterialExtensionErrorKind,
}

#[derive(Debug)]
pub enum SaveMaterialExtensionErrorKind {
    NotAnExtension,
    TomlSerialize(toml_edit::ser::Error),
    TomlReparse(toml_edit::TomlError),
    Io(io::Error),
}

impl fmt::Display for SaveMaterialExtensionError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "failed to save material extension to {}: ", self.path.display())?;
        match &self.kind {
            SaveMaterialExtensionErrorKind::NotAnExtension => out.write_str("the profile's extension flag is false"),
            SaveMaterialExtensionErrorKind::TomlSerialize(cause) => cause.fmt(out),
            SaveMaterialExtensionErrorKind::TomlReparse(cause) => cause.fmt(out),
            SaveMaterialExtensionErrorKind::Io(cause) => cause.fmt(out),
        }
    }
}

impl Error for SaveMaterialExtensionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            SaveMaterialExtensionErrorKind::NotAnExtension => None,
            SaveMaterialExtensionErrorKind::TomlSerialize(cause) => Some(cause),
            SaveMaterialExtensionErrorKind::TomlReparse(cause) => Some(cause),
            SaveMaterialExtensionErrorKind::Io(cause) => Some(cause),
        }
    }
}

/// Writes an extension. Sections it leaves at their defaults get a note
/// that their values come from the resin profile underneath.
pub fn save_material_extension_to_path(path: &Path, profile: &MaterialProfile) -> Result<(), SaveMaterialExtensionError> {
    let failed = |kind| SaveMaterialExtensionError { path: path.to_path_buf(), kind };
    if !profile.extension {
        return Err(failed(SaveMaterialExtensionErrorKind::NotAnExtension));
    }
    let mut text = toml_edit::ser::to_string_pretty(profile)
        .map_err(|cause| failed(SaveMaterialExtensionErrorKind::TomlSerialize(cause)))?;
    let inherited = [
        ("intent", profile.intent == MaterialIntent::default()),
        ("defaults", profile.defaults == MaterialDefaults::default()),
    ];
    for (section, _) in inherited.into_iter().filter(|(_, untouched)| *untouched) {
        let header = format!("[{section}]");
        text = text.replace(&header, &format!("# Values are inherited from the parent profile.\n{header}"));
    }
    write_with_folder(path, text.as_bytes()).map_err(|cause| failed(SaveMaterialExtensionErrorKind::Io(cause)))
}

pub fn save_material_extension_to_library(root: &Path, profile: &MaterialProfile) -> Result<(), SaveMaterialExtensionError> {
    save_material_extension_to_path(&material_extension_path_in_library(root, &profile.identity.id), profile)
}

// Deleting.

/// A profile file that could not be removed.
#[derive(Debug)]
pub struct DeleteProfileError {
    pub path: PathBuf,
    pub source: io::Error,
}

impl fmt::Display for DeleteProfileError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "failed to delete {}: {}", self.path.display(), self.source)
    }
}

impl Error for DeleteProfileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

pub type DeleteMaterialProfileError = DeleteProfileError;
pub type DeleteMaterialExtensionError = DeleteProfileError;

fn delete_file(path: PathBuf) -> Result<(), DeleteProfileError> {
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(source) => Err(DeleteProfileError { path, source }),
    }
}

pub fn delete_material_profile_from_library(root: &Path, id: &MaterialId) -> Result<(), DeleteProfileError> {
    delete_file(material_profile_path_in_library(root, id))
}

pub fn delete_material_extension_from_library(root: &Path, id: &MaterialId) -> Result<(), DeleteProfileError> {
    delete_file(material_extension_path_in_library(root, id))
}

// Printer pictures.

/// A printer picture that could not be copied into the library.
#[derive(Debug)]
pub struct SavePrinterImageError {
    pub path: PathBuf,
    pub kind: SavePrinterImageErrorKind,
}

#[derive(Debug)]
pub enum SavePrinterImageErrorKind {
    CreateDirectory(io::Error),
    Copy { source_path: PathBuf, source: io::Error },
}

impl fmt::Display for SavePrinterImageError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "failed to save printer image to {}: ", self.path.display())?;
        match &self.kind {
            SavePrinterImageErrorKind::CreateDirectory(cause) => write!(out, "cannot create the images directory: {cause}"),
            SavePrinterImageErrorKind::Copy { source_path, source } => {
                write!(out, "cannot copy {}: {source}", source_path.display())
            }
        }
    }
}

impl Error for SavePrinterImageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            SavePrinterImageErrorKind::CreateDirectory(cause) => Some(cause),
            SavePrinterImageErrorKind::Copy { source, .. } => Some(source),
        }
    }
}

/// Copies a picture into the library as `images/<id>.<ext>` (the
/// extension lower-cased, `png` if it has none) and returns that path,
/// relative to the library.
pub fn save_printer_image_to_library(root: &Path, id: &PrinterId, source: &Path) -> Result<ImageAssetRef, SavePrinterImageError> {
    let extension = match source.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if !extension.is_empty() => extension.to_ascii_lowercase(),
        _ => "png".to_owned(),
    };
    let relative = format!("images/{id}.{extension}");
    let path = root.join(&relative);
    if let Err(cause) = fs::create_dir_all(root.join("images")) {
        return Err(SavePrinterImageError { path, kind: SavePrinterImageErrorKind::CreateDirectory(cause) });
    }
    if let Err(cause) = fs::copy(source, &path) {
        let kind = SavePrinterImageErrorKind::Copy { source_path: source.to_path_buf(), source: cause };
        return Err(SavePrinterImageError { path, kind });
    }
    Ok(ImageAssetRef(relative))
}
