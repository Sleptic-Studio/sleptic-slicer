// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;
use crate::build_volume::FALLBACK_VOLUME;

fn screen() -> Option<ViewportSize> {
    ViewportSize::new(1920.0, 1080.0)
}

fn framed() -> ViewHolder {
    let cameras = ViewHolder::new();
    cameras.frame(None, FALLBACK_VOLUME);
    cameras
}

fn camera(cameras: &ViewHolder) -> ViewCamera {
    cameras.view_cell().borrow().clone().unwrap()
}

#[test]
fn a_camera_exists_from_framing_until_reset() {
    let cameras = ViewHolder::new();
    assert!(!cameras.has_view());
    cameras.frame(None, FALLBACK_VOLUME);
    assert!(cameras.has_view());
    cameras.forget_view();
    assert!(!cameras.has_view());
}

#[test]
fn framing_centres_on_the_plate_and_takes_in_a_scene_outside_it() {
    // The plate is centred on the origin; the view centre sits above it.
    let target = camera(&framed()).look_at;
    assert_eq!((target.x, target.y), (0.0, 0.0));
    assert!(target.z > 0.0);
    let cameras = ViewHolder::new();
    let far = Vec3::splat(1000.0);
    cameras.frame(Some((Vec3::ZERO, far)), FALLBACK_VOLUME);
    let target = camera(&cameras).look_at;
    assert!(target.x > 0.0 && target.y > 0.0 && target.z > 0.0);
}

#[test]
fn the_projection_applies_now_and_to_cameras_framed_later() {
    let cameras = framed();
    assert!(camera(&cameras).is_perspective, "perspective until set");
    cameras.set_perspective(false);
    assert!(!camera(&cameras).is_perspective);
    cameras.forget_view();
    cameras.frame(None, FALLBACK_VOLUME);
    assert!(!camera(&cameras).is_perspective);
    // Without a camera the choice is still kept.
    cameras.forget_view();
    cameras.set_perspective(true);
    cameras.frame(None, FALLBACK_VOLUME);
    assert!(camera(&cameras).is_perspective);
}

#[test]
fn drags_need_a_camera_and_a_viewport() {
    let empty = ViewHolder::new();
    let drag = ScreenDelta::new(10.0, 5.0);
    let cursor = ScreenPoint::new(100.0, 100.0);
    assert!(!empty.orbit_by(drag, screen()) && !empty.pan_by(drag, screen()));
    assert!(!empty.zoom_at(120.0, cursor, screen()));
    assert!(!empty.go_home() && !empty.look_along(ViewAxis::Top));
    assert!(!empty.look_from(Vec3::Z));
    let cameras = framed();
    let before = camera(&cameras);
    assert!(!cameras.orbit_by(drag, None) && !cameras.pan_by(drag, None));
    assert!(!cameras.zoom_at(120.0, cursor, None));
    let after = camera(&cameras);
    assert_eq!(
        (after.azimuth, after.look_at, after.eye_distance),
        (before.azimuth, before.look_at, before.eye_distance)
    );
}

#[test]
fn drags_orbit_pan_and_zoom() {
    let cameras = framed();
    let start = camera(&cameras);
    assert!(cameras.orbit_by(ScreenDelta::new(50.0, 0.0), screen()));
    assert_ne!(camera(&cameras).azimuth, start.azimuth);
    let before_pan = camera(&cameras).look_at;
    assert!(cameras.pan_by(ScreenDelta::new(10.0, 5.0), screen()));
    assert_ne!(camera(&cameras).look_at, before_pan);
    // A positive scroll comes closer.
    let before_zoom = camera(&cameras).eye_distance;
    assert!(cameras.zoom_at(120.0, ScreenPoint::new(960.0, 540.0), screen()));
    assert!(camera(&cameras).eye_distance < before_zoom);
}

#[test]
fn a_huge_scroll_still_leaves_a_finite_distance() {
    let center = ScreenPoint::new(960.0, 540.0);
    let cameras = framed();
    let before = camera(&cameras).eye_distance;
    assert!(cameras.zoom_at(1.0e6, center, screen()));
    let closer = camera(&cameras).eye_distance;
    assert!(closer.is_finite() && closer > 0.0 && closer <= before);
    let cameras = framed();
    assert!(cameras.zoom_at(-1.0e6, center, screen()));
    let further = camera(&cameras).eye_distance;
    assert!(further.is_finite() && further >= before);
}

#[test]
fn views_reset_snap_and_look_from_a_direction() {
    let cameras = framed();
    cameras.orbit_by(ScreenDelta::new(120.0, 60.0), screen());
    let turned = camera(&cameras).azimuth;
    assert!(cameras.go_home());
    assert_ne!(camera(&cameras).azimuth, turned);
    assert!(cameras.look_along(ViewAxis::Top));
    assert!(camera(&cameras).elevation > 0.0, "top looks down");
    let side = camera(&cameras).elevation;
    assert!(cameras.look_from(Vec3::new(1.0, 0.0, 0.0)));
    assert_ne!(camera(&cameras).elevation, side);
}
