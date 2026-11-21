// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Adapter selection against the machine's real adapters: that they are
//! found, that the order holds with a real driver, that the device handed
//! back works, and that the override reaches the adapter it names. The
//! order itself is tested without a GPU inside the module.
//! Every wgpu call here runs while holding `ONE_AT_A_TIME`, and nothing
//! from the GPU outlives its turn, as in `integration_render.rs`: making an
//! instance on one thread while another has work in flight has crashed
//! the process inside the driver loader. Without a usable adapter the
//! tests skip, so a machine with no GPU and no software renderer does not
//! fail the suite.
use std::sync::{Mutex, MutexGuard, PoisonError};

use sc_slicer_gui::graphics_adapter::{AdapterTraits, ChosenAdapter, adapter_preference_order};
use sc_slicer_gui::graphics_adapter::{open_best_adapter, open_best_adapter_named};
use slint::wgpu_29::wgpu;

/// The one backend Slint's Skia renderer takes on this platform, as
/// `run()` picks it; the app never uses another.
const PLATFORM_BACKEND: wgpu::Backends = if cfg!(target_vendor = "apple") {
    wgpu::Backends::METAL
} else if cfg!(target_family = "windows") {
    wgpu::Backends::DX12
} else {
    wgpu::Backends::VULKAN
};

/// Held while a test talks to wgpu.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn take_gpu_turn() -> MutexGuard<'static, ()> {
    // A failed test poisons the turn; the rest still need theirs.
    ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The device request `run()` makes (all defaults), which is what an
/// adapter may refuse.
fn app_request(label: &str) -> wgpu::DeviceDescriptor<'_> {
    wgpu::DeviceDescriptor { label: Some(label), ..Default::default() }
}

fn instance_for(backends: wgpu::Backends) -> wgpu::Instance {
    let mut options = wgpu::InstanceDescriptor::new_without_display_handle();
    options.backends = backends;
    wgpu::Instance::new(options)
}

/// What a test gets on its turn: the instance, its adapters and their info,
/// in the order wgpu listed them.
struct Machine {
    gpu: wgpu::Instance,
    adapters: Vec<wgpu::Adapter>,
    infos: Vec<wgpu::AdapterInfo>,
}

/// Runs `test` on its turn with the platform backend's adapters; skips when
/// there are none.
fn with_machine(test: impl FnOnce(&Machine)) {
    let _turn = take_gpu_turn();
    let gpu = instance_for(PLATFORM_BACKEND);
    let adapters = pollster::block_on(gpu.enumerate_adapters(PLATFORM_BACKEND));
    if adapters.is_empty() {
        eprintln!("skipping: no wgpu adapter available in this environment");
        return;
    }
    let infos: Vec<_> = adapters.iter().map(wgpu::Adapter::get_info).collect();
    eprintln!("adapter selection sees {} adapter(s):", infos.len());
    for about in &infos {
        eprintln!("  {} ({:?}) on {:?}", about.name, about.device_type, about.backend);
    }
    test(&Machine { gpu, adapters, infos });
}

/// Linux's runtime power state of a PCI device, read to explain a refused
/// request: a sleeping laptop GPU that reads `suspended` or `resuming` was
/// waking up, not rejecting the request. None elsewhere or when unreadable.
fn pci_power_state(bus_id: &str) -> Option<String> {
    let readable = cfg!(target_os = "linux") && !bus_id.is_empty();
    let file = format!("/sys/bus/pci/devices/{bus_id}/power/runtime_status");
    readable.then(|| std::fs::read_to_string(file).ok()).flatten().map(|s| s.trim().to_string())
}

#[test]
fn no_power_state_for_a_device_that_cannot_be_read() {
    assert_eq!(pci_power_state(""), None);
    assert_eq!(pci_power_state("no-such-pci-device"), None);
}

/// Whether `adapter` gives a device for the app's request (the device is
/// drained and dropped straight away). A refusal is logged with the power
/// state before and at it: this probe has failed for a moment on hardware
/// that took a device just after, and those are the only clues.
fn accepts_app_request(adapter: &wgpu::Adapter) -> bool {
    let about = adapter.get_info();
    let power_before = pci_power_state(&about.device_pci_bus_id);
    let refusal = match pollster::block_on(adapter.request_device(&app_request("graphics-adapter probe device"))) {
        Ok((device, _queue)) => {
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            return true;
        }
        Err(refusal) => refusal,
    };
    let power_at = pci_power_state(&about.device_pci_bus_id);
    let shown = |state: Option<String>| state.unwrap_or_else(|| String::from("unknown"));
    eprintln!(
        "  probe: {} refused a device request: {refusal} (power state before: {}, at refusal: {})",
        about.name,
        shown(power_before),
        shown(power_at)
    );
    false
}

/// The module's order for this machine's adapters.
fn preference(infos: &[wgpu::AdapterInfo], wanted: Option<&str>) -> Vec<usize> {
    let traits: Vec<_> =
        infos.iter().map(|about| AdapterTraits { adapter_label: &about.name, kind: about.device_type }).collect();
    adapter_preference_order(&traits, wanted)
}

