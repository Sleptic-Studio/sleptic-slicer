// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! What the 3D view does with keys and the pointer, through the real
//! window: each key or gesture sent through the Slint testing backend and
//! the callback it reaches, with its arguments. No controller is attached,
//! so only the view's own handling is under test.
use std::cell::RefCell;
use std::rc::Rc;

use i_slint_backend_testing::{ElementHandle, ElementQuery};
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, SharedString};

use crate::{AxisView, MainWindow};

type Log = Rc<RefCell<Vec<String>>>;

/// The modifiers a pointer callback carried, as the log prints them.
fn held(keys: &crate::PointerModifiers) -> String {
    format!("shift={} control={} alt={} meta={}", keys.shift, keys.control, keys.alt, keys.meta)
}

struct View {
    ui: MainWindow,
    log: Log,
    viewer: ElementHandle,
}

impl View {
    /// A window with a scene, the 3D view focused by a click, and every
    /// callback the view's keys and pointer reach written to the log.
    fn new() -> Self {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
        ui.set_scene_present(true);
        let log: Log = Rc::new(RefCell::new(Vec::new()));
        let writer = |log: &Log| {
            let log = Rc::clone(log);
            move |entry: String| log.borrow_mut().push(entry)
        };
        let w = writer(&log);
        ui.on_orbit_dragged(move |x, y| w(format!("rotate {x} {y}")));
        let w = writer(&log);
        ui.on_pan_dragged(move |x, y| w(format!("pan {x} {y}")));
        let w = writer(&log);
        ui.on_wheel_zoom(move |d, x, y| w(format!("zoom {d} {x} {y}")));
        let w = writer(&log);
        ui.on_fit_view_asked(move || w("frame".into()));
        let w = writer(&log);
        ui.on_projection_toggled(move |perspective| w(format!("projection {perspective}")));
        let w = writer(&log);
        ui.on_show_shortcuts_requested(move || w("help".into()));
        let w = writer(&log);
        ui.on_axis_view_asked(move |view: AxisView| w(format!("view {view:?}")));
        let w = writer(&log);
        ui.on_selection_dropped(move || w("clear".into()));
        let w = writer(&log);
        ui.on_move_tool_key(move || w("focus move".into()));
        let w = writer(&log);
        ui.on_rotate_tool_key(move || w("focus rotate".into()));
        let w = writer(&log);
        ui.on_scale_tool_key(move || w("focus scale".into()));
        let w = writer(&log);
        ui.on_arrow_key_down(move |code| w(format!("arrow down {code}")));
        let w = writer(&log);
        ui.on_arrow_key_up(move |code| w(format!("arrow up {code}")));
        let w = writer(&log);
        ui.on_step_selection(move |step| w(format!("cycle {step}")));
        let w = writer(&log);
        ui.on_copy_key(move || w("copy".into()));
        let w = writer(&log);
        ui.on_paste_key(move || w("paste".into()));
        let w = writer(&log);
        ui.on_delete_key(move || w("delete".into()));
        let w = writer(&log);
        ui.on_context_action(move |action| w(format!("action {action}")));
        let w = writer(&log);
        ui.on_viewport_pointer_down(move |x, y, keys| {
            w(format!("press {x} {y} {}", held(&keys)));
            false
        });
        let w = writer(&log);
        ui.on_pointer_released(move |x, y, keys| w(format!("release {x} {y} {}", held(&keys))));
        let w = writer(&log);
        ui.on_secondary_pressed(move |x, y| w(format!("right press {x} {y}")));
        let w = writer(&log);
        ui.on_secondary_released(move |x, y| w(format!("right release {x} {y}")));
        ui.show().unwrap();
        let viewer = ElementQuery::from_root(&ui)
            .match_descendants()
            .match_inherits("MeshViewer")
            .find_first()
            .expect("the 3D view");
        // A click gives the view keyboard focus.
        viewer.mock_single_click(PointerEventButton::Left);
        log.borrow_mut().clear();
        Self { ui, log, viewer }
    }

    fn send(&self, event: WindowEvent) {
        self.ui.window().dispatch_event(event);
    }

    fn key_down(&self, text: impl Into<SharedString>) {
        self.send(WindowEvent::KeyPressed { text: text.into() });
    }

    fn key_up(&self, text: impl Into<SharedString>) {
        self.send(WindowEvent::KeyReleased { text: text.into() });
    }

    /// Presses and lets go of `key`, with `held` held around it.
    fn tap(&self, held: Option<Key>, key: impl Into<SharedString>) {
        let key = key.into();
        if let Some(modifier) = held {
            self.key_down(modifier);
        }
        self.key_down(key.clone());
        self.key_up(key);
        if let Some(modifier) = held {
            self.key_up(modifier);
        }
    }

