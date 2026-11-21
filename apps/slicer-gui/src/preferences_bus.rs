// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The app's preferences and who is told when they change.
//!
//! Everything runs on the Slint event loop, so this is `Rc`/`RefCell`, not
//! `Arc`/`Mutex`; another thread changes a preference by scheduling an
//! `update` on the UI thread (`slint::invoke_from_event_loop`).
//!
//! Each listener gets the whole new snapshot and what changed, and can
//! ignore the changes it does not care about. Listeners are told in the
//! order they subscribed: the app's own (saving to disk, applying the
//! language) subscribe first, so a window's listeners see their effects.
//! A [`PreferenceWatch`] unsubscribes when dropped, so a window's listener
//! goes with the window.
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use sc_print_primitives::{AaLevel, MaterialId, PrinterId};

use crate::build_volume::BuildVolume;
use crate::language::LanguagePreference;
use crate::panel_place::{Panel, PanelPlace};
use crate::{MenuLayout, ThemePreference};

/// Every preference, as listeners see it.
///
/// - `perspective_view`: perspective, else orthographic (the default).
/// - `setup_complete`: the first-run setup is done or skipped.
/// - `currency_symbol`: shown before a resin cost; a label, not a currency
///   code (`print_estimate::format_cost`).
/// - `favourite_printers` / `favourite_resins`: app-wide; the Slice panel's
///   pickers show them with the chosen profiles.
/// - `build_volume`: not stored; follows `default_printer` (or the built-in
///   default), carried here for the viewports.
/// - `object_list`, `slice_panel`, `slice_view`: where the floating panels
///   are. Saved for the next start; a change in one window is not applied to
///   other open windows (`window_prefs`).
/// - `last_open_dir`: where the open dialog starts (`None`: home folder).
/// - `recent_files`: newest first, capped and without repeats
///   (`AppSettings::push_recent_file`).
#[derive(Clone, Debug)]
pub(crate) struct UserPreferences {
    pub orient_preset: i32,
    pub perspective_view: bool,
    pub setup_complete: bool,
    pub theme: ThemePreference,
    pub menu_layout: MenuLayout,
    pub language: LanguagePreference,
    pub currency_symbol: String,
    pub default_printer: Option<PrinterId>,
    pub default_resin: Option<MaterialId>,
    pub favourite_printers: BTreeSet<PrinterId>,
    pub favourite_resins: BTreeSet<MaterialId>,
    pub aa_level: AaLevel,
    pub build_volume: BuildVolume,
    pub object_list: PanelPlace,
    pub slice_panel: PanelPlace,
    pub slice_view: PanelPlace,
    pub last_open_dir: Option<PathBuf>,
    pub recent_files: Vec<PathBuf>,
}

impl UserPreferences {
    pub(crate) fn panel(&self, panel: Panel) -> PanelPlace {
        match panel {
            Panel::ObjectList => self.object_list,
            Panel::SliceControls => self.slice_panel,
            Panel::SliceView => self.slice_view,
        }
    }

    pub(crate) fn panel_mut(&mut self, panel: Panel) -> &mut PanelPlace {
        match panel {
            Panel::ObjectList => &mut self.object_list,
            Panel::SliceControls => &mut self.slice_panel,
            Panel::SliceView => &mut self.slice_view,
        }
    }
}

/// What an update changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreferenceEdit {
    Theme,
    MenuLayout,
    Language,
    CurrencySymbol,
    OrientPreset,
    Projection,
    DefaultPrinter,
    DefaultResin,
    Favourites,
    AaLevel,
    ObjectListPanel,
    SliceControlPanel,
    SliceViewPanel,
    LastOpenDir,
    RecentFiles,
    /// The first-run setup finished or was skipped.
    Setup,
    /// A profile was saved outside Preferences (a calibration result): the
    /// printer and resin lists reload.
    Profiles,
}

type Listener = Rc<dyn Fn(&UserPreferences, &PreferenceEdit)>;
type Edit = Box<dyn FnOnce(&mut UserPreferences) -> PreferenceEdit>;

pub(crate) struct PreferenceHub {
    values: RefCell<UserPreferences>,
    /// Keyed by subscription number, so iteration is subscription order.
    listeners: RefCell<BTreeMap<u64, Listener>>,
    numbers: std::cell::Cell<u64>,
    /// `Some` while an update is being handed out; updates asked for
    /// meanwhile wait in it, in order.
    backlog: RefCell<Option<VecDeque<Edit>>>,
}

/// Ends a delivery run however it ends (a listener may panic): pending
/// updates of a failed run are dropped instead of blocking later ones.
struct DeliveryRun<'a>(&'a RefCell<Option<VecDeque<Edit>>>);

impl Drop for DeliveryRun<'_> {
    fn drop(&mut self) {
        self.0.replace(None);
    }
}

impl PreferenceHub {
    pub(crate) fn new(initial: UserPreferences) -> Rc<Self> {
        Rc::new(PreferenceHub {
            values: RefCell::new(initial),
            listeners: RefCell::default(),
            numbers: std::cell::Cell::new(0),
            backlog: RefCell::new(None),
        })
    }

    /// The preferences now (windows read them when they open).
    pub(crate) fn current_values(&self) -> UserPreferences {
        self.values.borrow().clone()
    }

    /// Changes the preferences with `change`, which says what it changed,
    /// and hands the result to every listener. An update asked for while
    /// one is being handed out (from `change` or from a listener) waits its
    /// turn, so every listener sees updates in the order they were made.
    pub(crate) fn edit_preferences<F>(&self, change: F)
    where
        F: FnOnce(&mut UserPreferences) -> PreferenceEdit + 'static,
    {
        if let Some(waiting) = self.backlog.borrow_mut().as_mut() {
            waiting.push_back(Box::new(change));
            return;
        }
        self.backlog.replace(Some(VecDeque::new()));
        let _run = DeliveryRun(&self.backlog);
        let mut next: Option<Edit> = Some(Box::new(change));
        while let Some(edit) = next.take() {
            self.deliver(edit);
            next = self.backlog.borrow_mut().as_mut().and_then(VecDeque::pop_front);
        }
    }

    /// Applies one edit, then calls the listeners subscribed at that moment
    /// with nothing borrowed, so they may read, subscribe or edit.
    fn deliver(&self, edit: Edit) {
        let what = edit(&mut self.values.borrow_mut());
        let snapshot = self.current_values();
        let audience: Vec<Listener> = self.listeners.borrow().values().cloned().collect();
        for listener in audience {
            listener(&snapshot, &what);
        }
    }

    /// Adds a listener, told of every update until the returned watch is
    /// dropped.
    pub(crate) fn watch_preferences<F>(self: &Rc<Self>, listener: F) -> PreferenceWatch
    where
        F: Fn(&UserPreferences, &PreferenceEdit) + 'static,
    {
        let number = self.numbers.get();
        self.numbers.set(number.wrapping_add(1));
        self.listeners.borrow_mut().insert(number, Rc::new(listener));
        PreferenceWatch { number, hub: Rc::downgrade(self) }
    }
}

/// A listener's place on the hub; dropping it unsubscribes (the hub may be
/// gone already).
pub(crate) struct PreferenceWatch {
    number: u64,
    hub: Weak<PreferenceHub>,
}

impl Drop for PreferenceWatch {
    fn drop(&mut self) {
        if let Some(hub) = self.hub.upgrade() {
            hub.listeners.borrow_mut().remove(&self.number);
        }
    }
}

#[cfg(test)]
#[path = "tests_preferences_bus.rs"]
mod tests;
