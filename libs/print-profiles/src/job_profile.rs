// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! How a printer wants a job written when its format is a text template
//! (G-code in a zip): the start, per-layer and end templates, and values
//! of the printer's own that the templates may use.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};

/// A printer's job templates and its own template values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobProfile {
    pub kind: JobKind,
    pub templates: JobTemplates,
    /// Values by name, for `{name}` in the templates.
    #[serde(default)]
    pub custom: BTreeMap<String, CustomValue>,
}

/// The kind of job file the templates make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobKind {
    #[serde(rename = "gcode-zip")]
    GcodeZip,
}

/// The text written before the first layer, for each layer, and after the
/// last.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobTemplates {
    pub start: String,
    pub layer: String,
    pub end: String,
}

/// A template value as TOML gives it: text, a flag, or a number.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CustomValue {
    String(String),
    Bool(bool),
    Integer(i64),
    Float(f64),
}

/// How a value appears in the template's output; decimals get three
/// places, as G-code expects.
impl fmt::Display for CustomValue {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            CustomValue::Float(number) => write!(out, "{number:.3}"),
            CustomValue::Integer(number) => write!(out, "{number}"),
            CustomValue::Bool(flag) => write!(out, "{flag}"),
            CustomValue::String(ref text) => out.write_str(text),
        }
    }
}
