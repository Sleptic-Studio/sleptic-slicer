// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Prepared printer jobs, shared by CLI and GUI.

use sc_print_profiles::job_profile::JobTemplates;
use sc_print_profiles::{MaterialResolved, ResolvedPrintConfig};
use sc_sl1cer::contour::ContourStack;
use sc_sl1cer::profile::{LayerSchedule, SliceProfile};
use sc_sl1cer::raster::{accuracy::rasterize_accurate, pixel_map::PixelMap};
use std::collections::BTreeMap;

mod estimate;
pub mod islands;
pub mod risks;
pub use estimate::{
    estimated_cost, estimated_seconds, estimated_volume_mm3, format_cost, format_duration,
    format_volume_ml, seconds_per_mm,
};
pub use islands::{
    Island, LayerAnalysis, analyze_islands, analyze_layers, cure_bleed_px, find_islands,
    has_islands, island_spans, self_supporting_reach_px,
};
pub use risks::{PrintRisk, RiskKind, RiskSettings, Severity, analyze_layers_with_settings};

/// Per-layer process data. Speeds are mm/min and Z is the slab's top surface.
#[derive(Clone, Debug, PartialEq)]
pub struct ExposureLayer {
    pub index: usize,
    pub z_mm: f64,
    pub height_mm: f64,
    pub exposure_s: f64,
    pub is_bottom: bool,
    pub is_transition: bool,
    pub lift_distance_mm: f64,
    pub lift_speed: f64,
    pub retract_speed: f64,
    pub light_off_delay_s: f64,
    pub motion: sc_print_profiles::motion::Motion,
    pub has_motion_override: bool,
    z_nm: i64,
    height_nm: i64,
}