    /// What `act` logged.
    fn after(&self, act: impl FnOnce(&Self)) -> Vec<String> {
        self.log.borrow_mut().clear();
        act(self);
        std::mem::take(&mut *self.log.borrow_mut())
    }

    /// The view's centre, where key zooms aim, as the log prints it.
    fn centre(&self) -> String {
        let size = self.viewer.size();
        format!("{} {}", size.width / 2.0, size.height / 2.0)
    }
}

#[test]
fn arrows_turn_the_camera_and_pan_it_with_shift() {
    let view = View::new();
    let cases = [
        (Key::LeftArrow, "rotate -20 0", "pan -30 0"),
        (Key::RightArrow, "rotate 20 0", "pan 30 0"),
        (Key::UpArrow, "rotate 0 -20", "pan 0 -30"),
        (Key::DownArrow, "rotate 0 20", "pan 0 30"),
    ];
    for (key, turned, panned) in cases {
        assert_eq!(view.after(|v| v.tap(None, key)), [turned], "{key:?}");
        assert_eq!(view.after(|v| v.tap(Some(Key::Shift), key)), [panned], "Shift {key:?}");
    }
}

#[test]
fn zoom_keys_zoom_at_the_middle_of_the_view() {
    let view = View::new();
    let centre = view.centre();
    let zoom_in = format!("zoom 120 {centre}");
    let zoom_out = format!("zoom -120 {centre}");
    assert_eq!(view.after(|v| v.tap(None, Key::PageUp)), [zoom_in.clone()]);
    assert_eq!(view.after(|v| v.tap(None, Key::PageDown)), [zoom_out.clone()]);
    for key in ["+", "="] {
        assert_eq!(view.after(|v| v.tap(None, key)), [zoom_in.clone()], "{key}");
    }
    assert_eq!(view.after(|v| v.tap(None, "-")), [zoom_out]);
}

#[test]
fn with_the_keyboard_gizmo_on_the_arrows_drive_it_down_and_up() {
    let view = View::new();
    // Without the gizmo, letting go of an arrow is nobody's business.
    view.after(|v| v.key_down(Key::RightArrow));
    assert_eq!(view.after(|v| v.key_up(Key::RightArrow)), Vec::<String>::new());
    view.ui.set_keys_drive_gizmo(true);
    let codes = [
        (Key::RightArrow, 0),
        (Key::LeftArrow, 1),
        (Key::UpArrow, 2),
        (Key::DownArrow, 3),
        (Key::PageUp, 4),
        (Key::PageDown, 5),
    ];
    for (key, code) in codes {
        let expected = [format!("arrow down {code}"), format!("arrow up {code}")];
        assert_eq!(view.after(|v| v.tap(None, key)), expected, "{key:?}");
        // Shift does not turn a gizmo arrow into a camera pan.
        assert_eq!(view.after(|v| v.tap(Some(Key::Shift), key)), expected, "Shift {key:?}");
    }
    // The zoom keys that are not axis keys still zoom.
    let zoom_in = format!("zoom 120 {}", view.centre());
    assert_eq!(view.after(|v| v.tap(None, "+")), [zoom_in]);
}

#[test]
fn view_keys_frame_pick_a_side_or_switch_projection() {
    let view = View::new();
    for key in [SharedString::from(Key::Home), "f".into(), "F".into(), "0".into()] {
        assert_eq!(view.after(|v| v.tap(None, key.clone())), ["frame"], "{key}");
    }
    let sides = [("1", "Front", "Back"), ("3", "Right", "Left"), ("7", "Top", "Bottom")];
    for (key, plain, flipped) in sides {
        assert_eq!(view.after(|v| v.tap(None, key)), [format!("view {plain}")]);
        let with_control = view.after(|v| v.tap(Some(Key::Control), key));
        assert_eq!(with_control, [format!("view {flipped}")], "Control {key}");
    }
    // 5 switches perspective and orthographic, each time.
    let first = view.after(|v| v.tap(None, "5"));
    let second = view.after(|v| v.tap(None, "5"));
    assert_eq!(first.len(), 1, "{first:?}");
    assert_ne!(first, second, "it switched back");
    assert_eq!(view.after(|v| v.tap(None, "?")), ["help"]);
}

#[test]
fn letters_pick_the_gizmo_tool_and_escape_clears() {
    let view = View::new();
    for (key, expected) in [("t", "focus move"), ("r", "focus rotate"), ("s", "focus scale")] {
        assert_eq!(view.after(|v| v.tap(None, key)), [expected]);
        assert_eq!(view.after(|v| v.tap(None, key.to_uppercase())), [expected]);
    }
    assert_eq!(view.after(|v| v.tap(None, Key::Escape)), ["clear"]);
}

