// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Presentation of the shared risk report. Geometry remains in sc-print-job.
use crate::{MainWindow, RiskRow};
use sc_print_job::{PrintRisk, RiskKind, Severity};
use sc_sl1cer::profile::SliceProfile;

/// Format at the displayed precision before removing a rounded negative zero.
fn coordinate(value: f64) -> String {
    let text = format!("{value:.1}");
    if text == "-0.0" { "0.0".into() } else { text }
}

pub(crate) fn title(kind: RiskKind) -> slint::SharedString {
    match kind {
        RiskKind::UnsupportedIsland => crate::i18n::tr!("Unsupported islands"),
        RiskKind::SuctionCup => crate::i18n::tr!("Suction cups"),
        RiskKind::TrappedCavity => crate::i18n::tr!("Trapped resin cavities"),
        RiskKind::ThinWall => crate::i18n::tr!("Thin walls / features"),
        RiskKind::LargePeelArea => crate::i18n::tr!("Large peel area"),
        RiskKind::OutOfBounds => crate::i18n::tr!("Outside build volume"),
        RiskKind::Overlap => crate::i18n::tr!("Overlapping models"),
    }
}
fn explanation(kind: RiskKind) -> slint::SharedString {
    match kind {
        RiskKind::UnsupportedIsland => {
            crate::i18n::tr!("This region has no contact with the layer below.")
        }
        RiskKind::SuctionCup => crate::i18n::tr!(
            "An enclosed void starts above a sealed floor and can pull a vacuum during peeling."
        ),
        RiskKind::TrappedCavity => crate::i18n::tr!(
            "This void has no connection to the outside anywhere in the layer stack."
        ),
        RiskKind::ThinWall => crate::i18n::tr!(
            "Material disappears under minimum-feature erosion and reconstruction."
        ),
        RiskKind::LargePeelArea => crate::i18n::tr!(
            "Lit area exceeds the peel limit or changes abruptly from the previous layer."
        ),
        RiskKind::OutOfBounds => {
            crate::i18n::tr!("Model geometry extends beyond the printer's build envelope.")
        }
        RiskKind::Overlap => crate::i18n::tr!("Separate model volumes intersect."),
    }
}
fn fix(kind: RiskKind) -> slint::SharedString {
    match kind {
        RiskKind::UnsupportedIsland => crate::i18n::tr!("Add supports."),
        RiskKind::SuctionCup | RiskKind::TrappedCavity => {
            crate::i18n::tr!("Add drain / vent holes.")
        }
        RiskKind::ThinWall => {
            crate::i18n::tr!("Thicken the feature or lower the minimum after calibration.")
        }
        RiskKind::LargePeelArea => crate::i18n::tr!("Tilt the model or reduce the cross-section."),
        RiskKind::OutOfBounds => {
            crate::i18n::tr!("Move or scale the model inside the build volume.")
        }
        RiskKind::Overlap => crate::i18n::tr!("Arrange the models apart."),
    }
}
pub(crate) fn publish(ui: &MainWindow, risks: &[PrintRisk], p: &SliceProfile) {
    let mut rows = Vec::new();
    let res = p.pixel_resolution();
    for (i, r) in risks.iter().enumerate() {
        let count = risks.iter().filter(|n| n.kind == r.kind).count();
        let mut x0 = r.bounds_px[0] as f32 / res.x() as f32;
        let mut x1 = r.bounds_px[2] as f32 / res.x() as f32;
        let mut y0 = r.bounds_px[1] as f32 / res.y() as f32;
        let mut y1 = r.bounds_px[3] as f32 / res.y() as f32;
        if p.mirror().x {
            (x0, x1) = (1.0 - x1, 1.0 - x0);
        }
        if p.mirror().y {
            (y0, y1) = (1.0 - y1, 1.0 - y0);
        }
        rows.push(RiskRow {
            title: format!(
                "{} {}–{} · {} × {} mm",
                crate::i18n::tr!("Layers"),
                r.first_layer + 1,
                r.last_layer + 1,
                coordinate(
                    f64::from(r.bounds_px[2] - r.bounds_px[0]) * p.build_volume().x_mm()
                        / f64::from(res.x())
                ),
                coordinate(
                    f64::from(r.bounds_px[3] - r.bounds_px[1]) * p.build_volume().y_mm()
                        / f64::from(res.y())
                )
            )
            .into(),
            detail: explanation(r.kind),
            fix: fix(r.kind),
            group: if i == 0 || risks[i - 1].kind != r.kind {
                format!("{} ({count})", title(r.kind)).into()
            } else {
                Default::default()
            },
            error: r.severity == Severity::Error,
            layer: r.worst_layer as i32,
            last: r.last_layer as i32,
            action: match r.kind {
                RiskKind::UnsupportedIsland => 1,
                RiskKind::SuctionCup | RiskKind::TrappedCavity => 2,
                _ => 0,
            },
            x0,
            y0,
            x1,
            y1,
            kind: r.kind as i32,
            px: r.position_mm.0 as f32,
            py: r.position_mm.1 as f32,
        });
    }
    ui.set_risk_error_count(
        risks
            .iter()
            .filter(|r| r.severity == Severity::Error)
            .count() as i32,
    );
    ui.set_selected_risk(-1);
    ui.set_expanded_risk_kind(-1);
    ui.set_print_risks(slint::ModelRc::new(slint::VecModel::from(rows)));
}

#[cfg(test)]
#[path = "tests_print_risks.rs"]
mod tests;
