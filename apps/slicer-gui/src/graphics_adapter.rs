// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Choosing the graphics adapter the app draws with, and opening a device
//! on it. `run()` does this once and hands the pair to Slint
//! (`WGPUConfiguration::Manual`), so every window uses this choice.
//! `Instance::request_adapter` is not enough: with the default
//! `PowerPreference::None` it does not sort at all and returns adapters in
//! enumeration order (DXGI order on Windows). With one GPU that is fine;
//! with a discrete GPU beside an integrated one, the first listed is not
//! always the one that can drive the window.
//! So the order is decided here: real GPUs before software ones, discrete
//! before integrated, and an environment override for diagnosing a machine
//! we cannot test on. The decisions are plain functions, testable without a
//! GPU ([`adapter_preference_order`], `open_in_order`, `open_with_second_try`);
//! [`open_best_adapter`] only adds the wgpu calls.
//! Before any request, each adapter's advertised features and limits are
//! checked the way wgpu checks them ([`unmet_need`]). An adapter
//! short there will be refused every time, so it is not retried; one that
//! passes and is still refused failed for some passing reason (driver or
//! system state), which one delayed second try can get past.
use std::time::Duration;

use slint::wgpu_29::wgpu;

/// Forces an adapter by name: wgpu's own variable, matched the same way (a
/// substring, any case). Unlike wgpu's helper, a name that matches nothing
/// is ignored instead of panicking.
pub const ADAPTER_OVERRIDE_VAR: &str = "WGPU_ADAPTER_NAME";

/// What the ordering looks at in `wgpu::AdapterInfo`; kept apart from the
/// adapter so the ordering can be tested on machines without a GPU (real
/// devices are covered in `tests/integration_graphics_adapter.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdapterTraits<'a> {
    pub adapter_label: &'a str,
    pub kind: wgpu::DeviceType,
}

/// Lower is better. A slicer redraws a build volume and models all the
/// time, so a discrete GPU comes first. A software adapter comes last on
/// purpose: it can open a device and then fail to show anything, which is
/// worse than never picking it.
fn kind_order(kind: wgpu::DeviceType) -> u8 {
    use wgpu::DeviceType as Kind;
    match kind {
        Kind::DiscreteGpu => 0,
        Kind::IntegratedGpu => 1,
        Kind::VirtualGpu => 2,
        // Unknown kinds: likely real hardware, but no promise.
        Kind::Other => 3,
        Kind::Cpu => 4,
    }
}

/// Whether `name` contains `wanted`, ignoring case: adapter names carry
/// vendor words nobody wants to type out ("NVIDIA GeForce RTX 4070 Ti").
fn name_contains(name: &str, wanted: &str) -> bool {
    let (name, wanted) = (name.to_lowercase(), wanted.to_lowercase());
    name.contains(wanted.as_str())
}

/// The candidates' indices, best first. A matching `preferred_name` beats
/// the device type, since the override must be able to reach an adapter
/// this order puts last; one matching nothing changes nothing, so a stale
/// or misspelt variable cannot stop the app starting. Equals keep their
/// enumeration order, the platform's own preference.
pub fn adapter_preference_order(candidates: &[AdapterTraits<'_>], preferred_name: Option<&str>) -> Vec<usize> {
    let rank = |facts: &AdapterTraits<'_>| {
        let named = preferred_name.is_some_and(|wanted| name_contains(facts.adapter_label, wanted));
        (!named, kind_order(facts.kind))
    };
    let mut order: Vec<usize> = candidates.iter().enumerate().map(|(i, _)| i).collect();
    // Stable, which keeps enumeration order among equals.
    order.sort_by_key(|&i| rank(&candidates[i]));
    order
}

/// The wait before a refused device request is tried again: long enough
/// for a device caught changing power state to settle, short enough that
/// falling through to the next adapter is not a noticeable stall.
const SECOND_TRY_WAIT: Duration = Duration::from_millis(300);

/// An advertised capability short of what the device request needs. The
/// test is wgpu's own, so a shortfall means a certain refusal: the request
/// is still made (wgpu decides), but never retried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnmetNeed {
    /// The required features the adapter lacks.
    FeaturesLacking(wgpu::Features),
    /// A limit the adapter cannot meet, named as wgpu names it; `required`
    /// and `allowed` as the limit reads (some are better high, some low).
    LimitUnmet {
        limit: &'static str,
        needs: u64,
        offers: u64,
    },
}

