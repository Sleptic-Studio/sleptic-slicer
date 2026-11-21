// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The Slice panel's printer and resin dropdowns, and its first-layer
//! height field. A dropdown lists the favourites, the global default and the scene's
//! own choice, not the whole catalogue, so whatever is selected can
//! always be shown.
use std::collections::BTreeSet;

use sc_print_primitives::{MaterialId, PositiveFiniteMm, PrinterId};

use crate::profile_catalog::{CatalogResin, CatalogPrinter};

/// A row: the profile id handed back on choice, and its label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DropdownRow<Id> {
    pub id: Id,
    pub caption: String,
}

/// The rows in order and the selected row, -1 for none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DropdownRows<Id> {
    pub rows: Vec<DropdownRow<Id>>,
    pub selected_row: i32,
}

/// The dropdown's rows from `(id, caption)` choices (unique ids): the
/// starred ones, the `fallback` and the `picked` one, by caption ignoring
/// case and then by id. `picked`, else `fallback`, is selected; an id with
/// no choice has no caption, so it is not listed and selects -1.
pub(crate) fn dropdown_rows<Id: Ord + Clone>(
    choices: &[(Id, String)],
    starred: &BTreeSet<Id>,
    fallback: Option<&Id>,
    picked: Option<&Id>,
) -> DropdownRows<Id> {
    let shown = |id: &Id| starred.contains(id) || fallback == Some(id) || picked == Some(id);
    let mut rows = Vec::new();
    for (id, caption) in choices {
        if shown(id) {
            rows.push(DropdownRow { id: id.clone(), caption: caption.clone() });
        }
    }
    rows.sort_by_cached_key(|row| (row.caption.to_lowercase(), row.id.clone()));
    let wanted = picked.or(fallback);
    let selected_row = wanted
        .and_then(|id| rows.iter().position(|row| row.id == *id))
        .and_then(|i| i32::try_from(i).ok())
        .unwrap_or(-1);
    DropdownRows { rows, selected_row }
}

/// "Manufacturer Name", trimmed, without repeating a manufacturer the name
/// already starts with (any case), and just the name without one.
fn printer_caption(manufacturer: &str, name: &str) -> String {
    let (maker, name) = (manufacturer.trim(), name.trim());
    let starts_with_maker = name.get(..maker.len()).is_some_and(|head| head.eq_ignore_ascii_case(maker))
        && name[maker.len()..].chars().next().is_none_or(char::is_whitespace);
    match maker {
        "" => name.to_owned(),
        _ if starts_with_maker => name.to_owned(),
        _ => format!("{maker} {name}"),
    }
}

/// "Brand Name", or the name alone without a brand.
fn resin_caption(brand: &str, name: &str) -> String {
    match brand {
        "" => name.to_owned(),
        brand => format!("{brand} {name}"),
    }
}

/// Every catalog printer as an `(id, caption)` choice.
pub(crate) fn printer_choices(catalog: &[CatalogPrinter]) -> Vec<(PrinterId, String)> {
    catalog.iter().map(|p| (p.id.clone(), printer_caption(&p.printer_maker, &p.name))).collect()
}

/// Every catalog resin as an `(id, caption)` choice.
pub(crate) fn resin_choices(catalog: &[CatalogResin]) -> Vec<(MaterialId, String)> {
    catalog.iter().map(|r| (r.id.clone(), resin_caption(&r.brand, &r.name))).collect()
}

/// A profile as the full chooser sorts it: under its brand, then its group
/// (a printer's series, a resin's kind).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChooserItem<Id> {
    pub id: Id,
    pub brand: String,
    pub group: String,
    /// The name shown under the group's heading, without the brand.
    pub label: String,
}

/// One line of the chooser: a heading, or a profile to pick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ChooserLine<Id> {
    Heading(String),
    Choice { id: Id, label: String },
}

/// Brands that are no maker's (the generic printer, Sleptic's own) come last.
fn brand_rank(brand: &str) -> u8 {
    let brand = brand.trim();
    u8::from(brand.is_empty() || brand.eq_ignore_ascii_case("generic") || brand.eq_ignore_ascii_case("sleptic"))
}

/// A sort key that reads numbers as numbers, so "Mars 4" comes before
/// "Mars 10" and "8K" before "12K".
fn natural(text: &str) -> Vec<(u8, u64, String)> {
    let mut key = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            let mut digits = String::new();
            while let Some(&d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                digits.push(d);
                chars.next();
            }
            key.push((0, digits.parse().unwrap_or(u64::MAX), String::new()));
        } else {
            let mut word = String::new();
            while let Some(&w) = chars.peek().filter(|w| !w.is_ascii_digit()) {
                word.extend(w.to_lowercase());
                chars.next();
            }
            key.push((1, 0, word));
        }
    }
    key
}

