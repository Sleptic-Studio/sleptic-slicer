// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
#[allow(unused_imports)]
use crate::panel_place::Panel;
use crate::build_volume::FALLBACK_VOLUME;

use std::cell::Cell;

use super::*;

fn initial() -> UserPreferences {
    UserPreferences {
        orient_preset: 0,
        perspective_view: false,
        setup_complete: true,
        theme: ThemePreference::System,
        menu_layout: MenuLayout::Bar,
        language: LanguagePreference::System,
        currency_symbol: "$".into(),
        default_printer: None,
        default_resin: None,
        favourite_printers: BTreeSet::new(),
        favourite_resins: BTreeSet::new(),
        aa_level: AaLevel::Off,
        build_volume: FALLBACK_VOLUME,
        object_list: Panel::ObjectList.start(),
        slice_panel: Panel::SliceControls.start(),
        slice_view: Panel::SliceView.start(),
        last_open_dir: None,
        recent_files: Vec::new(),
    }
}

fn set_theme(bus: &PreferenceHub, theme: ThemePreference) {
    bus.edit_preferences(move |snapshot| {
        snapshot.theme = theme;
        PreferenceEdit::Theme
    });
}

/// Records the changes a listener is told of, with a value read from the
/// snapshot it is given.
fn recorder<T: 'static>(
    bus: &Rc<PreferenceHub>,
    read: impl Fn(&UserPreferences) -> T + 'static,
) -> (Rc<RefCell<Vec<(T, PreferenceEdit)>>>, PreferenceWatch) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    let subscription = bus.watch_preferences(move |snapshot, change| sink.borrow_mut().push((read(snapshot), *change)));
    (seen, subscription)
}

#[test]
fn listeners_are_told_the_new_preferences_and_what_changed() {
    let bus = PreferenceHub::new(initial());
    assert_eq!(bus.current_values().language, LanguagePreference::System);
    let (seen, _sub) = recorder(&bus, |s| s.theme);
    set_theme(&bus, ThemePreference::Dark);
    assert_eq!(*seen.borrow(), [(ThemePreference::Dark, PreferenceEdit::Theme)]);
    assert_eq!(bus.current_values().theme, ThemePreference::Dark);
}

#[test]
fn every_kind_of_preference_reaches_listeners() {
    let bus = PreferenceHub::new(initial());
    let (seen, _sub) = recorder(&bus, |s| format!("{s:?}"));
    let printer = PrinterId::new("anycubic-photon-m3-pm3").unwrap();
    let volume = BuildVolume::new(150.0, 80.0, 160.0);
    let changes: Vec<Box<dyn Fn(&mut UserPreferences) -> PreferenceEdit>> = vec![
        Box::new(|s| {
            s.menu_layout = MenuLayout::Compact;
            PreferenceEdit::MenuLayout
        }),
        Box::new(|s| {
            s.object_list.visible = false;
            (s.object_list.anchor_right, s.object_list.anchor_bottom) = (false, true);
            (s.object_list.offset_x, s.object_list.offset_y) = (Some(30.0), Some(40.0));
            (s.object_list.width, s.object_list.height, s.object_list.minimized) = (Some(300.0), Some(200.0), true);
            PreferenceEdit::ObjectListPanel
        }),
        Box::new(|s| {
            s.slice_view.visible = false;
            (s.slice_view.offset_x, s.slice_view.width, s.slice_view.minimized) = (Some(5.0), Some(260.0), true);
            PreferenceEdit::SliceViewPanel
        }),
        Box::new(|s| {
            s.last_open_dir = Some(PathBuf::from("/models"));
            PreferenceEdit::LastOpenDir
        }),
        Box::new(|s| {
            s.recent_files = vec![PathBuf::from("/models/b.stl"), PathBuf::from("/models/a.stl")];
            PreferenceEdit::RecentFiles
        }),
        Box::new(move |s| {
            s.default_printer = Some(printer.clone());
            s.build_volume = volume;
            PreferenceEdit::DefaultPrinter
        }),
    ];
    let mut expected = initial();
    for change in changes {
        let what = change(&mut expected);
        bus.edit_preferences(move |s| change(s));
        assert_eq!(seen.borrow().last(), Some(&(format!("{expected:?}"), what)));
    }
    assert_eq!(format!("{:?}", bus.current_values()), format!("{expected:?}"));
}