impl std::fmt::Display for UnmetNeed {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LimitUnmet { limit, needs, offers } => {
                write!(out, "limit {limit}: required {needs}, adapter allows {offers}")
            }
            Self::FeaturesLacking(missing) => write!(out, "missing features: {missing:?}"),
        }
    }
}

/// What the request needs against what the adapter offers, checked as
/// `request_device` checks it; `None` when all is met. Features first;
/// of the limits only the first one short is named, which is enough to
/// explain a skip.
pub fn unmet_need(
    wanted_features: wgpu::Features,
    wanted_limits: &wgpu::Limits,
    offered_features: wgpu::Features,
    offered_limits: &wgpu::Limits,
) -> Option<UnmetNeed> {
    // Required but not offered; nothing here means every feature is met.
    let missing = wanted_features - offered_features;
    if !missing.is_empty() {
        return Some(UnmetNeed::FeaturesLacking(missing));
    }
    let mut first = None;
    // `true`: stop at the first limit out of range.
    let note = |name: &'static str, required: u64, allowed: u64| {
        first = Some(UnmetNeed::LimitUnmet { limit: name, needs: required, offers: allowed });
    };
    wanted_limits.check_limits_with_fail_fn(offered_limits, true, note);
    first
}

/// The limits to ask an adapter for: `base`, with the buffer and texture
/// size limits the adapter's own, so large models and views can use all
/// the adapter allows.
pub fn request_limits_for(base: &wgpu::Limits, adapter: &wgpu::Limits) -> wgpu::Limits {
    let mut limits = base.clone();
    limits.max_texture_dimension_2d = adapter.max_texture_dimension_2d;
    limits.max_buffer_size = adapter.max_buffer_size;
    limits
}

/// Opens one candidate: once, and if `retry_eligible` once more after
/// `delay`. A success on the second try carries the first try's error,
/// so the refusal it got past is not lost; a failure returns every error
/// in order. `sleep` is passed in so tests need not wait.
fn open_with_second_try<T, E>(
    worth_second_try: bool,
    wait: Duration,
    pause: impl FnOnce(Duration),
    mut attempt: impl FnMut() -> Result<T, E>,
) -> Result<(T, Option<E>), Vec<E>> {
    match attempt() {
        Ok(value) => Ok((value, None)),
        Err(refusal) if !worth_second_try => Err(vec![refusal]),
        Err(refusal) => {
            pause(wait);
            match attempt() {
                Ok(value) => Ok((value, Some(refusal))),
                Err(again) => Err(vec![refusal, again]),
            }
        }
    }
}

/// An override value, trimmed; blank counts as unset, so clearing the
/// variable with an empty string does not match every adapter.
fn clean_override(raw: Option<&str>) -> Option<&str> {
    Some(raw?.trim()).filter(|name| !name.is_empty())
}

/// The override from the environment; unset, unreadable or blank is none.
fn override_from_env() -> Option<String> {
    let raw = std::env::var(ADAPTER_OVERRIDE_VAR).ok();
    clean_override(raw.as_deref()).map(String::from)
}

/// Each refusing candidate's index with its error.
type RefusalList<E> = Vec<(usize, E)>;

/// Tries the candidates in `order` until one opens: its index and value,
/// with the refusals passed on the way, or every refusal in order when none
/// opens. Opening a device is where an adapter can first refuse the limits asked
/// for, so stopping at the first refusal could fail the launch while a
/// usable adapter went untried. The refusals are kept on success too: they
/// mean a better adapter was passed over, which should not go unrecorded.
fn open_in_order<T, E>(
    ranking: &[usize],
    mut attempt: impl FnMut(usize) -> Result<T, E>,
) -> Result<(usize, T, RefusalList<E>), RefusalList<E>> {
    let mut passed = Vec::new();
    for index in ranking.iter().copied() {
        match attempt(index) {
            Ok(value) => return Ok((index, value, passed)),
            Err(err) => passed.push((index, err)),
        }
    }
    Err(passed)
}

