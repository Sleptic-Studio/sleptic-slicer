// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! First-run setup (Chitubox / Lychee style): pick the look, the printer
//! and the resin before the first model. Pure list building lives here so it
//! is testable; `AppState` wires the window callbacks.

use sc_print_primitives::{MaterialId, PrinterId};
use sc_print_profiles::CalibrationTarget;

use crate::profile_catalog::InstalledProfiles;

/// One line of a setup list: a manufacturer / brand header, or an item.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub header: bool,
    pub title: String,
    pub subtitle: String,
    pub badge: String,
    pub selected: bool,
}

fn matches(query: &str, parts: &[&str]) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || query
            .split_whitespace()
            .all(|word| parts.iter().any(|p| p.to_lowercase().contains(word)))
}

/// Printers grouped by manufacturer (headers in between), filtered by
/// `query`. Returns the rows and, per row, the printer it selects.
pub(crate) fn printer_rows(
    catalog: &InstalledProfiles,
    query: &str,
    selected: Option<&PrinterId>,
) -> (Vec<Row>, Vec<Option<PrinterId>>) {
    let mut printers: Vec<_> = catalog
        .printers()
        .iter()
        .filter(|p| matches(query, &[&p.printer_maker, &p.name]))
        .collect();
    printers.sort_by(|a, b| {
        a.printer_maker
            .to_lowercase()
            .cmp(&b.printer_maker.to_lowercase())
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    let mut rows = Vec::new();
    let mut ids = Vec::new();
    let mut last = None::<String>;
    for p in printers {
        if last.as_deref() != Some(p.printer_maker.as_str()) {
            rows.push(Row {
                header: true,
                title: p.printer_maker.clone(),
                subtitle: String::new(),
                badge: String::new(),
                selected: false,
            });
            ids.push(None);
            last = Some(p.printer_maker.clone());
        }
        let v = p.printer_plate_volume;
        rows.push(Row {
            header: false,
            title: p.name.clone(),
            subtitle: format!(
                "{:.0} \u{d7} {:.0} \u{d7} {:.0} mm",
                v.x_mm(),
                v.y_mm(),
                v.z_mm()
            ),
            badge: String::new(),
            selected: selected == Some(&p.id),
        });
        ids.push(Some(p.id.clone()));
    }
    (rows, ids)
}

/// Resins, the ones tuned for `printer` first ("Tuned for your printer"),
/// then the rest by brand; filtered by `query`.
pub(crate) fn resin_rows(
    catalog: &InstalledProfiles,
    printer: Option<&PrinterId>,
    query: &str,
    selected: Option<&MaterialId>,
    tuned_label: &str,
) -> (Vec<Row>, Vec<Option<MaterialId>>) {
    let target = printer.map(CalibrationTarget::from_printer);
    let mut resins: Vec<_> = catalog
        .resin_list()
        .iter()
        .filter(|m| matches(query, &[&m.brand, &m.name, &m.resin_family]))
        .map(|m| {
            let tuned = target
                .as_ref()
                .is_some_and(|t| m.calibrated_for.contains(t));
            (tuned, m)
        })
        .collect();
    resins.sort_by(|(ta, a), (tb, b)| {
        tb.cmp(ta)
            .then(a.brand.to_lowercase().cmp(&b.brand.to_lowercase()))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    let mut rows = Vec::new();
    let mut ids = Vec::new();
    let mut last = None::<String>;
    for (tuned, m) in resins {
        let group = if tuned {
            tuned_label.to_owned()
        } else {
            m.brand.clone()
        };
        if last.as_deref() != Some(group.as_str()) {
            rows.push(Row {
                header: true,
                title: group.clone(),
                subtitle: String::new(),
                badge: String::new(),
                selected: false,
            });
            ids.push(None);
            last = Some(group);
        }
        rows.push(Row {
            header: false,
            title: if tuned {
                format!("{} {}", m.brand, m.name)
            } else {
                m.name.clone()
            },
            subtitle: m.resin_family.clone(),
            badge: if tuned { tuned_label.to_owned() } else { String::new() },
            selected: selected == Some(&m.id),
        });
        ids.push(Some(m.id.clone()));
    }
    (rows, ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> InstalledProfiles {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/profiles");
        InstalledProfiles::scan_libraries(&sc_print_profiles::ProfileLibraries::new(None, vec![root]))
    }

    // R48: the first-run printer list is grouped by manufacturer, searchable,
    // and every item row maps back to its printer.
    #[test]
    fn printer_list_is_grouped_searchable_and_maps_rows_to_printers() {
        let catalog = catalog();
        let (rows, ids) = printer_rows(&catalog, "", None);
        assert_eq!(rows.len(), ids.len());
        assert!(rows[0].header && ids[0].is_none());
        let items = rows.iter().filter(|r| !r.header).count();
        assert_eq!(items, catalog.printers().len());
        let sleptic = catalog
            .printers()
            .iter()
            .find(|p| p.name.contains("Sleptic"))
            .expect("Sleptic's printer is bundled");
        let (rows, ids) = printer_rows(&catalog, "sleptic msla", Some(&sleptic.id));
        let picked: Vec<_> = rows.iter().zip(&ids).filter(|(r, _)| !r.header).collect();
        assert_eq!(picked.len(), 1);
        assert!(picked[0].0.selected && picked[0].1.as_ref() == Some(&sleptic.id));
        assert!(picked[0].0.subtitle.ends_with("mm"));
    }

    #[test]
    fn resins_tuned_for_the_printer_come_first() {
        let catalog = catalog();
        let Some(tuned) = catalog.resin_list().iter().find_map(|m| {
            m.calibrated_for.iter().find_map(|t| match t {
                CalibrationTarget::Printer(p) => Some(p.as_printer_id().clone()),
                CalibrationTarget::Generic => None,
            })
        }) else {
            return; // no printer-specific calibration bundled
        };
        let (rows, _) = resin_rows(&catalog, Some(&tuned), "", None, "Tuned for your printer");
        assert!(rows[0].header && rows[0].title == "Tuned for your printer");
        assert_eq!(rows[1].badge, "Tuned for your printer");
    }
}
