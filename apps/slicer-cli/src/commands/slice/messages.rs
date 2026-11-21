// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! What `slice` prints. The wording is interface: scripts and the tests
//! read these lines.
use std::collections::HashSet;
use std::fmt::Display;
use std::io::Write;

use sc_mesh_core::indexed_mesh::{MeshErrorReport, MeshFix, group_errors_by_variant};
use sc_print_glue::{UnsliceableScene, UnsliceableSelection};
use sc_print_job::{Island, PrintRisk, Severity};
use sc_project_formats::ReadProjectWarning;
use sc_scene::repair::SceneRepairOutcome;
use sc_sl1cer::error::{InvalidLeafGeometry, SliceSceneError};

use crate::commands::report::{display_threemf_warning, show_applied_fixes, show_refusal_warnings};

/// Islands listed one by one before the rest are only counted.
const ISLANDS_LISTED: usize = 10;

/// Project reads warn about mesh content the same way plain 3MF reads do.
/// The project-only warnings concern saved profiles and versions, which a
/// command-line slice ignores: its profiles come from the flags.
pub(super) fn project_warnings(warnings: &[ReadProjectWarning], out: &mut dyn Write) {
    for warning in warnings {
        if let ReadProjectWarning::ThreeMf(warning) = warning {
            display_threemf_warning(warning, out);
        }
    }
}

/// One summary line when the repair changed anything (plus every fix with
/// `--verbose`), then a warning per mesh it had to leave alone.
pub(super) fn repair_summary(outcomes: &[SceneRepairOutcome], verbose: bool, out: &mut dyn Write) {
    let mut fixes: Vec<MeshFix> = Vec::new();
    for outcome in outcomes {
        // A mesh shared by several parts reports the same fix once per use.
        let mut seen = HashSet::new();
        fixes.extend(
            outcome
                .fixes
                .iter()
                .filter(|fix| seen.insert(format!("{fix:?}")))
                .cloned(),
        );
    }
    if !fixes.is_empty() {
        let [mut removed, mut flipped, mut duplicates, mut detached] = [0usize; 4];
        for fix in &fixes {
            match fix {
                MeshFix::RemovedZeroLengthEdgeFace { .. } => removed += 1,
                MeshFix::FlippedFaceOrientation { .. } => flipped += 1,
                MeshFix::RemovedDuplicateFace { .. } => duplicates += 1,
                MeshFix::DetachedSharedVertex { .. } => detached += 1,
                MeshFix::RecomputedNormal { .. } => {}
            }
        }
        // Shell separation is mentioned only when it did something, so the
        // usual line stays short.
        let shells = if duplicates + detached > 0 {
            format!(
                " Separated touching shells: {duplicates} duplicate faces removed, {detached} shared vertices detached."
            )
        } else {
            String::new()
        };
        let _ = writeln!(
            out,
            "Repair: {removed} degenerate faces removed, {flipped} faces flipped.{shells}"
        );
        if verbose {
            show_applied_fixes(&fixes, out);
        }
    }
    show_refusal_warnings(outcomes, out);
}

/// Why the scene could not be prepared for slicing. Sitting at or below the
/// plate is only reachable with `--no-auto-place`, so moving it is the advice.
pub(super) fn unsliceable(why: UnsliceableSelection) -> anyhow::Error {
    match why {
        UnsliceableSelection::LayerHeight(err) => {
            anyhow::anyhow!("no layer height resolves for this printer and material: {err}")
        }
        UnsliceableSelection::Scene(UnsliceableScene::TopAtOrBelowPlate { z_max_mm }) => anyhow::anyhow!(
            "input scene has its top at z = {z_max_mm} mm. The slicer slices upward from the build plate at z = 0; reposition your model(s) above z = 0 before slicing.",
        ),
        UnsliceableSelection::Scene(other) => anyhow::anyhow!("cannot slice this scene: {other}"),
    }
}

/// The engine's own summary of which meshes failed validation; with
/// `--verbose`, every error per mesh grouped by kind; then how to fix it.
pub(super) fn validation_failure(error: &SliceSceneError, verbose: bool, out: &mut dyn Write) {
    let _ = writeln!(out, "{error}");
    if verbose && let SliceSceneError::LeavesValidationFailed(failures) = error {
        for failure in failures {
            let label = match &failure.leaf_name {
                Some(name) => format!("leaf #{} (\"{name}\")", failure.leaf.get()),
                None => format!("leaf #{}", failure.leaf.get()),
            };
            let _ = writeln!(out, "\n{label}:");
            match &failure.geometry {
                InvalidLeafGeometry::Vertices(errors) => grouped(errors, out),
                InvalidLeafGeometry::Faces(errors) => grouped(errors, out),
                InvalidLeafGeometry::Manifold(errors) => grouped(errors, out),
            }
        }
    }
    let more = if verbose {
        ""
    } else {
        ", or re-run with --verbose for the full error list"
    };
    let _ = writeln!(out, "\nRun `sc-slicer-cli fix` to repair the mesh{more}.");
}

/// `Kind (n):` then `  - error` lines, kinds in order of first appearance,
/// a blank line between kinds.
fn grouped<E: MeshErrorReport + Display>(errors: &[E], out: &mut dyn Write) {
    for (at, group) in group_errors_by_variant(errors).iter().enumerate() {
        if at > 0 {
            let _ = writeln!(out);
        }
        let _ = writeln!(out, "{} ({}):", group.variant_name, group.members.len());
        for member in &group.members {
            let _ = writeln!(out, "  - {member}");
        }
    }
}

/// `--risks`: a count by severity, then each kind present with its findings,
/// kinds in their fixed order and findings by first layer.
pub(super) fn risks(mut found: Vec<PrintRisk>) {
    print!("{}", risk_report(&mut found));
}

fn risk_report(found: &mut [PrintRisk]) -> String {
    found.sort_by_key(|risk| (risk.kind, risk.first_layer));
    let count = |severity| found.iter().filter(|risk| risk.severity == severity).count();
    let mut text = format!(
        "Print risks: {} errors, {} warnings\n",
        count(Severity::Error),
        count(Severity::Warning)
    );
    for kind in found.chunk_by(|a, b| a.kind == b.kind) {
        text += &format!("{} ({})\n", kind[0].kind.title(), kind.len());
        for risk in kind {
            text += &format!("  {}\n", risk.summary());
        }
    }
    text
}

/// `--islands`: a count, then the first few with their layer (counting from
/// 1), area and pixel position.
pub(super) fn islands(found: &[Island]) {
    print!("{}", island_report(found));
}

fn island_report(found: &[Island]) -> String {
    if found.is_empty() {
        return String::from("Islands: none (every region touches the layer below).\n");
    }
    let mut text = format!("Islands: {} unsupported region(s) need supports:\n", found.len());
    for island in found.iter().take(ISLANDS_LISTED) {
        text += &format!(
            "  layer {}: {:.3} mm2 at pixel ({:.0}, {:.0})\n",
            island.layer + 1,
            island.area_mm2,
            island.centroid_px.0,
            island.centroid_px.1
        );
    }
    if found.len() > ISLANDS_LISTED {
        text += &format!("  ... and {} more\n", found.len() - ISLANDS_LISTED);
    }
    text
}

#[cfg(test)]
#[path = "tests_messages.rs"]
mod tests;
