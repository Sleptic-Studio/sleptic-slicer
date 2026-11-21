// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The Sleptic part of a project file: the chosen printer and resin, the
//! process settings, and any profiles the project carries with it.
use sc_print_primitives::{AaLevel, MaterialId, PositiveFiniteMm, PrinterId};

/// A profile a project names, by its kind and id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NamedProfile {
    Printer(PrinterId),
    Resin(MaterialId),
}

/// A profile a project names that is not installed. With `embedded`, the
/// file carries a copy of it, so the project still opens complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileRefWarning {
    pub profile: NamedProfile,
    pub embedded: bool,
}

/// What a project file holds besides the scene, all optional: the choices
/// made for it, the process settings changed from the profiles', and the
/// profiles it carries so it opens the same elsewhere.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectPayload {
    /// The printer and the resin chosen.
    pub printer: Option<PrinterId>,
    pub resin: Option<MaterialId>,
    /// Settings that override the profiles: the first layer's height, the
    /// anti-aliasing level, and the rest as a TOML table.
    pub first_layer_height: Option<PositiveFiniteMm>,
    pub aa_level: Option<AaLevel>,
    pub process_overrides: Option<String>,
    /// Copies of the profiles named above (and their extensions).
    pub embedded: Vec<EmbeddedProfile>,
}
/// A profile file the project carries, as its bytes: a printer, a resin,
/// or (with `extension`) a resin's extension file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddedProfile {
    pub profile: NamedProfile,
    pub extension: bool,
    pub bytes: Vec<u8>,
}
impl ProjectPayload {
    /// Whether the project carries its own copy of `wanted` (for a resin,
    /// the profile itself: an extension alone does not count).
    fn carries(&self, wanted: &NamedProfile) -> bool {
        self.embedded.iter().any(|copy| !copy.extension && copy.profile == *wanted)
    }

    pub fn embeds_printer(&self, id: &PrinterId) -> bool {
        self.carries(&NamedProfile::Printer(id.clone()))
    }

    pub fn embeds_material(&self, id: &MaterialId) -> bool {
        self.carries(&NamedProfile::Resin(id.clone()))
    }

    /// What to tell the user about the project's printer and resin when
    /// `printer_known` / `resin_known` say the library lacks them: missing
    /// outright, or only available as the copy embedded in the project.
    /// Printer first; nothing for one the project does not name.
    pub fn profile_ref_warnings(
        &self,
        mut printer_known: impl FnMut(&PrinterId) -> bool,
        mut resin_known: impl FnMut(&MaterialId) -> bool,
    ) -> Vec<ProfileRefWarning> {
        let printer = self.printer.as_ref().filter(|id| !printer_known(id)).map(|id| ProfileRefWarning {
            embedded: self.embeds_printer(id),
            profile: NamedProfile::Printer(id.clone()),
        });
        let resin = self.resin.as_ref().filter(|id| !resin_known(id)).map(|id| ProfileRefWarning {
            embedded: self.embeds_material(id),
            profile: NamedProfile::Resin(id.clone()),
        });
        printer.into_iter().chain(resin).collect()
    }
}
