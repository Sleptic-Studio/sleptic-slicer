// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_print_glue::slice_input_fingerprint;
use sc_project_formats::{
    EmbedProfiles, PlateSlices, ProjectFileType, infer_project_file_type_from_extension,
    read_project, write_project,
};
use sc_scene::node::LeafPayload;
use sc_sl1cer::contour::{ClosedRing, ContourLayer, Polygon};
use sc_sl1cer::geom2d::Point2D;

use sc_print_primitives::{MaterialId, PrinterId};
use sc_sl1cer::profile::LayerSchedule;

use super::*;
use crate::slice_projection::layers_for_choice;

/// A 1 mm cube standing on the plate under this placement.
fn placed_cube(placement: [[f64; 3]; 3], shift: [f64; 3]) -> Transform3D {
    let lifted = AffineTransform3D::from_translation([0.0, 0.0, 0.5]);
    let placed = AffineTransform3D::from_linear_and_translation(placement, shift);
    Transform3D::Affine(placed.compose(&lifted))
}

/// The cube as a mesh file opens: the part itself is the scene's root.
fn opened_mesh(transform: Transform3D) -> Scene {
    let mut scene = Scene::new();
    let mesh = Arc::new(sc_mesh_fixtures::unit_cube_mesh());
    let part = scene
        .add_leaf(None, transform, LeafPayload::Mesh(mesh))
        .unwrap();
    scene.add_root(part).unwrap();
    scene
}

const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn schedule() -> LayerSchedule {
    LayerSchedule::uniform(0.05, 0.05, 20).unwrap()
}

fn token(scene: &Scene, schedule: &LayerSchedule) -> String {
    slice_input_fingerprint(scene, schedule).unwrap().to_token()
}

fn three_mf() -> ProjectFileType {
    infer_project_file_type_from_extension(Path::new("plate.3mf")).unwrap()
}

fn payload(printer: Option<PrinterId>, resin: Option<MaterialId>) -> ProjectPayload {
    ProjectPayload {
        printer,
        resin,
        first_layer_height: None,
        process_overrides: None,
        aa_level: None,
        embedded: Vec::new(),
    }
}

/// Saves the scene with these slices and token, then opens the file again.
fn reopen(
    scene: &Scene,
    payload: &ProjectPayload,
    slices: Option<&PlateSlices>,
    token: &str,
) -> sc_project_formats::LoadedProject {
    let mut file = Vec::new();
    write_project(
        scene,
        payload,
        EmbedProfiles::No,
        slices,
        Some(token),
        three_mf(),
        &mut file,
    )
    .unwrap();
    read_project(&mut Cursor::new(file), three_mf()).unwrap()
}

#[test]
fn the_token_matches_only_its_own_scene_and_schedule() {
    let scene = opened_mesh(placed_cube(IDENTITY, [0.0; 3]));
    let token = token(&scene, &schedule());
    assert!(still_matches(&token, &scene, &schedule()));
    assert!(!still_matches("not-a-token", &scene, &schedule()));
    // Another layer height, or the part moved, and the slices are stale.
    let thicker = LayerSchedule::uniform(0.1, 0.05, 10).unwrap();
    assert!(!still_matches(&token, &scene, &thicker));
    let moved = opened_mesh(placed_cube(IDENTITY, [1.0, 0.0, 0.0]));
    assert!(!still_matches(&token, &moved, &schedule()));
}

#[test]
fn the_token_survives_saving_and_reopening() {
    let scene = opened_mesh(placed_cube(IDENTITY, [0.0; 3]));
    let token = token(&scene, &schedule());
    let opened = reopen(&scene, &payload(None, None), None, &token);
    assert_eq!(opened.slice_fingerprint.as_deref(), Some(token.as_str()));
    assert!(still_matches(&token, &opened.scene, &schedule()));
}