impl ExposureLayer {
    /// Seconds this layer takes on the machine: exposure plus its motion
    /// cycle (lift, retract, waits, light-off delay).
    pub fn duration_s(&self) -> f64 {
        if self.has_motion_override {
            self.exposure_s + self.motion.duration_s()
        } else {
            // Keep the legacy left-to-right addition order, including for
            // nonzero light-off delay, so old estimates keep their bits.
            self.exposure_s
                + self.light_off_delay_s
                + 60.0 * self.lift_distance_mm * (1.0 / self.lift_speed + 1.0 / self.retract_speed)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExposureSchedule {
    pub layers: Vec<ExposureLayer>,
}

impl ExposureSchedule {
    /// Build from the exact geometry layer grid, using integer nanometres for Z math.
    pub fn new(material: &MaterialResolved, grid: &LayerSchedule) -> Self {
        let LayerSchedule::Uniform(g) = grid;
        Self::uniform(
            material,
            g.total_layers().get(),
            g.first_layer_height().value(),
            g.layer_height().value(),
        )
    }

    pub fn uniform(m: &MaterialResolved, count: u32, first_mm: f64, height_mm: f64) -> Self {
        let first = (first_mm * 1_000_000.0).round() as i64;
        let height = (height_mm * 1_000_000.0).round() as i64;
        let layers = (0..count)
            .map(|i| {
                let is_bottom = i < m.bottom_layer_count;
                let t = i.saturating_sub(m.bottom_layer_count);
                let is_transition = !is_bottom && t < m.transition_layers;
                let normal = m.normal_exposure_sec.value();
                let bottom = m.bottom_exposure_sec.value();
                let exposure_s = if is_bottom {
                    bottom
                } else if is_transition {
                    bottom
                        + (normal - bottom) * (f64::from(t) + 1.0)
                            / (f64::from(m.transition_layers) + 1.0)
                } else {
                    normal
                };
                let z_nm = first + i64::from(i) * height;
                let height_nm = if i == 0 { first } else { height };
                let fallback = sc_print_profiles::motion::Motion::legacy(m);
                let normal = fallback.overlay(m.motion.normal);
                let bottom = normal.overlay(m.motion.bottom);
                let motion = if is_bottom {
                    bottom
                } else if is_transition {
                    bottom.overlay(m.motion.transition).interpolate(
                        normal,
                        (f64::from(t) + 1.0) / (f64::from(m.transition_layers) + 1.0),
                    )
                } else {
                    normal
                };
                ExposureLayer {
                    index: i as usize,
                    z_mm: z_nm as f64 / 1_000_000.0,
                    height_mm: height_nm as f64 / 1_000_000.0,
                    exposure_s,
                    is_bottom,
                    is_transition,
                    lift_distance_mm: m.lift_distance_mm.value(),
                    lift_speed: m.lift_speed_mm_min.value(),
                    retract_speed: m.retract_speed_mm_min.value(),
                    light_off_delay_s: motion.light_off_delay_s.unwrap_or(0.0),
                    motion,
                    has_motion_override: m.motion != Default::default(),
                    z_nm,
                    height_nm,
                }
            })
            .collect();
        Self { layers }
    }

    pub fn total_time_s(&self) -> f64 {
        // Compensated summation keeps a long print from rounding below a half-second
        // boundary simply because the same fractional travel time was added repeatedly.
        let mut total = 0.0;
        let mut correction = 0.0;
        for l in &self.layers {
            let seconds = l.duration_s();
            let adjusted = seconds - correction;
            let next = total + adjusted;
            correction = (next - total) - adjusted;
            total = next;
        }
        total
    }

    pub fn summary(&self, m: &MaterialResolved) -> String {
        let bottom = self.layers.iter().filter(|l| l.is_bottom).count();
        let transition = self.layers.iter().filter(|l| l.is_transition).count();
        format!(
            "bottom {bottom} at {:.3}s, transition {transition}, normal {:.3}s",
            m.bottom_exposure_sec.value(),
            m.normal_exposure_sec.value()
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrintEstimate {
    pub total_time_s: f64,
    pub volume_ml: f64,
    pub weight_g: f64,
    pub cost: Option<f64>,
    pub diagnostics: Vec<String>,
}

impl PrintEstimate {
    /// Alpha-weighted raster volume, using the exact export AA and mirror settings.
    pub fn new(
        schedule: &ExposureSchedule,
        volume_ml: f64,
        density: Option<f64>,
        price_per_liter: Option<f64>,
    ) -> Self {
        Self {
            total_time_s: schedule.total_time_s(),
            volume_ml,
            weight_g: volume_ml * density.unwrap_or(1.1),
            cost: price_per_liter.map(|p| p * volume_ml / 1000.0),
            diagnostics: if density.is_none() {
                vec!["Material density unspecified; using 1.1 g/ml.".into()]
            } else {
                vec![]
            },
        }
    }

    pub fn summary(&self, layers: usize) -> String {
        let seconds = self.total_time_s.round() as u64;
        let cost = self
            .cost
            .map_or_else(|| "unpriced".into(), |c| format!("{c:.2}"));
        format!(
            "{layers} layers; print time {}:{:02}:{:02}; volume {:.2} ml; weight {:.2} g; cost {cost}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60,
            self.volume_ml,
            self.weight_g
        )
    }
}

/// Lit-pixel area integrated over slab heights. Retains only one compressed raster.
pub fn raster_volume_ml(stack: &ContourStack, profile: &SliceProfile) -> f64 {
    let b = profile.build_volume();
    let r = profile.pixel_resolution();
    let map =
        PixelMap::new(b.x(), b.y(), *r, profile.mirror()).with_calibration(profile.calibration());
    let area = b.x_mm() * b.y_mm() / (f64::from(r.x()) * f64::from(r.y()));
    let LayerSchedule::Uniform(grid) = profile.layer_schedule();
    stack
        .layers()
        .iter()
        .enumerate()
        .map(|(i, layer)| {
            let raster =
                rasterize_accurate(layer, &map, profile.aa_level(), 0.0, profile.accuracy(), i);
            let alpha: f64 = raster
                .runs()
                .iter()
                .map(|r| f64::from(r.value) * f64::from(r.length) / 255.0)
                .sum();
            let height = if i == 0 {
                grid.first_layer_height().value()
            } else {
                grid.layer_height().value()
            };
            alpha * area * height / 1000.0
        })
        .sum()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateError {
    pub template: String,
    pub line: usize,
    pub placeholder: String,
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "template {} line {}: unknown or malformed placeholder {{{}}}",
            self.template, self.line, self.placeholder
        )
    }
}
impl std::error::Error for TemplateError {}

/// Substitute only known keys; values are never recursively interpreted as templates.
pub fn render_template(
    name: &str,
    template: &str,
    values: &BTreeMap<String, String>,
) -> Result<String, TemplateError> {
    let mut out = String::new();
    let mut rest = template;
    let mut line = 1;
    while let Some(start) = rest.find(['{', '}']) {
        let prefix = &rest[..start];
        line += prefix.bytes().filter(|b| *b == b'\n').count();
        out.push_str(prefix);
        let tail = &rest[start + 1..];
        let end = tail.find('}');
        if rest.as_bytes()[start] != b'{' || end.is_none() {
            return Err(TemplateError {
                template: name.into(),
                line,
                placeholder: rest[start..].into(),
            });
        }
        let end = end.unwrap();
        let key = &tail[..end];
        let value = values.get(key).ok_or_else(|| TemplateError {
            template: name.into(),
            line,
            placeholder: key.into(),
        })?;
        out.push_str(value);
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Default controller job used when a printer has no explicit templates.
pub fn default_job_templates() -> JobTemplates {
    JobTemplates {
        start: "G90\n".into(),
        layer: ";LAYER_START:{layer}\n;Z:{z}\nG0 Z{z} F{retract_speed}\nG91\nG0 Z{lift_distance} F{lift_speed}\nG0 Z-{lift_distance} F{retract_speed}\nG90\nG4 P{light_off_ms}\nM6054 \"{image}\"\nM106 S255\nG4 P{exposure_ms}\nM106 S0\n;LAYER_END\n".into(),
        end: "G90\nG1 Z{end_lift_z} F{retract_speed}\nM18\n".into(),
    }
}

/// Build all script text before the writer creates the destination archive.
pub fn render_job(
    config: &ResolvedPrintConfig,
    schedule: &ExposureSchedule,
    estimate: &PrintEstimate,
) -> Result<String, TemplateError> {
    for l in &schedule.layers {
        if l.has_motion_override {
            l.motion.validate().map_err(|message| TemplateError {
                template: "layer".into(),
                line: 1,
                placeholder: message.into(),
            })?;
        }
    }
    let default = default_job_templates();
    let templates = config
        .printer
        .job
        .as_ref()
        .map_or(&default, |j| &j.templates);
    let mut values = BTreeMap::new();
    if let Some(job) = &config.printer.job {
        for (k, v) in &job.custom {
            values.insert(format!("custom.{k}"), v.to_string());
        }
    }
    values.insert("layers_total".into(), schedule.layers.len().to_string());
    values.insert(
        "total_time_s".into(),
        format!("{:.3}", estimate.total_time_s),
    );
    values.insert("volume_ml".into(), format!("{:.3}", estimate.volume_ml));
    let hardware = &config.printer.hardware;
    let res = hardware.pixel_resolution();
    let last_z = schedule.layers.last().map_or(0.0, |l| l.z_mm);
    values.insert(
        "end_lift_z".into(),
        format!("{:.3}", hardware.build_volume().z_mm().min(last_z + 60.0)),
    );

    let mut out = format!(
        ";fileName:run\n;machineType:{}\n;estimatedPrintTime:{:.3}\n;volume:{:.3}\n;resin:{}\n;layerHeight:{:.3}\n;resolutionX:{}\n;resolutionY:{}\n",
        config.printer.identity.id,
        estimate.total_time_s,
        estimate.volume_ml,
        config.material_resolved.identity.id,
        schedule
            .layers
            .get(1)
            .or_else(|| schedule.layers.first())
            .map_or(config.material_resolved.layer_height_mm.value(), |l| l
                .height_mm),
        res.x(),
        res.y()
    );
    // Global templates may use the first/last layer values as well as aggregate values.
    if let Some(first) = schedule.layers.first() {
        layer_values(&mut values, first);
    }
    out.push_str(&render_template("start", &templates.start, &values)?);
    for layer in &schedule.layers {
        layer_values(&mut values, layer);
        if layer.has_motion_override {
            values.insert(
                "motion_cycle".into(),
                motion_cycle(layer, motion_command(&templates.layer)),
            );
        }
        let template = if layer.has_motion_override {
            if templates.layer.contains("{motion_cycle}") {
                templates.layer.clone()
            } else if let Some(start) = templates.layer.find("G91") {
                if let Some(end) = templates.layer[start..].find("G90") {
                    let end = start + end;
                    let line_end = templates.layer[end..]
                        .find('\n')
                        .map_or(templates.layer.len(), |i| end + i + 1);
                    format!(
                        "{}{motion}{}",
                        &templates.layer[..start],
                        &templates.layer[line_end..],
                        motion = "{motion_cycle}"
                    )
                } else {
                    return Err(TemplateError {
                        template: "layer".into(),
                        line: 1,
                        placeholder:
                            "Motion overrides require {motion_cycle} or a G91/G90 peel block".into(),
                    });
                }
            } else {
                return Err(TemplateError {
                    template: "layer".into(),
                    line: 1,
                    placeholder: "Motion overrides require {motion_cycle} or a G91/G90 peel block"
                        .into(),
                });
            }
        } else {
            templates.layer.clone()
        };
        out.push_str(&render_template("layer", &template, &values)?);
    }
    out.push_str(&render_template("end", &templates.end, &values)?);
    Ok(out)
}

fn layer_values(values: &mut BTreeMap<String, String>, l: &ExposureLayer) {
    values.insert(
        "lift_slow_distance_mm".into(),
        format!("{:.3}", l.motion.lift_slow_distance_mm.unwrap_or(0.0)),
    );
    values.insert(
        "lift_slow_speed_mm_min".into(),
        format!("{:.3}", l.motion.lift_slow_speed_mm_min.unwrap_or(0.0)),
    );
    values.insert(
        "lift_fast_distance_mm".into(),
        format!("{:.3}", l.motion.lift_fast_distance_mm.unwrap_or(0.0)),
    );
    values.insert(
        "lift_fast_speed_mm_min".into(),
        format!("{:.3}", l.motion.lift_fast_speed_mm_min.unwrap_or(0.0)),
    );
    values.insert(
        "retract_slow_distance_mm".into(),
        format!("{:.3}", l.motion.retract_slow_distance_mm.unwrap_or(0.0)),
    );
    values.insert(
        "retract_slow_speed_mm_min".into(),
        format!("{:.3}", l.motion.retract_slow_speed_mm_min.unwrap_or(0.0)),
    );
    values.insert(
        "retract_fast_distance_mm".into(),
        format!("{:.3}", l.motion.retract_fast_distance_mm.unwrap_or(0.0)),
    );
    values.insert(
        "retract_fast_speed_mm_min".into(),
        format!("{:.3}", l.motion.retract_fast_speed_mm_min.unwrap_or(0.0)),
    );
    values.insert(
        "wait_before_cure_s".into(),
        format!("{:.3}", l.motion.wait_before_cure_s.unwrap_or(0.0)),
    );
    values.insert(
        "wait_after_lift_s".into(),
        format!("{:.3}", l.motion.wait_after_lift_s.unwrap_or(0.0)),
    );
    values.insert(
        "wait_after_retract_s".into(),
        format!("{:.3}", l.motion.wait_after_retract_s.unwrap_or(0.0)),
    );
    values.insert(
        "light_off_delay_s".into(),
        format!("{:.3}", l.motion.light_off_delay_s.unwrap_or(0.0)),
    );

    for (key, value) in [
        ("layer", (l.index + 1).to_string()),
        ("layer_index0", l.index.to_string()),
        ("z", decimal_nm(l.z_nm)),
        ("layer_height", decimal_nm(l.height_nm)),
        ("exposure_s", format!("{:.3}", l.exposure_s)),
        ("exposure_ms", format!("{:.0}", l.exposure_s * 1000.0)),
        (
            "light_off_ms",
            format!("{:.0}", l.light_off_delay_s * 1000.0),
        ),
        ("image", format!("{}.png", l.index + 1)),
        ("lift_distance", format!("{:.3}", l.lift_distance_mm)),
        ("lift_speed", format!("{:.3}", l.lift_speed)),
        ("retract_speed", format!("{:.3}", l.retract_speed)),
        ("is_bottom", l.is_bottom.to_string()),
        ("motion_cycle", motion_cycle(l, "G1")),
    ] {
        values.insert(key.into(), value);
    }
}

// Preserve the printer template's controlled/rapid move convention. Templates
// using the standalone placeholder default to a controlled feed move.
fn motion_command(template: &str) -> &'static str {
    let Some((_, relative)) = template.split_once("G91") else {
        return "G1";
    };
    for line in relative
        .lines()
        .take_while(|line| !line.trim_start().starts_with("G90"))
    {
        match line.split_ascii_whitespace().next() {
            Some("G0" | "G00") => return "G0",
            Some("G1" | "G01") => return "G1",
            _ => {}
        }
    }
    "G1"
}
fn motion_cycle(l: &ExposureLayer, command: &str) -> String {
    let m = l.motion;
    let mut out = String::from("G91\n");
    for (distance, speed) in [
        (m.lift_slow_distance_mm, m.lift_slow_speed_mm_min),
        (m.lift_fast_distance_mm, m.lift_fast_speed_mm_min),
    ] {
        if distance.unwrap_or(0.0) > 0.0 {
            out.push_str(&format!(
                "{command} Z{:.3} F{:.3}\n",
                distance.unwrap_or(0.0),
                speed.unwrap_or(l.lift_speed)
            ));
        }
    }
    if m.wait_after_lift_s.unwrap_or(0.0) > 0.0 {
        out.push_str(&format!(
            "G4 P{:.0}\n",
            m.wait_after_lift_s.unwrap_or(0.0) * 1000.0
        ));
    }
    for (distance, speed) in [
        (m.retract_fast_distance_mm, m.retract_fast_speed_mm_min),
        (m.retract_slow_distance_mm, m.retract_slow_speed_mm_min),
    ] {
        if distance.unwrap_or(0.0) > 0.0 {
            out.push_str(&format!(
                "{command} Z-{:.3} F{:.3}\n",
                distance.unwrap_or(0.0),
                speed.unwrap_or(l.retract_speed)
            ));
        }
    }
    out.push_str("G90\n");
    for wait in [m.wait_after_retract_s, m.wait_before_cure_s] {
        if wait.unwrap_or(0.0) > 0.0 {
            out.push_str(&format!("G4 P{:.0}\n", wait.unwrap_or(0.0) * 1000.0));
        }
    }
    out
}

fn decimal_nm(nm: i64) -> String {
    let microns = (nm + 500) / 1000;
    format!("{}.{:03}", microns / 1000, microns % 1000)
}

#[cfg(test)]
mod tests_job;

/// Render sequential region exposures with one motion cycle per layer. Custom
/// templates must identify their exposure block with M6054 and ;LAYER_END.
/// `exposures[tile][layer]` contains bottom/transition-adjusted seconds.
pub fn render_multi_job(
    config: &ResolvedPrintConfig,
    schedule: &ExposureSchedule,
    estimate: &PrintEstimate,
    exposures: &[Vec<f64>],
) -> Result<String, TemplateError> {
    let mut config = config.clone();
    let mut templates = config
        .printer
        .job
        .as_ref()
        .map(|j| j.templates.clone())
        .unwrap_or_else(default_job_templates);
    let start = templates.layer.find("M6054").ok_or_else(|| TemplateError {
        template: "layer".into(),
        line: 1,
        placeholder: "Multi-exposure requires an M6054 exposure block".into(),
    })?;
    let end = templates.layer[start..]
        .find(";LAYER_END")
        .map(|i| start + i)
        .unwrap_or(templates.layer.len());
    let exposure_template = templates.layer[start..end].to_string();
    templates
        .layer
        .replace_range(start..end, "{region_exposures}");
    let original = config.printer.job.clone();
    if let Some(job) = &mut config.printer.job {
        job.templates = templates.clone();
    } else {
        config.printer.job = Some(sc_print_profiles::job_profile::JobProfile {
            kind: sc_print_profiles::job_profile::JobKind::GcodeZip,
            templates: templates.clone(),
            custom: BTreeMap::new(),
        });
    }
    // Render the ordinary script with a unique marker, then replace each layer's
    // marker. Rendered custom values remain literal, matching render_job.
    config.printer.job.as_mut().unwrap().templates.layer = templates
        .layer
        .replace("{region_exposures}", ";SLEPTIC_REGION_EXPOSURES\n");
    let mut script = render_job(&config, schedule, estimate)?;
    for layer in &schedule.layers {
        let mut block = String::new();
        for (tile, values) in exposures.iter().enumerate() {
            let exposure = *values.get(layer.index).ok_or_else(|| TemplateError {
                template: "layer".into(),
                line: 1,
                placeholder: "Region exposure schedule length".into(),
            })?;
            if !exposure.is_finite() || exposure <= 0.0 {
                return Err(TemplateError {
                    template: "layer".into(),
                    line: 1,
                    placeholder: "Positive finite tile exposure required".into(),
                });
            }
            let mut values = BTreeMap::new();
            if let Some(job) = &original {
                for (k, v) in &job.custom {
                    values.insert(format!("custom.{k}"), v.to_string());
                }
            }
            layer_values(&mut values, layer);
            values.insert(
                "image".into(),
                format!("{}-tile{}.png", layer.index + 1, tile + 1),
            );
            values.insert("exposure_s".into(), format!("{exposure:.3}"));
            values.insert("exposure_ms".into(), format!("{:.0}", exposure * 1000.0));
            block.push_str(&render_template("exposure", &exposure_template, &values)?);
        }
        script = script.replacen(";SLEPTIC_REGION_EXPOSURES\n", &block, 1);
    }
    Ok(script)
}