/// The brands to filter by, in the chooser's order.
pub(crate) fn chooser_brands<Id>(items: &[ChooserItem<Id>]) -> Vec<String> {
    let mut brands: Vec<String> = Vec::new();
    for item in items {
        if !brands.iter().any(|b| b.eq_ignore_ascii_case(&item.brand)) {
            brands.push(item.brand.clone());
        }
    }
    brands.sort_by_cached_key(|b| (brand_rank(b), b.to_lowercase()));
    brands
}

/// The chooser's lines: the items of `brand` (all brands for none) that
/// `shown` keeps, by brand, group and name, with a heading wherever the
/// group changes. Across all brands a heading names the brand too.
pub(crate) fn chooser_lines<Id: Clone>(
    items: &[ChooserItem<Id>],
    brand: Option<&str>,
    shown: impl Fn(&ChooserItem<Id>) -> bool,
) -> Vec<ChooserLine<Id>> {
    let mut kept: Vec<&ChooserItem<Id>> = items
        .iter()
        .filter(|item| brand.is_none_or(|b| item.brand.eq_ignore_ascii_case(b)) && shown(item))
        .collect();
    kept.sort_by_cached_key(|item| {
        (brand_rank(&item.brand), item.brand.to_lowercase(), natural(&item.group), natural(&item.label))
    });
    let mut lines = Vec::new();
    let mut last: Option<(String, String)> = None;
    for item in kept {
        let section = (item.brand.to_lowercase(), item.group.to_lowercase());
        if last.as_ref() != Some(&section) {
            let heading = match (brand, item.group.trim()) {
                (Some(_), "") => item.brand.clone(),
                (Some(_), group) => group.to_owned(),
                (None, "") => item.brand.clone(),
                (None, group) if item.brand.trim().is_empty() => group.to_owned(),
                (None, group) => format!("{} \u{b7} {group}", item.brand),
            };
            lines.push(ChooserLine::Heading(heading));
            last = Some(section);
        }
        lines.push(ChooserLine::Choice { id: item.id.clone(), label: item.label.clone() });
    }
    lines
}

/// Every catalog printer as a chooser item: its maker, its series and its
/// name without the maker.
pub(crate) fn printer_items(catalog: &[CatalogPrinter]) -> Vec<ChooserItem<PrinterId>> {
    catalog
        .iter()
        .map(|p| ChooserItem {
            id: p.id.clone(),
            brand: p.printer_maker.trim().to_owned(),
            group: p.series.trim().to_owned(),
            label: crate::mesh_render::plate_model_for(&p.printer_maker, &p.name),
        })
        .collect()
}

/// A resin profile's kind (`family`) as a heading.
pub(crate) fn resin_kind_label(family: &str) -> String {
    use crate::i18n::tr;
    match family.trim().to_ascii_lowercase().as_str() {
        "standard" => tr!("Standard").to_string(),
        "abs-like" => tr!("ABS-like").to_string(),
        "tough" => tr!("Tough").to_string(),
        "flexible" => tr!("Flexible").to_string(),
        "water-washable" => tr!("Water washable").to_string(),
        "plant-based" => tr!("Plant-based").to_string(),
        "engineering" => tr!("Engineering").to_string(),
        "clear" => tr!("Clear").to_string(),
        "" | "other" => tr!("Other").to_string(),
        other => other.to_owned(),
    }
}

/// Every catalog resin as a chooser item: its brand, its kind and its name.
pub(crate) fn resin_items(catalog: &[CatalogResin]) -> Vec<ChooserItem<MaterialId>> {
    catalog
        .iter()
        .map(|r| ChooserItem {
            id: r.id.clone(),
            brand: r.brand.trim().to_owned(),
            group: resin_kind_label(&r.resin_family),
            label: r.name.trim().to_owned(),
        })
        .collect()
}

/// A first-layer height field edit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum FirstLayerEdit {
    /// Empty: remove this print's own first-layer height.
    Unset,
    Height(PositiveFiniteMm),
    /// Not a positive number: the edit is ignored.
    Ignored,
}

/// What a first-layer field edit means.
pub(crate) fn read_first_layer_edit(typed: &str) -> FirstLayerEdit {
    match typed.trim() {
        "" => FirstLayerEdit::Unset,
        number => match number.parse().map(PositiveFiniteMm::new) {
            Ok(Ok(mm)) => FirstLayerEdit::Height(mm),
            _ => FirstLayerEdit::Ignored,
        },
    }
}

/// The field's text: the number of millimetres, or empty when unset.
pub(crate) fn first_layer_text(override_mm: Option<PositiveFiniteMm>) -> String {
    override_mm.map_or_else(String::new, |mm| mm.value().to_string())
}

#[cfg(test)]
#[path = "tests_slice_selection.rs"]
mod tests;
