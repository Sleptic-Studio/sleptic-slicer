// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::error::ReadThreeMfProjectError;
use crate::*;
use sc_print_primitives::{AaLevel, MaterialId, PositiveFiniteMm, PrinterId};
use serde::{Deserialize, Serialize};
#[derive(Default, Serialize, Deserialize)]
struct Profiles {
    #[serde(skip_serializing_if = "Option::is_none")]
    printer: Option<PrinterId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resin: Option<MaterialId>,
}
#[derive(Default, Serialize, Deserialize)]
struct Print {
    #[serde(skip_serializing_if = "Option::is_none")]
    first_layer_height_mm: Option<PositiveFiniteMm>,
    #[serde(skip_serializing_if = "Option::is_none")]
    process_overrides: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aa_level: Option<AaLevel>,
}
#[derive(Default, Serialize, Deserialize)]
struct Slices {
    fingerprint: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Settings {
    version: i64,
    #[serde(default)]
    profiles: Profiles,
    #[serde(skip_serializing_if = "Option::is_none")]
    print: Option<Print>,
    #[serde(skip_serializing_if = "Option::is_none")]
    slices: Option<Slices>,
}
pub(super) fn encode(
    p: &ProjectPayload,
    fp: Option<&str>,
) -> Result<Vec<u8>, toml_edit::ser::Error> {
    let print = if p.first_layer_height.is_some()
        || p.process_overrides.is_some()
        || p.aa_level.is_some()
    {
        Some(Print {
            first_layer_height_mm: p.first_layer_height,
            process_overrides: p.process_overrides.clone(),
            aa_level: p.aa_level,
        })
    } else {
        None
    };
    let s = Settings {
        version: 1,
        profiles: Profiles {
            printer: p.printer.clone(),
            resin: p.resin.clone(),
        },
        print,
        slices: fp.map(|f| Slices {
            fingerprint: Some(f.into()),
        }),
    };
    Ok(toml_edit::ser::to_string_pretty(&s)?.into_bytes())
}
pub(super) fn decode(
    bytes: &[u8],
    warnings: &mut Vec<ReadProjectWarning>,
) -> Result<(Option<ProjectPayload>, Option<String>), ReadThreeMfProjectError> {
    let text = std::str::from_utf8(bytes)?;
    #[derive(Deserialize)]
    struct Version {
        version: i64,
    }
    let version: Version = toml_edit::de::from_str(text)?;
    if version.version != 1 {
        warnings.push(ReadProjectWarning::NewerProjectData {
            version: version.version,
        });
        return Ok((None, None));
    }
    let s: Settings = toml_edit::de::from_str(text)?;
    let print = s.print.unwrap_or_default();
    Ok((
        Some(ProjectPayload {
            printer: s.profiles.printer,
            resin: s.profiles.resin,
            first_layer_height: print.first_layer_height_mm,
            process_overrides: print.process_overrides,
            aa_level: print.aa_level,
            embedded: vec![],
        }),
        s.slices.and_then(|s| s.fingerprint),
    ))
}
/// Where a carried profile lives under `/Metadata/profiles/`, without the
/// `.toml`: printers, resins, and resin extensions each have a folder.
pub(super) fn embedded_path(profile: &EmbeddedProfile) -> String {
    match (&profile.profile, profile.extension) {
        (NamedProfile::Printer(id), _) => format!("printers/{id}"),
        (NamedProfile::Resin(id), false) => format!("materials/{id}"),
        (NamedProfile::Resin(id), true) => format!("materials/extensions/{id}"),
    }
}

/// The carried profile at `tail` (a path under `/Metadata/profiles/`), the
/// inverse of [`embedded_path`]; `None` for anything else.
pub(super) fn embedded(tail: &str, bytes: &[u8]) -> Option<EmbeddedProfile> {
    let id = |rest: &str| rest.strip_suffix(".toml").map(str::to_owned);
    let (profile, extension) = if let Some(rest) = tail.strip_prefix("printers/") {
        (NamedProfile::Printer(id(rest)?.parse().ok()?), false)
    } else if let Some(rest) = tail.strip_prefix("materials/extensions/") {
        (NamedProfile::Resin(id(rest)?.parse().ok()?), true)
    } else {
        (NamedProfile::Resin(id(tail.strip_prefix("materials/")?)?.parse().ok()?), false)
    };
    Some(EmbeddedProfile { profile, extension, bytes: bytes.to_vec() })
}