#[test]
fn edit_keys_copy_paste_delete_arrange_and_select_all() {
    let view = View::new();
    for key in ["c", "C"] {
        assert_eq!(view.after(|v| v.tap(Some(Key::Control), key)), ["copy"]);
        assert_eq!(view.after(|v| v.tap(Some(Key::Meta), key)), ["copy"], "Command {key}");
    }
    for key in ["v", "V"] {
        assert_eq!(view.after(|v| v.tap(Some(Key::Control), key)), ["paste"]);
    }
    // Plain C and V are kept free.
    assert_eq!(view.after(|v| v.tap(None, "c")), Vec::<String>::new());
    assert_eq!(view.after(|v| v.tap(None, "v")), Vec::<String>::new());
    for key in [Key::Delete, Key::Backspace] {
        assert_eq!(view.after(|v| v.tap(None, key)), ["delete"], "{key:?}");
    }
    assert_eq!(view.after(|v| v.tap(None, "a")), ["action 2"], "A arranges");
    assert_eq!(view.after(|v| v.tap(Some(Key::Control), "a")), ["action 1"], "Control A selects all");
}

#[test]
fn tab_cycles_the_selection_only_with_a_scene() {
    let view = View::new();
    assert_eq!(view.after(|v| v.tap(None, Key::Tab)), ["cycle 1"]);
    assert_eq!(view.after(|v| v.tap(Some(Key::Shift), Key::Tab)), ["cycle -1"]);
    assert_eq!(view.after(|v| v.tap(None, Key::Backtab)), ["cycle -1"]);
    view.ui.set_scene_present(false);
    assert_eq!(view.after(|v| v.tap(None, Key::Tab)), Vec::<String>::new());
}

#[test]
fn a_left_click_reports_press_and_release_with_the_modifiers() {
    let view = View::new();
    let clicked = view.after(|v| v.viewer.mock_single_click(PointerEventButton::Left));
    // Press, a zero turn (the press started an orbit, as it grabbed no
    // gizmo handle), release: at one pixel, no modifiers.
    assert_eq!(clicked.len(), 3, "{clicked:?}");
    let none = "shift=false control=false alt=false meta=false";
    assert!(clicked[0].starts_with("press ") && clicked[0].ends_with(none), "{clicked:?}");
    assert_eq!(clicked[1], "rotate 0 0");
    assert!(clicked[2].starts_with("release ") && clicked[2].ends_with(none), "{clicked:?}");
    let at = |entry: &str| entry.split(' ').skip(1).take(2).collect::<Vec<_>>().join(" ");
    assert_eq!(at(&clicked[0]), at(&clicked[2]));
}

#[test]
fn a_right_drag_pans_and_a_left_drag_off_the_gizmo_turns() {
    let view = View::new();
    let start = LogicalPosition::new(400.0, 300.0);
    let end = LogicalPosition::new(440.0, 330.0);
    let step = |v: &View, button: PointerEventButton| {
        v.send(WindowEvent::PointerMoved { position: start });
        v.send(WindowEvent::PointerPressed { position: start, button });
        v.send(WindowEvent::PointerMoved { position: end });
        v.send(WindowEvent::PointerReleased { position: end, button });
    };
    let right = view.after(|v| step(v, PointerEventButton::Right));
    assert!(right.contains(&"pan 40 30".to_string()), "{right:?}");
    assert!(right.first().is_some_and(|e| e.starts_with("right press")), "{right:?}");
    assert!(right.last().is_some_and(|e| e.starts_with("right release")), "{right:?}");
    // The press did not grab a gizmo handle (the log answers false), so
    // dragging with the left button turns the camera.
    let left = view.after(|v| step(v, PointerEventButton::Left));
    assert!(left.contains(&"rotate 40 30".to_string()), "{left:?}");
}

#[test]
fn a_click_carries_each_modifier_held_with_it() {
    let view = View::new();
    let cases = [
        (Key::Shift, "shift=true control=false alt=false meta=false"),
        (Key::Control, "shift=false control=true alt=false meta=false"),
        (Key::Alt, "shift=false control=false alt=true meta=false"),
        (Key::Meta, "shift=false control=false alt=false meta=true"),
    ];
    for (key, expected) in cases {
        let clicked = view.after(|v| {
            v.key_down(key);
            v.viewer.mock_single_click(PointerEventButton::Left);
            v.key_up(key);
        });
        let ends = |entry: &String| entry.ends_with(expected);
        let reported: Vec<&String> =
            clicked.iter().filter(|e| e.starts_with("press") || e.starts_with("release")).collect();
        assert_eq!(reported.len(), 2, "{clicked:?}");
        assert!(reported.into_iter().all(ends), "{key:?}: {clicked:?}");
    }
}
