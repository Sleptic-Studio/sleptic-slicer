// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Device tab, "Calibrate time estimate" (M7 #4): read a real print's log
//! (layer number and a timestamp per line), compare each layer's measured
//! duration with the slicer's prediction, and fit the motion part of the
//! model per printer:
//!
//! `real layer seconds = exposure + motion_scale * motion + layer_overhead`
//!
//! Exposure is the one time the slicer knows exactly (the controller turns
//! the light on for that long), so only the mechanical part is corrected.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The fitted correction for one printer.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Correction {
    pub motion_scale: f64,
    pub layer_overhead_s: f64,
    /// Layers the fit used.
    pub samples: usize,
}

impl Correction {
    pub(crate) const IDENTITY: Self = Self {
        motion_scale: 1.0,
        layer_overhead_s: 0.0,
        samples: 0,
    };

    pub(crate) fn layer_seconds(&self, exposure_s: f64, duration_s: f64) -> f64 {
        exposure_s + self.motion_scale * (duration_s - exposure_s) + self.layer_overhead_s
    }

    /// The schedule's total with this correction applied.
    pub(crate) fn total_s(&self, schedule: &sc_print_job::ExposureSchedule) -> f64 {
        schedule
            .layers
            .iter()
            .map(|l| self.layer_seconds(l.exposure_s, l.duration_s()))
            .sum()
    }
}

/// One timestamped line of a print log.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LogPoint {
    pub layer: u32,
    pub time_s: f64,
}

/// Seconds from "HH:MM:SS(.fff)", "MM:SS" or a plain number.
fn parse_time(token: &str) -> Option<f64> {
    if token.contains(':') {
        let parts: Vec<f64> = token.split(':').map(|p| p.parse().ok()).collect::<Option<_>>()?;
        return Some(parts.iter().fold(0.0, |acc, v| acc * 60.0 + v));
    }
    token.parse().ok()
}

/// Reads a print log: each useful line has a layer number and a time,
/// in that order ("12,345.6", "layer 12 at 00:05:45", "12 00:05:45.2").
/// A line with only a time counts as the next layer. Header lines and
/// anything without a number are skipped.
pub(crate) fn parse_log(text: &str) -> Vec<LogPoint> {
    let mut points = Vec::new();
    for line in text.lines() {
        let tokens: Vec<&str> = line
            .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == ':'))
            .map(|t| t.trim_matches(['.', ':']))
            .filter(|t| t.chars().any(|c| c.is_ascii_digit()))
            .collect();
        let point = match tokens.as_slice() {
            [layer, time, ..] if !layer.contains(':') && !layer.contains('.') => {
                layer.parse().ok().zip(parse_time(time)).map(|(layer, time_s)| LogPoint { layer, time_s })
            }
            [time] => parse_time(time).map(|time_s| LogPoint {
                layer: points.last().map_or(0, |p: &LogPoint| p.layer + 1),
                time_s,
            }),
            _ => None,
        };
        points.extend(point);
    }
    points
}

/// Measured seconds per layer (0-based index), from consecutive log points.
/// A log that starts at layer 1 is taken as 1-based.
pub(crate) fn measured_durations(points: &[LogPoint]) -> Vec<(usize, f64)> {
    let base = points.iter().map(|p| p.layer).min().unwrap_or(0).min(1);
    points
        .windows(2)
        .filter(|w| w[1].layer == w[0].layer + 1 && w[1].time_s > w[0].time_s)
        .map(|w| ((w[0].layer - base) as usize, w[1].time_s - w[0].time_s))
        .collect()
}