/// The answer worked out independently: the first adapter in order that
/// accepts the request.
fn first_that_opens(machine: &Machine) -> Option<wgpu::AdapterInfo> {
    let first = preference(&machine.infos, None).into_iter().find(|&i| accepts_app_request(&machine.adapters[i]))?;
    Some(machine.infos[first].clone())
}

/// Runs the selection with `wanted`, drains its device and says which
/// adapter it took.
fn select(machine: &Machine, label: &str, wanted: Option<&str>) -> wgpu::AdapterInfo {
    let picked = open_best_adapter_named(&machine.gpu, PLATFORM_BACKEND, &app_request(label), wanted)
        .expect("selection must succeed");
    let _ = picked.device.poll(wgpu::PollType::wait_indefinitely());
    picked.adapter.get_info()
}

#[test]
fn the_first_adapter_that_opens_wins_and_an_unknown_override_changes_nothing() {
    with_machine(|machine| {
        let Some(answer) = first_that_opens(machine) else {
            return eprintln!("skipping: no adapter accepts the application device profile");
        };
        let picked = open_best_adapter(&machine.gpu, PLATFORM_BACKEND, &app_request("graphics-adapter selection test"))
            .expect("an adapter accepts the request");
        let _ = picked.device.poll(wgpu::PollType::wait_indefinitely());
        assert_eq!(picked.adapter.get_info(), answer);
        // An override naming no adapter (stale or misspelt) must not stop the app.
        let with_stale_override =
            select(machine, "graphics-adapter unmatched-override test", Some("no adapter is called this"));
        assert_eq!(with_stale_override, answer);
    });
}

#[test]
fn hardware_beats_software_and_discrete_beats_the_rest() {
    with_machine(|machine| {
        use wgpu::DeviceType::{DiscreteGpu, IntegratedGpu};
        let any_usable = |kinds: &[wgpu::DeviceType]| {
            (0..machine.adapters.len())
                .any(|i| kinds.contains(&machine.infos[i].device_type) && accepts_app_request(&machine.adapters[i]))
        };
        if !any_usable(&[DiscreteGpu, IntegratedGpu]) {
            return eprintln!("skipping: this environment exposes no usable hardware GPU");
        }
        let discrete_usable = any_usable(&[DiscreteGpu]);
        let picked = select(machine, "graphics-adapter preference test", None);
        let what = &picked.name;
        assert_ne!(picked.device_type, wgpu::DeviceType::Cpu, "chose software {what} over a usable GPU");
        if discrete_usable {
            assert_eq!(picked.device_type, DiscreteGpu, "chose {what} over a usable discrete GPU");
        }
    });
}

#[test]
fn the_override_can_pick_the_adapter_ranked_last() {
    with_machine(|machine| {
        // The lowest-ranked adapter that still opens; only the override can
        // move it to the front.
        let ranking = preference(&machine.infos, None);
        let Some(&last) = ranking.iter().rev().find(|&&i| accepts_app_request(&machine.adapters[i])) else {
            return eprintln!("skipping: no adapter accepts the application device profile");
        };
        let name = machine.infos[last].name.as_str();
        assert_eq!(select(machine, "graphics-adapter override test", Some(name)).name, name);
    });
}

#[test]
fn the_selected_device_runs_a_submission_with_the_adapters_size_limits() {
    with_machine(|machine| {
        let request = app_request("graphics-adapter usability test");
        let Ok(ChosenAdapter { adapter, device, queue, .. }) =
            open_best_adapter(&machine.gpu, PLATFORM_BACKEND, &request)
        else {
            return eprintln!("skipping: no adapter accepts the application device profile");
        };
        // An order that is right but hands back a device unable to run work
        // would still leave the view blank, which is what selection is for.
        let scratch = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("graphics-adapter usability buffer"),
            size: 256,
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&scratch, 0, &[7; 256]);
        let commands = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        queue.submit([commands.finish()]);
        device.poll(wgpu::PollType::wait_indefinitely()).expect("the device completes a submission");
        // Buffer and texture size come from the adapter, all else from the defaults.
        let adapter_limits = adapter.limits();
        let mut wanted = wgpu::Limits::default();
        wanted.max_buffer_size = adapter_limits.max_buffer_size;
        wanted.max_texture_dimension_2d = adapter_limits.max_texture_dimension_2d;
        assert_eq!(device.limits(), wanted);
    });
}

#[test]
fn selection_with_no_adapters_fails() {
    let _turn = take_gpu_turn();
    // No backends at all lists no adapter on any machine.
    let nothing = wgpu::Backends::empty();
    let failure = open_best_adapter(&instance_for(nothing), nothing, &app_request("graphics-adapter empty test"))
        .expect_err("an empty backend set has no adapter");
    let text = failure.to_string();
    assert!(text.contains("No graphics adapter is available"), "{text}");
}
