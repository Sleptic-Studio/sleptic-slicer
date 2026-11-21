// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! A two-phase reader: classify/map source files, then combine resin calibrations.
pub mod material;
pub mod printer;
pub mod problem;
use crate::documents::Document;
use material::MaterialRecord;
use printer::PrinterRecord;
pub use problem::{Problem, RecordKind, RecordProblem};
use sc_print_profiles::{MaterialId, MaterialProfile};
use std::{
    collections::{BTreeMap, btree_map::Entry},
    fmt, fs, io,
    path::{Path, PathBuf},
};
#[derive(Debug, Default, Clone, Copy)]
pub struct JsonCatalogReader;
impl JsonCatalogReader {
    pub fn new() -> Self {
        Self
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialConflict {
    SharedFields,
    PrinterSettings,
}
#[derive(Debug)]
pub enum JsonCatalogError {
    ReadFile {
        path: PathBuf,
        source: io::Error,
    },
    ReadDir {
        path: PathBuf,
        source: io::Error,
    },
    ParseJson {
        path: PathBuf,
        source: serde_json::Error,
    },
    /// Record `index` of the file could not become a profile.
    Record {
        path: PathBuf,
        index: usize,
        source: RecordProblem,
    },
    UnrecognisedRecord {
        path: PathBuf,
    },
    ConflictingMaterial {
        path: PathBuf,
        index: usize,
        id: String,
        conflict: MaterialConflict,
    },
}
impl fmt::Display for JsonCatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadFile { path, source } => {
                write!(f, "Cannot read source {}: {source}", path.display())
            }
            Self::ReadDir { path, source } => write!(
                f,
                "Cannot list source directory {}: {source}",
                path.display()
            ),
            Self::ParseJson { path, source } => {
                write!(f, "Invalid profile JSON in {}: {source}", path.display())
            }
            Self::Record { path, index, source } => write!(f, "{} record {index}: {source}", path.display()),
            Self::UnrecognisedRecord { path } => {
                write!(
                    f,
                    "No printer or resin record type in {} record 0",
                    path.display()
                )
            }
            Self::ConflictingMaterial {
                path,
                index,
                id,
                conflict,
            } => write!(
                f,
                "{} resin record {index}, conflicting resin {id:?}: {}",
                path.display(),
                match conflict {
                    MaterialConflict::SharedFields => "identity or shared settings differ",
                    MaterialConflict::PrinterSettings =>
                        "calibrations disagree for the same printer",
                }
            ),
        }
    }
}
impl std::error::Error for JsonCatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        let cause: &dyn std::error::Error = match self {
            Self::ReadFile { source, .. } | Self::ReadDir { source, .. } => source,
            Self::ParseJson { source, .. } => source,
            Self::Record { source, .. } => source,
            _ => return None,
        };
        Some(cause)
    }
}
fn source_files(inputs: &[&Path]) -> Result<Vec<PathBuf>, JsonCatalogError> {
    let mut result = Vec::new();
    for input in inputs {
        if input.is_dir() {
            result.extend(json_children(input)?);
        } else {
            result.push(input.to_path_buf());
        }
    }
    Ok(result)
}

fn json_children(directory: &Path) -> Result<Vec<PathBuf>, JsonCatalogError> {
    let list = || -> io::Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            let json = path.extension().and_then(|ext| ext.to_str());
            if path.is_file() && json.is_some_and(|ext| ext.eq_ignore_ascii_case("json")) {
                paths.push(path);
            }
        }
        paths.sort();
        Ok(paths)
    };
    list().map_err(|source| JsonCatalogError::ReadDir {
        path: directory.to_path_buf(),
        source,
    })
}
fn combine(
    materials: &mut BTreeMap<MaterialId, MaterialProfile>,
    incoming: MaterialProfile,
    path: &Path,
    index: usize,
) -> Result<(), JsonCatalogError> {
    let id = incoming.identity.id.clone();
    let existing = match materials.entry(id) {
        Entry::Vacant(slot) => {
            slot.insert(incoming);
            return Ok(());
        }
        Entry::Occupied(slot) => slot.into_mut(),
    };
    let problem = |conflict| JsonCatalogError::ConflictingMaterial {
        path: path.to_path_buf(),
        index,
        id: incoming.identity.id.to_string(),
        conflict,
    };
    if existing.identity != incoming.identity
        || existing.intent != incoming.intent
        || existing.defaults != incoming.defaults
    {
        return Err(problem(MaterialConflict::SharedFields));
    }
    for (target, calibration) in &incoming.per_printer {
        if existing
            .per_printer
            .get(target)
            .is_some_and(|old| old != calibration)
        {
            return Err(problem(MaterialConflict::PrinterSettings));
        }
    }
    existing.per_printer.extend(incoming.per_printer);
    Ok(())
}
#[derive(PartialEq, Eq)]
enum SourceKind {
    Printer,
    Material,
    Unknown,
}

fn classify(text: &str) -> Result<Option<SourceKind>, serde_json::Error> {
    let values: Vec<serde_json::Value> = serde_json::from_str(text)?;
    Ok(values.first().map(|first| {
        if first.get("presetId").is_some() {
            SourceKind::Printer
        } else if first.get("templateId").is_some() {
            SourceKind::Material
        } else {
            SourceKind::Unknown
        }
    }))
}

impl JsonCatalogReader {
    pub fn read(&self, inputs: &[&Path]) -> Result<Vec<Document>, JsonCatalogError> {
        let mut printers = Vec::new();
        let mut materials = BTreeMap::new();
        for path in source_files(inputs)? {
            let text = fs::read_to_string(&path).map_err(|source| JsonCatalogError::ReadFile {
                path: path.clone(),
                source,
            })?;
            let parse_error = |source| JsonCatalogError::ParseJson {
                path: path.clone(),
                source,
            };
            let Some(kind) = classify(&text).map_err(parse_error)? else {
                continue;
            };
            if kind == SourceKind::Unknown {
                return Err(JsonCatalogError::UnrecognisedRecord { path });
            }
            if kind == SourceKind::Printer {
                let records: Vec<PrinterRecord> =
                    serde_json::from_str(&text).map_err(parse_error)?;
                for (index, record) in records.into_iter().enumerate() {
                    let (profile, image) = printer::convert(record).map_err(|source| {
                        JsonCatalogError::Record {
                            path: path.clone(),
                            index,
                            source,
                        }
                    })?;
                    let image_source =
                        image.map(|name| path.parent().unwrap_or(Path::new(".")).join(name));
                    printers.push(Document::Printer {
                        profile,
                        image_source,
                    });
                }
            } else {
                let records: Vec<MaterialRecord> =
                    serde_json::from_str(&text).map_err(parse_error)?;
                for (index, record) in records.into_iter().enumerate() {
                    let profile = material::convert(record).map_err(|source| {
                        JsonCatalogError::Record {
                            path: path.clone(),
                            index,
                            source,
                        }
                    })?;
                    combine(&mut materials, profile, &path, index)?;
                }
            }
        }
        printers.extend(materials.into_values().map(Document::Material));
        Ok(printers)
    }
}
