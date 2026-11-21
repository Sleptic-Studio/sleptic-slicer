// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;

fn model(
    rows: &[(&str, &str)],
    favourites: &[&str],
    default: Option<&str>,
    current: Option<&str>,
) -> (Vec<String>, i32) {
    let candidates: Vec<(String, String)> =
        rows.iter().map(|(id, label)| (id.to_string(), label.to_string())).collect();
    let favourites = favourites.iter().map(|f| f.to_string()).collect();
    let (default, current) = (default.map(str::to_string), current.map(str::to_string));
    let built = dropdown_rows(&candidates, &favourites, default.as_ref(), current.as_ref());
    (built.rows.into_iter().map(|o| o.caption).collect(), built.selected_row)
}

const ROWS: [(&str, &str); 4] = [("c", "Charlie"), ("a", "alpha"), ("b", "Bravo"), ("d", "Delta")];

#[test]
fn favourites_default_and_current_are_listed_once_sorted_by_label() {
    assert_eq!(model(&ROWS, &["c", "a"], None, None), (vec!["alpha".into(), "Charlie".into()], -1));
    let (labels, _) = model(&ROWS, &["a"], Some("b"), Some("c"));
    assert_eq!(labels, ["alpha", "Bravo", "Charlie"]);
    assert_eq!(model(&ROWS, &["a"], Some("a"), Some("a")), (vec!["alpha".into()], 0));
    // Same label: ordered by id.
    let (labels, _) = model(&[("y", "Same"), ("x", "same")], &["x", "y"], None, None);
    assert_eq!(labels, ["same", "Same"]);
}

#[test]
fn the_scenes_choice_is_selected_before_the_default_and_unknown_ids_drop() {
    assert_eq!(model(&ROWS, &["a"], Some("a"), Some("b")).1, 1);
    assert_eq!(model(&ROWS, &["d"], Some("a"), None).1, 0);
    assert_eq!(model(&ROWS, &["a", "gone"], Some("gone"), None), (vec!["alpha".into()], -1));
    assert_eq!(model(&[], &["a"], Some("a"), Some("a")), (vec![], -1));
}

#[test]
fn labels_join_maker_or_brand_and_name() {
    assert_eq!(printer_caption("Anycubic", "Photon"), "Anycubic Photon");
    assert_eq!(printer_caption("", "Photon"), "Photon");
    assert_eq!(printer_caption("Sleptic", "Sleptic DIY MSLA"), "Sleptic DIY MSLA");
    assert_eq!(printer_caption("sleptic", "Sleptic DIY MSLA"), "Sleptic DIY MSLA");
    assert_eq!(printer_caption("Sleptic", "Sleptic"), "Sleptic");
    assert_eq!(printer_caption("Sleptic", "Slepticity"), "Sleptic Slepticity");
    assert_eq!(printer_caption("Sleptic", "Model One"), "Sleptic Model One");
    assert_eq!(printer_caption(" Sleptic ", " DIY MSLA "), "Sleptic DIY MSLA");
    assert_eq!(printer_caption("Élan", "É"), "Élan É");
    assert_eq!(resin_caption("Elegoo", "Standard"), "Elegoo Standard");
    assert_eq!(resin_caption("", "Standard"), "Standard");
}

#[test]
fn the_first_layer_field_clears_sets_or_ignores() {
    use FirstLayerEdit::*;
    let mm = |v| PositiveFiniteMm::new(v).unwrap();
    for empty in ["", "   "] {
        assert_eq!(read_first_layer_edit(empty), Unset);
    }
    assert_eq!(read_first_layer_edit(" 0.05 "), Height(mm(0.05)));
    for bad in ["0", "-0.1", "abc", "0.1mm", "inf", "NaN"] {
        assert_eq!(read_first_layer_edit(bad), Ignored, "{bad}");
    }
    assert_eq!(first_layer_text(None), "");
    assert_eq!(first_layer_text(Some(mm(0.05))), "0.05");
    assert_eq!(read_first_layer_edit(&first_layer_text(Some(mm(0.05)))), Height(mm(0.05)));
}

fn item(id: &str, brand: &str, group: &str, label: &str) -> ChooserItem<String> {
    ChooserItem { id: id.into(), brand: brand.into(), group: group.into(), label: label.into() }
}

fn shown(lines: &[ChooserLine<String>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| match line {
            ChooserLine::Heading(text) => format!("# {text}"),
            ChooserLine::Choice { label, .. } => label.clone(),
        })
        .collect()
}

#[test]
fn the_chooser_groups_by_brand_and_series_with_numbers_in_order() {
    let items = [
        item("s10", "Elegoo", "Saturn", "Saturn 10"),
        item("generic", "Generic", "", "Generic Machine"),
        item("m4", "Elegoo", "Mars", "Mars 4"),
        item("s3", "Elegoo", "Saturn", "Saturn 3"),
        item("x", "Anycubic", "Photon Mono X", "Photon Mono X 6K"),
        item("m10", "Elegoo", "Mars", "Mars 10"),
    ];
    assert_eq!(chooser_brands(&items), ["Anycubic", "Elegoo", "Generic"], "makers first, generic last");
    let all = chooser_lines(&items, None, |_| true);
    assert_eq!(
        shown(&all),
        [
            "# Anycubic · Photon Mono X", "Photon Mono X 6K",
            "# Elegoo · Mars", "Mars 4", "Mars 10",
            "# Elegoo · Saturn", "Saturn 3", "Saturn 10",
            "# Generic", "Generic Machine",
        ]
    );
    let elegoo = chooser_lines(&items, Some("elegoo"), |_| true);
    assert_eq!(shown(&elegoo), ["# Mars", "Mars 4", "Mars 10", "# Saturn", "Saturn 3", "Saturn 10"]);
    let found = chooser_lines(&items, None, |i| i.label.contains("10"));
    assert_eq!(shown(&found), ["# Elegoo · Mars", "Mars 10", "# Elegoo · Saturn", "Saturn 10"]);
    assert!(chooser_lines(&items, Some("Phrozen"), |_| true).is_empty());
}

#[test]
fn resin_kinds_read_as_headings() {
    assert_eq!(resin_kind_label("abs-like"), "ABS-like");
    assert_eq!(resin_kind_label(" water-washable "), "Water washable");
    assert_eq!(resin_kind_label(""), "Other");
    assert_eq!(resin_kind_label("ceramic"), "ceramic");
}