#[test]
fn listeners_are_told_once_each_in_the_order_they_subscribed() {
    let bus = PreferenceHub::new(initial());
    let order = Rc::new(RefCell::new(Vec::new()));
    let subs: Vec<PreferenceWatch> = (0..3)
        .map(|n| {
            let order = Rc::clone(&order);
            bus.watch_preferences(move |_, _| order.borrow_mut().push(n))
        })
        .collect();
    set_theme(&bus, ThemePreference::Dark);
    set_theme(&bus, ThemePreference::Light);
    assert_eq!(*order.borrow(), [0, 1, 2, 0, 1, 2]);
    drop(subs);
}

#[test]
fn dropping_a_subscription_stops_its_listener_even_after_the_bus() {
    let bus = PreferenceHub::new(initial());
    let told = Rc::new(Cell::new(0));
    let count = Rc::clone(&told);
    let subscription = bus.watch_preferences(move |_, _| count.set(count.get() + 1));
    set_theme(&bus, ThemePreference::Dark);
    drop(subscription);
    set_theme(&bus, ThemePreference::Light);
    assert_eq!(told.get(), 1);
    let late = bus.watch_preferences(|_, _| {});
    drop(bus);
    drop(late);
}

#[test]
fn a_listener_added_while_telling_hears_only_later_updates() {
    let bus = PreferenceHub::new(initial());
    let (told, added) = (Rc::new(Cell::new(0)), Rc::new(RefCell::new(None::<PreferenceWatch>)));
    let (count, slot, inner_bus) = (Rc::clone(&told), Rc::clone(&added), Rc::clone(&bus));
    let _outer = bus.watch_preferences(move |_, _| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let count = Rc::clone(&count);
            *slot = Some(inner_bus.watch_preferences(move |_, _| count.set(count.get() + 1)));
        }
    });
    set_theme(&bus, ThemePreference::Dark);
    assert_eq!(told.get(), 0);
    set_theme(&bus, ThemePreference::Light);
    assert_eq!(told.get(), 1);
}

#[test]
fn updates_made_while_updating_wait_their_turn() {
    // From inside the change itself.
    let bus = PreferenceHub::new(initial());
    let inner = Rc::clone(&bus);
    bus.edit_preferences(move |s| {
        s.theme = ThemePreference::Dark;
        set_theme(&inner, ThemePreference::Light);
        PreferenceEdit::Theme
    });
    assert_eq!(bus.current_values().theme, ThemePreference::Light);
    // From a listener: a later listener still hears Dark before Light.
    let bus = PreferenceHub::new(initial());
    let (again, once) = (Rc::clone(&bus), Cell::new(false));
    let _first = bus.watch_preferences(move |s, _| {
        if s.theme == ThemePreference::Dark && !once.replace(true) {
            set_theme(&again, ThemePreference::Light);
        }
    });
    let (seen, _second) = recorder(&bus, |s| s.theme);
    set_theme(&bus, ThemePreference::Dark);
    let themes: Vec<_> = seen.borrow().iter().map(|(theme, _)| *theme).collect();
    assert_eq!(themes, [ThemePreference::Dark, ThemePreference::Light]);
}

#[test]
fn a_panicking_listener_does_not_leave_updates_queued() {
    let bus = PreferenceHub::new(initial());
    let panicking = bus.watch_preferences(|_, _| panic!("listener failed"));
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| set_theme(&bus, ThemePreference::Dark)));
    assert!(failed.is_err());
    drop(panicking);
    // The next update is applied at once, not queued behind the failure.
    set_theme(&bus, ThemePreference::Light);
    assert_eq!(bus.current_values().theme, ThemePreference::Light);
}