/// The refusals as one message, each under its adapter's name, so a bug
/// report says which hardware refused and why.
fn refusal_report<E: std::fmt::Display>(
    traits: &[AdapterTraits<'_>],
    refused: &[(usize, E)],
) -> String {
    let named = |(index, err): &(usize, E)| {
        let name = traits
            .get(*index)
            .map_or("unknown adapter", |facts| facts.adapter_label);
        format!("{name}: {err}")
    };
    refused.iter().map(named).collect::<Vec<_>>().join("; ")
}

/// A better-ranked adapter that refused: the shortfall that predicted it,
/// if any, and the error of each try (one, or two when retried).
#[derive(Debug)]
pub struct PassedOverAdapter {
    pub adapter_name: String,
    pub predicted: Option<UnmetNeed>,
    pub try_errors: Vec<wgpu::RequestDeviceError>,
}

/// The chosen adapter's own first refusal, when its second try worked: kept
/// so the launch still reports what it got past, the very trace a passing
/// failure leaves.
#[derive(Debug)]
pub struct FirstTryRefusal {
    pub refused_with: wgpu::RequestDeviceError,
}

/// The chosen adapter with its device and queue, the better adapters that
/// refused first, and the chosen one's recovered refusal. Any `skipped`
/// means this is not the adapter the order prefers, which the caller should
/// say rather than let pass.
#[derive(Debug)]
pub struct ChosenAdapter {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub passed_over: Vec<PassedOverAdapter>,
    pub first_try: Option<FirstTryRefusal>,
}

/// Opens a device on the best adapter of `backends` that accepts
/// `descriptor`, honouring [`ADAPTER_NAME_ENV`].
pub fn open_best_adapter(
    gpu: &wgpu::Instance,
    apis: wgpu::Backends,
    request: &wgpu::DeviceDescriptor<'_>,
) -> anyhow::Result<ChosenAdapter> {
    let preferred = override_from_env();
    open_best_adapter_named(gpu, apis, request, preferred.as_deref())
}

/// [`select_adapter_and_device`] with the override given rather than read
/// from the environment, so tests need not change state the whole test
/// binary shares.
pub fn open_best_adapter_named(
    gpu: &wgpu::Instance,
    apis: wgpu::Backends,
    request_template: &wgpu::DeviceDescriptor<'_>,
    wanted_name: Option<&str>,
) -> anyhow::Result<ChosenAdapter> {
    let found = pollster::block_on(gpu.enumerate_adapters(apis));
    anyhow::ensure!(
        !found.is_empty(),
        "No graphics adapter is available for apis {apis:?}"
    );
    let infos: Vec<wgpu::AdapterInfo> = found.iter().map(wgpu::Adapter::get_info).collect();
    let candidates: Vec<AdapterTraits<'_>> = infos
        .iter()
        .map(|info| AdapterTraits { adapter_label: &info.name, kind: info.device_type })
        .collect();
    // Each adapter's own request, so the prediction and the request see
    // the same limits; predicted before any request is made.
    let mut requests = Vec::with_capacity(found.len());
    let mut shortfalls = Vec::with_capacity(found.len());
    for adapter in &found {
        let offered = adapter.limits();
        let mut request = request_template.clone();
        request.required_limits = request_limits_for(&request_template.required_limits, &offered);
        let short = unmet_need(
            request.required_features,
            &request.required_limits,
            adapter.features(),
            &offered,
        );
        requests.push(request);
        shortfalls.push(short);
    }
    let order = adapter_preference_order(&candidates, wanted_name);
    let try_adapter = |index: usize| {
        let request = || pollster::block_on(found[index].request_device(&requests[index]));
        let worth_a_retry = shortfalls[index].is_none();
        open_with_second_try(worth_a_retry, SECOND_TRY_WAIT, std::thread::sleep, request)
    };
    let opened = open_in_order(&order, try_adapter);
    match opened {
        Ok((index, ((device, queue), recovered), refusals)) => {
            let skipped = refusals
                .into_iter()
                .map(|(refused, errors)| {
                    let name = infos[refused].name.clone();
                    let shortfall = shortfalls[refused].clone();
                    PassedOverAdapter { adapter_name: name, predicted: shortfall, try_errors: errors }
                })
                .collect();
            let recovery = recovered.map(|error| FirstTryRefusal { refused_with: error });
            Ok(ChosenAdapter {
                adapter: found[index].clone(),
                device,
                queue,
                passed_over: skipped,
                first_try: recovery,
            })
        }
        Err(refused_all) => {
            // One entry per adapter, its tries joined in order.
            let mut tries = Vec::with_capacity(refused_all.len());
            for (index, errors) in refused_all {
                let each: Vec<String> = errors.iter().map(ToString::to_string).collect();
                tries.push((index, each.join("; on retry: ")));
            }
            let tried = refusal_report(&candidates, &tries);
            Err(anyhow::anyhow!("No graphics adapter accepted a device request. Tried: {tried}"))
        }
    }
}

#[cfg(test)]
#[path = "tests_graphics_adapter.rs"]
mod tests;
