// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{orient::Preset, support_settings::SupportSettings};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub version: u32,
    pub name: String,
    pub steps: Vec<Step>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    Scale {
        percent: f64,
    },
    AutoOrient {
        #[serde(default)]
        preset: Preset,
    },
    Hollow {
        wall_mm: f64,
    },
    DrainHoles {
        count: usize,
        diameter_mm: f64,
    },
    Supports {
        #[serde(skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
        settings: SupportSettings,
    },
    Arrange,
    Slice,
    Export {
        format: String,
        folder: PathBuf,
        name_pattern: String,
    },
}
impl Step {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Scale { .. } => "scale",
            Self::AutoOrient { .. } => "auto orient",
            Self::Hollow { .. } => "hollow",
            Self::DrainHoles { .. } => "drain holes",
            Self::Supports { .. } => "supports",
            Self::Arrange => "arrange",
            Self::Slice => "slice",
            Self::Export { .. } => "export",
        }
    }
    fn valid(&self) -> bool {
        let positive = |v: f64| v.is_finite() && v > 0.0;
        match self {
            Self::Scale { percent } => positive(*percent),
            Self::Hollow { wall_mm } => positive(*wall_mm),
            Self::DrainHoles { count, diameter_mm } => {
                (1..=64).contains(count) && positive(*diameter_mm)
            }
            Self::Supports { settings, preset } => {
                settings.validate().is_ok()
                    && preset
                        .as_ref()
                        .is_none_or(|p| ["light", "medium", "heavy"].contains(&p.as_str()))
            }
            Self::Export {
                format,
                name_pattern,
                ..
            } => {
                [
                    "gcode-zip",
                    "3mf",
                    "png-per-layer",
                    "svg-per-layer",
                    "svg-combined",
                ]
                .contains(&format.as_str())
                    && valid_pattern(name_pattern)
            }
            _ => true,
        }
    }
}
fn valid_pattern(s: &str) -> bool {
    if s.is_empty() || matches!(s, "." | "..") || s.contains(['/', '\\', ':']) {
        return false;
    }
    let remaining = s.replace("{name}", "").replace("{plate}", "");
    !remaining.contains(['{', '}'])
}
impl Recipe {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.name.trim().is_empty()
            || !(1..=100).contains(&self.steps.len())
        {
            return Err("Recipe requires version 1, a name and 1 to 100 steps".into());
        }
        for step in &self.steps {
            if !step.valid() {
                return Err(format!("Invalid {} parameters", step.name()));
            }
        }
        Ok(())
    }
    pub fn parse(s: &str) -> Result<Self, String> {
        let recipe: Self = toml_edit::de::from_str(s).map_err(|e| e.to_string())?;
        recipe.validate()?;
        Ok(recipe)
    }
    pub fn serialize(&self) -> Result<String, String> {
        self.validate()?;
        toml_edit::ser::to_string_pretty(self).map_err(|e| e.to_string())
    }
}
pub fn user_dir() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("SLEPTIC_RECIPES_DIR") {
        return Ok(p.into());
    }
    sc_app_paths::project_dirs()
        .map(|d| d.data_dir().join("recipes"))
        .ok_or_else(|| "Cannot resolve recipe data directory".into())
}
pub fn defaults() -> Vec<Recipe> {
    vec![
        Recipe {
            version: 1,
            name: "Miniature: hollow-free, tree supports, slice".into(),
            steps: vec![
                Step::Supports {
                    preset: None,
                    settings: SupportSettings::default(),
                },
                Step::Slice,
            ],
        },
        Recipe {
            version: 1,
            name: "Hollow + drain + supports + slice + export".into(),
            steps: vec![
                Step::Hollow { wall_mm: 2.0 },
                Step::DrainHoles {
                    count: 2,
                    diameter_mm: 3.0,
                },
                Step::Supports {
                    preset: None,
                    settings: SupportSettings::default(),
                },
                Step::Slice,
                Step::Export {
                    format: "gcode-zip".into(),
                    folder: ".".into(),
                    name_pattern: "{name}_plate{plate}.zip".into(),
                },
            ],
        },
    ]
}
/// The built-in recipes (no file), then every recipe file in `dir` in
/// file-name order.
pub fn list(dir: &Path) -> Result<Vec<(Option<PathBuf>, Recipe)>, String> {
    let mut result = defaults()
        .into_iter()
        .map(|r| (None, r))
        .collect::<Vec<_>>();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(result),
        Err(e) => return Err(e.to_string()),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let p = entry.map_err(|e| e.to_string())?.path();
        if p.extension().and_then(|e| e.to_str()) == Some("toml") {
            paths.push(p);
        }
    }
    paths.sort();
    for path in paths {
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let r = Recipe::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        result.push((Some(path), r));
    }
    Ok(result)
}
pub fn load(name: &str) -> Result<Recipe, String> {
    let path = Path::new(name);
    if path.is_file() {
        return Recipe::parse(&fs::read_to_string(path).map_err(|e| e.to_string())?);
    }
    let mut matched = list(&user_dir()?)?
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.name == name);
    match (matched.next(), matched.next()) {
        (Some(r), None) => Ok(r),
        _ => Err(format!("Recipe name is missing or ambiguous: {name}")),
    }
}
pub fn save_new(dir: &Path, recipe: &Recipe) -> Result<PathBuf, String> {
    let text = recipe.serialize()?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for i in 1..=10000 {
        let path = dir.join(format!("recipe-{i}.toml"));
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("Recipe library is full".into())
}