#[test]
fn nothing_is_kept_when_anything_is_missing() {
    let scene = opened_mesh(placed_cube(IDENTITY, [0.0; 3]));
    let none = payload(None, None);
    let stack = || Some(ContourStack::new_unchecked(Vec::new()));
    let token = || Some(token(&scene, &schedule()));
    assert!(check_saved_slices(Some(&none), &scene, None, token()).is_none());
    assert!(check_saved_slices(Some(&none), &scene, stack(), None).is_none());
    assert!(check_saved_slices(None, &scene, stack(), token()).is_none());
    // Without both a printer and a resin there is no schedule to check.
    assert!(check_saved_slices(Some(&none), &scene, stack(), token()).is_none());
    let printer = PrinterId::new("elegoo-mars-5-goo").unwrap();
    let only_printer = payload(Some(printer), None);
    assert!(check_saved_slices(Some(&only_printer), &scene, stack(), token()).is_none());
}

#[test]
fn the_held_slice_takes_the_new_revision_and_aa_off() {
    let printer = PrinterId::new("elegoo-mars-5-goo").unwrap();
    let resin = MaterialId::new("sirayatech-craft-ultra-clear-saturn4").unwrap();
    let scene = opened_mesh(placed_cube(IDENTITY, [0.0; 3]));
    let first = Some(sc_print_primitives::PositiveFiniteMm::new(0.05).unwrap());
    let layers = layers_for_choice(&printer, &resin, first, &scene).unwrap();
    let mut saved_with = payload(Some(printer.clone()), Some(resin.clone()));
    saved_with.first_layer_height = first;
    let saved = check_saved_slices(
        Some(&saved_with),
        &scene,
        Some(ContourStack::new_unchecked(Vec::new())),
        Some(token(&scene, &layers)),
    )
    .expect("the slices fit the reopened scene");
    let geometry = Revision::with_head(Some(3));
    let held = saved.into_slice_result(geometry);
    assert_eq!(held.token, token(&scene, &layers));
    assert_eq!(held.schedule, layers);
    assert_eq!((held.params.printer, held.params.resin), (printer, resin));
    assert_eq!(held.params.first_layer_height, first);
    assert_eq!(held.params.aa_level, AaLevel::Off);
    assert_eq!(held.params.geometry, geometry);
}

#[test]
fn slices_of_a_placed_part_are_kept_when_the_project_reopens() {
    // A mesh file opens with its part as the root, scaled and moved onto
    // the plate; a reopened 3MF puts the same part under a new group. The
    // token must not depend on that nesting, or the slices would be thrown
    // away on every reopen.
    let printer = PrinterId::new("elegoo-mars-5-goo").unwrap();
    let resin = MaterialId::new("sirayatech-craft-ultra-clear-saturn4").unwrap();
    let scale = [[0.62, 0.0, 0.0], [0.0, 0.62, 0.0], [0.0, 0.0, 0.62]];
    let scene = opened_mesh(placed_cube(scale, [-31.4, -12.9, 0.002]));
    let schedule = layers_for_choice(&printer, &resin, None, &scene).unwrap();
    let token = token(&scene, &schedule);
    let square = Polygon::new(
        ClosedRing::new(
            [(-0.3, -0.3), (0.3, -0.3), (0.3, 0.3), (-0.3, 0.3)]
                .map(|(x, y)| Point2D::new(x, y))
                .to_vec(),
        ),
        vec![],
    )
    .unwrap();
    let slab_tops: Vec<f64> = schedule.iter_slab_tops().collect();
    let layers = (0..slab_tops.len())
        .map(|_| ContourLayer::new(0.05, vec![square.clone()]))
        .collect();
    let slices = PlateSlices {
        contours: ContourStack::new_unchecked(layers),
        slab_tops,
    };
    let saved = payload(Some(printer), Some(resin));
    let opened = reopen(&scene, &saved, Some(&slices), &token);
    assert!(opened.slices.is_some());
    assert_eq!(opened.slice_fingerprint.as_deref(), Some(token.as_str()));
    let kept = check_saved_slices(
        opened.payload.as_ref(),
        &opened.scene,
        opened.slices,
        opened.slice_fingerprint,
    );
    assert!(kept.is_some());
}
