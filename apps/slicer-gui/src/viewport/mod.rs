// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The 3D viewport of a main window: the controller that owns it, and the
//! camera, input, gizmo, rendering and object-list parts it is made of.
// The controller and what it holds for the window's life.
mod controller;
mod session;
mod frame_host;
mod screen;

// Moving the camera.
mod orbit;
mod view_steps;

// Pointer and keyboard input.
mod router;
mod buttons;
mod arrow_keys;

// The transform handles: their drags, states and frames.
mod handle_drag;
mod handle_states;
mod handle_frames;
mod move_limits;

// The object panel.
mod parts_panel;

#[cfg(test)]
mod tests_controller_handle_before_rendering;
#[cfg(test)]
mod tests_controller_lifecycle;
#[cfg(test)]
mod tests_controller_section_gating;

pub(crate) use controller::ViewController;
pub(crate) use parts_panel::support_row_owner;
pub(crate) use frame_host::RESIZE_QUIET;
pub(crate) use screen::{ScreenDelta, ScreenPoint};
