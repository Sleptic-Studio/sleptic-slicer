// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Deterministic desktop launch and verification options.
use std::{ffi::OsString, path::PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LaunchOptions {
    pub open: Vec<PathBuf>,
    pub automated: bool,
    pub printer: Option<String>,
    pub material: Option<String>,
    pub slice: bool,
    pub slice_all: bool,
    pub plate: Option<u32>,
    pub batch_mode: Option<String>,
    pub batch_sources: Vec<PathBuf>,
    pub supports: bool,
    pub auto_orient: bool,
    pub arrange: bool,
    pub hollow: bool,
    pub auto_drains: bool,
    pub fill_plate: bool,
    pub auto_prepare: bool,
    pub mirror: Option<i32>,
    pub verify_printer_error: bool,
    pub verify_measure: bool,
    pub verify_input: Option<String>,
    pub settings_mode: Option<String>,
    pub search: Option<String>,
    pub theme: Option<String>,
    pub preview_view: Option<String>,
    pub benchmark: bool,
    pub workspace: Option<String>,
    pub preview_layer: Option<u32>,
    /// Layer height (mm) set through the Slice settings before slicing.
    pub layer_height: Option<String>,
    pub risk_jump: Option<usize>,
    pub window: Option<(u32, u32)>,
    /// Top-left of the window in physical screen pixels, so automated runs
    /// can open on a chosen monitor instead of the user's main one.
    pub window_at: Option<(i32, i32)>,
    pub screenshot: Option<PathBuf>,
    pub dump_state: Option<PathBuf>,
    /// Open a window of its own even when the app is already running,
    /// instead of handing `--open` files to that window (test runs).
    pub new_instance: bool,
}

#[derive(Debug)]
pub(crate) struct LaunchError(String);
impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for LaunchError {}

impl LaunchOptions {
    pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, LaunchError> {
        let mut result = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let flag = arg.to_string_lossy();
            if flag.starts_with('-') && flag != "--open" { result.automated = true; }
            if flag == "--new-instance" {
                result.new_instance = true;
                continue;
            }
            if flag == "--benchmark-preview" {
                result.benchmark = true;
                continue;
            }
            if flag == "--auto-orient" {
                result.auto_orient = true;
                continue;
            }
            if flag == "--arrange" {
                result.arrange = true;
                continue;
            }
            if flag == "--verify-printer-error" {
                result.verify_printer_error = true;
                continue;
            }
            if flag == "--verify-measure" {
                result.verify_measure = true;
                continue;
            }
            if flag == "--hollow" {
                result.hollow = true;
                continue;
            }
            if flag == "--auto-drains" {
                result.auto_drains = true;
                continue;
            }
            if flag == "--fill-plate" {
                result.fill_plate = true;
                continue;
            }
            if flag == "--auto-prepare" {
                result.auto_prepare = true;
                continue;
            }
            if flag == "--slice-all" {
                result.slice_all = true;
                continue;
            }
            if flag == "--slice" {
                result.slice = true;
                continue;
            }
            if !flag.starts_with('-') {
                result.open.push(arg.into());
                continue;
            }
            let value = args
                .next()
                .ok_or_else(|| LaunchError(format!("Missing value for {flag}")))?;
            let string = value.to_string_lossy().into_owned();
            match flag.as_ref() {
                "--mirror" if matches!(string.as_str(), "x" | "y" | "z") => {
                    result.mirror = Some(match string.as_str() {
                        "x" => 0,
                        "y" => 1,
                        _ => 2,
                    })
                }
                "--supports" if string == "auto" => result.supports = true,
                "--supports" if string == "off" => result.supports = false,
                "--open" => result.open.push(value.into()),
                "--plate" => {
                    let plate = string
                        .parse::<u32>()
                        .map_err(|_| LaunchError("Invalid plate".into()))?;
                    if plate == 0 {
                        return Err(LaunchError("Plate is 1-based".into()));
                    }
                    result.plate = Some(plate);
                }
                "--batch-source" => result.batch_sources.push(value.into()),
                "--batch-mode" if matches!(string.as_str(), "dialog" | "summary") => {
                    result.batch_mode = Some(string)
                }
                "--printer" => result.printer = Some(string),
                "--material" => result.material = Some(string),
                "--settings-mode"
                    if matches!(string.as_str(), "simple" | "advanced" | "expert") =>
                {
                    result.settings_mode = Some(string)
                }
                "--verify-input"
                    if matches!(
                        string.as_str(),
                        "machine-orientation"
                            | "machine-xy-ruler"
                            | "machine-z-tower"
                            | "machine-xy-dialog"
                            | "machine-orientation-dialog"
                            | "machine-z-dialog"
                            | "calibration-menu"
                            | "overhang-test"
                            | "audit-preferences"
                            | "audit-printer"
                            | "audit-profile"
                            | "audit-fork"
                            | "audit-extend"
                            | "audit-error"
                            | "audit-error-technical"
                            | "audit-preferences-printer"
                            | "audit-preferences-resin"
                            | "audit-preferences-slicing"
                            | "audit-info"
                            | "audit-replace"
                            | "audit-overwrite"
                            | "audit-about"
                            | "audit-shortcuts"
                            | "popup-mirror"
                            | "popup-plate"
                            | "popup-slice"
                            | "popup-export"
                            | "popup-navigation"
                            | "popup-more"
                            | "hub-home"
                            | "confirm"
                            | "setup-0"
                            | "setup-1"
                            | "setup-2"
                            | "setup-3"
                            | "hub-device"
                            | "hub-project"
                            | "panel-hollow"
                            | "panel-supports"
                            | "chooser-printer"
                            | "object-panel-hidden"
                            | "place-face"
                            | "cube-face"
                            | "cube-edge"
                            | "cube-corner"
                            | "layer-closeup"
                            | "support-closeup"
                            | "plate-label"
                            | "panel-batch"
                            | "plate-menu"
                            | "object-menu"
                            | "move-menu"
                            | "multi"
                            | "stack"
                            | "supports"
                            | "global-menu"
                            | "orbit"
                            | "drag"
                            | "toolbar-move"
                            | "toolbar-scale"
                            | "toolbar-rotate"
                            | "model-simplify"
                            | "model-cut"
                            | "model-split"
                            | "model-label"
                            | "model-boolean"
                            | "supports-comparison"
                            | "orient-options"
                            | "motion-editor"
                            | "calibration-dialog"
                            | "calibration-plate"
                            | "calibration-preview"
                            | "recipe-editor"
                            | "recipe-progress"
                            | "recipe-summary"
                    ) =>
                {
                    result.verify_input = Some(string)
                }
                "--search" => result.search = Some(string),
                "--theme" if matches!(string.as_str(), "light" | "dark") => {
                    result.theme = Some(string)
                }
                "--preview-view" if matches!(string.as_str(), "2d" | "3d") => {
                    result.preview_view = Some(string)
                }
                "--workspace" if matches!(string.as_str(), "prepare" | "preview") => {
                    result.workspace = Some(string)
                }
                "--risk-jump" => {
                    result.risk_jump = Some(
                        string
                            .parse()
                            .map_err(|_| LaunchError("Invalid risk index".into()))?,
                    );
                }
                "--preview-layer" => {
                    result.preview_layer = Some(
                        string
                            .parse()
                            .map_err(|_| LaunchError("Invalid preview layer".into()))?,
                    )
                }
                "--window" => {
                    let (w, h) = string
                        .split_once('x')
                        .ok_or_else(|| LaunchError("Expected WxH".into()))?;
                    let size = (
                        w.parse::<u32>()
                            .map_err(|_| LaunchError("Invalid width".into()))?,
                        h.parse::<u32>()
                            .map_err(|_| LaunchError("Invalid height".into()))?,
                    );
                    if size.0 < 1100 || size.1 < 700 {
                        return Err(LaunchError("Minimum window size is 1100x700".into()));
                    }
                    result.window = Some(size);
                }
                "--window-at" => {
                    let (x, y) = string
                        .split_once(',')
                        .ok_or_else(|| LaunchError("Expected X,Y".into()))?;
                    result.window_at = Some((
                        x.trim()
                            .parse()
                            .map_err(|_| LaunchError("Invalid window X".into()))?,
                        y.trim()
                            .parse()
                            .map_err(|_| LaunchError("Invalid window Y".into()))?,
                    ));
                }
                "--layer-height" => result.layer_height = Some(string),
                "--screenshot" => result.screenshot = Some(value.into()),
                "--dump-state" => result.dump_state = Some(value.into()),
                _ => {
                    return Err(LaunchError(format!(
                        "Invalid option or value: {flag} {string}"
                    )));
                }
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
#[path = "tests_launch_options.rs"]
mod tests;
