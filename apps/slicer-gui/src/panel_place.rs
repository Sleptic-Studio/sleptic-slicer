// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Where each of the main window's floating panels sits: the object list,
//! the slice controls and the slice view. All three are placed the same
//! way, so one type describes them, and each panel only differs in where it
//! starts out.
use serde::{Deserialize, Serialize};

/// One floating panel's place, in logical pixels.
///
/// The panel keeps to a window corner (`anchor_right`, `anchor_bottom`) at
/// a distance from it (`offset_x`, `offset_y`), with a size; `None` means
/// the layout's own default. The slice controls are always shown and never
/// minimised, so their `visible` and `minimized` stay at the start values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelPlace {
    pub visible: bool,
    pub anchor_right: bool,
    pub anchor_bottom: bool,
    pub offset_x: Option<f32>,
    pub offset_y: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub minimized: bool,
}

/// The three panels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Panel {
    ObjectList,
    SliceControls,
    SliceView,
}

impl Panel {
    /// Where the panel is on a first start: the object list top right, the
    /// slice view bottom left, the slice controls top left.
    pub(crate) fn start(self) -> PanelPlace {
        let corner = |right, bottom| PanelPlace {
            visible: true,
            anchor_right: right,
            anchor_bottom: bottom,
            offset_x: None,
            offset_y: None,
            width: None,
            height: None,
            minimized: false,
        };
        match self {
            Panel::ObjectList => corner(true, false),
            Panel::SliceControls => corner(false, false),
            Panel::SliceView => corner(false, true),
        }
    }
}

/// A panel's place as `settings.toml` keeps it: only what differs from the
/// panel's start place is written, so an untouched panel leaves no section
/// and a new default reaches everyone who never moved it. Every key is
/// optional, so files from any earlier version read.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct SavedPanel {
    #[serde(skip_serializing_if = "Option::is_none")]
    visible: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    anchor_right: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    anchor_bottom: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    offset_x: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    offset_y: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    minimized: Option<bool>,
}

impl SavedPanel {
    /// What has to be written for `place`, given where `panel` starts.
    pub(crate) fn record(panel: Panel, place: PanelPlace) -> SavedPanel {
        let start = panel.start();
        let flag = |now: bool, then: bool| (now != then).then_some(now);
        SavedPanel {
            visible: flag(place.visible, start.visible),
            anchor_right: flag(place.anchor_right, start.anchor_right),
            anchor_bottom: flag(place.anchor_bottom, start.anchor_bottom),
            offset_x: place.offset_x,
            offset_y: place.offset_y,
            width: place.width,
            height: place.height,
            minimized: flag(place.minimized, start.minimized),
        }
    }

    /// The place this describes, filled in from where `panel` starts.
    pub(crate) fn place(&self, panel: Panel) -> PanelPlace {
        let start = panel.start();
        PanelPlace {
            visible: self.visible.unwrap_or(start.visible),
            anchor_right: self.anchor_right.unwrap_or(start.anchor_right),
            anchor_bottom: self.anchor_bottom.unwrap_or(start.anchor_bottom),
            offset_x: self.offset_x.or(start.offset_x),
            offset_y: self.offset_y.or(start.offset_y),
            width: self.width.or(start.width),
            height: self.height.or(start.height),
            minimized: self.minimized.unwrap_or(start.minimized),
        }
    }

    /// Nothing differs from the start place: the section is left out.
    pub(crate) fn is_empty(&self) -> bool {
        *self == SavedPanel::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{Panel, PanelPlace, SavedPanel};

    const PANELS: [Panel; 3] = [Panel::ObjectList, Panel::SliceControls, Panel::SliceView];

    #[test]
    fn untouched_panels_save_nothing() {
        for panel in PANELS {
            let saved = SavedPanel::record(panel, panel.start());
            assert!(saved.is_empty(), "{panel:?}");
            assert_eq!(saved.place(panel), panel.start());
        }
    }

    #[test]
    fn a_moved_panel_saves_and_restores_its_place() {
        let moved = PanelPlace {
            visible: false,
            anchor_right: false,
            anchor_bottom: true,
            offset_x: Some(12.0),
            offset_y: Some(30.5),
            width: Some(260.0),
            height: None,
            minimized: true,
        };
        for panel in PANELS {
            let saved = SavedPanel::record(panel, moved);
            assert!(!saved.is_empty());
            assert_eq!(saved.place(panel), moved, "{panel:?}");
        }
    }

    #[test]
    fn panels_start_in_their_own_corners() {
        let corner = |panel: Panel| (panel.start().anchor_right, panel.start().anchor_bottom);
        assert_eq!(corner(Panel::ObjectList), (true, false));
        assert_eq!(corner(Panel::SliceControls), (false, false));
        assert_eq!(corner(Panel::SliceView), (false, true));
        assert!(PANELS.iter().all(|p| p.start().visible && !p.start().minimized));
    }
}