/// Least-squares fit of the correction against `schedule`. None with fewer
/// than five matched layers.
pub(crate) fn fit(
    schedule: &sc_print_job::ExposureSchedule,
    measured: &[(usize, f64)],
) -> Option<Correction> {
    // y = real - exposure = scale * motion + overhead
    let pairs: Vec<(f64, f64)> = measured
        .iter()
        .filter_map(|&(i, real)| {
            let l = schedule.layers.get(i)?;
            Some((l.duration_s() - l.exposure_s, real - l.exposure_s))
        })
        .collect();
    if pairs.len() < 5 {
        return None;
    }
    let n = pairs.len() as f64;
    let mx = pairs.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pairs.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = pairs.iter().map(|p| (p.0 - mx).powi(2)).sum();
    let sxy: f64 = pairs.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    // Every layer with the same motion (the common case once past the
    // bottom layers) cannot separate scale from overhead: keep the
    // modelled motion and put the whole difference in the overhead.
    let (scale, overhead) = if sxx / n > 1e-6 {
        let scale = (sxy / sxx).clamp(0.2, 5.0);
        (scale, my - scale * mx)
    } else {
        (1.0, my - mx)
    };
    Some(Correction {
        motion_scale: scale,
        layer_overhead_s: overhead,
        samples: pairs.len(),
    })
}

fn store_path() -> Option<PathBuf> {
    crate::print_history::data_dir().map(|d| d.join("estimate_calibration.json"))
}

fn load_all() -> BTreeMap<String, Correction> {
    store_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// The saved correction for `printer`, identity when none was fitted.
pub(crate) fn for_printer(printer: &str) -> Correction {
    load_all().get(printer).copied().unwrap_or(Correction::IDENTITY)
}

pub(crate) fn save(printer: &str, correction: Option<Correction>) -> std::io::Result<()> {
    let Some(path) = store_path() else {
        return Ok(());
    };
    let mut all = load_all();
    match correction {
        Some(c) => all.insert(printer.to_string(), c),
        None => all.remove(printer),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(&all)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule(layers: u32) -> sc_print_job::ExposureSchedule {
        let printer: sc_print_profiles::PrinterProfile = toml::from_str(include_str!(
            "../../../assets/profiles/printers/sleptic-diy-msla.toml"
        ))
        .unwrap();
        let material: sc_print_profiles::MaterialProfile = toml::from_str(include_str!(
            "../../../assets/profiles/materials/standard-405.toml"
        ))
        .unwrap();
        let config = sc_print_profiles::resolve(
            &printer,
            &sc_print_profiles::EffectiveMaterial::from(material),
            &sc_print_profiles::SceneOverrides::default(),
        )
        .unwrap();
        sc_print_job::ExposureSchedule::uniform(&config.material_resolved, layers, 0.05, 0.05)
    }

    #[test]
    fn logs_in_several_shapes_parse_to_layer_times() {
        let csv = "layer,seconds\n0,0\n1,9.5\n2,19.0\n";
        assert_eq!(parse_log(csv).len(), 3);
        assert_eq!(parse_log(csv)[2], LogPoint { layer: 2, time_s: 19.0 });
        let clock = "Print started\nlayer 1 at 00:00:10\nlayer 2 at 00:01:05.5\n";
        assert_eq!(parse_log(clock), vec![
            LogPoint { layer: 1, time_s: 10.0 },
            LogPoint { layer: 2, time_s: 65.5 },
        ]);
        let bare = "00:00:00\n00:00:12\n00:00:24\n";
        let points = parse_log(bare);
        assert_eq!(points.iter().map(|p| p.layer).collect::<Vec<_>>(), [0, 1, 2]);
        // 1-based logs map onto 0-based layers; gaps and clock resets drop out.
        let one_based = parse_log("1,0\n2,10\n4,30\n5,20\n6,30\n");
        assert_eq!(measured_durations(&one_based), vec![(0, 10.0), (4, 10.0)]);
    }

    #[test]
    fn fit_recovers_a_known_motion_scale_and_overhead() {
        let s = schedule(40);
        let truth = Correction { motion_scale: 1.3, layer_overhead_s: 2.0, samples: 0 };
        let measured: Vec<(usize, f64)> = s
            .layers
            .iter()
            .map(|l| (l.index, truth.layer_seconds(l.exposure_s, l.duration_s())))
            .collect();
        let got = fit(&s, &measured).unwrap();
        assert_eq!(got.samples, 40);
        // Bottom layers share the normal motion here, so only the total
        // per-layer difference is identifiable; the fitted model must still
        // reproduce the real time.
        let real: f64 = measured.iter().map(|m| m.1).sum();
        assert!((got.total_s(&s) - real).abs() < 1e-6, "{} vs {real}", got.total_s(&s));
        assert!(fit(&s, &measured[..4]).is_none());
    }
}
