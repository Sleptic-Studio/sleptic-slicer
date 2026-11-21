// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use wgpu::{DeviceType, Features, Limits};

use super::*;

fn adapter(name: &str, device_type: DeviceType) -> AdapterTraits<'_> {
    AdapterTraits { adapter_label: name, kind: device_type }
}

fn names<'a>(candidates: &[AdapterTraits<'a>], preferred: Option<&str>) -> Vec<&'a str> {
    adapter_preference_order(candidates, preferred)
        .into_iter()
        .map(|i| candidates[i].adapter_label)
        .collect()
}

#[test]
fn real_gpus_come_first_discrete_before_integrated_software_last() {
    assert!(adapter_preference_order(&[], None).is_empty());
    let all = [
        adapter("cpu", DeviceType::Cpu),
        adapter("other", DeviceType::Other),
        adapter("igpu", DeviceType::IntegratedGpu),
        adapter("virtual", DeviceType::VirtualGpu),
        adapter("dgpu", DeviceType::DiscreteGpu),
    ];
    assert_eq!(
        names(&all, None),
        ["dgpu", "igpu", "virtual", "other", "cpu"]
    );
    assert_eq!(names(&all[..1], None), ["cpu"]);
    // Equals keep the order they were listed in.
    let twins = [
        adapter("b", DeviceType::IntegratedGpu),
        adapter("a", DeviceType::IntegratedGpu),
        adapter("c", DeviceType::Cpu),
    ];
    assert_eq!(names(&twins, None), ["b", "a", "c"]);
}

#[test]
fn a_named_adapter_comes_first_and_an_unknown_name_changes_nothing() {
    let all = [
        adapter("Microsoft Basic Render Driver", DeviceType::Cpu),
        adapter("Intel(R) UHD Graphics", DeviceType::IntegratedGpu),
        adapter("NVIDIA GeForce RTX 4070 Ti", DeviceType::DiscreteGpu),
        adapter("NVIDIA Quadro", DeviceType::IntegratedGpu),
    ];
    assert_eq!(
        names(&all, Some("basic render"))[0],
        "Microsoft Basic Render Driver"
    );
    // Several matches keep the usual order among themselves.
    assert_eq!(
        names(&all, Some("nvidia"))[..2],
        ["NVIDIA GeForce RTX 4070 Ti", "NVIDIA Quadro"]
    );
    assert_eq!(names(&all, Some("radeon")), names(&all, None));
    assert!(name_contains("NVIDIA GeForce", "geforce") && name_contains("nvidia", "NVIDIA"));
    assert!(!name_contains("Intel", "intel uhd"));
    assert!(name_contains("anything", ""));
}

#[test]
fn a_blank_override_counts_as_unset() {
    assert_eq!(clean_override(None), None);
    assert_eq!(clean_override(Some("")), None);
    assert_eq!(clean_override(Some(" \t ")), None);
    assert_eq!(
        clean_override(Some("  nvidia ")),
        Some("nvidia")
    );
}

#[test]
fn the_request_takes_the_adapters_size_limits_and_the_rest_from_the_base() {
    let base = Limits {
        max_texture_dimension_1d: 4096,
        min_uniform_buffer_offset_alignment: 64,
        ..Limits::default()
    };
    let roomy = Limits {
        max_texture_dimension_1d: 1 << 20,
        max_texture_dimension_2d: 1 << 20,
        min_uniform_buffer_offset_alignment: 1,
        max_buffer_size: 1 << 40,
        ..Limits::default()
    };
    assert_eq!(
        request_limits_for(&base, &roomy),
        Limits {
            max_buffer_size: 1 << 40,
            max_texture_dimension_2d: 1 << 20,
            ..base
        }
    );
    // A smaller adapter is asked for no more than it has.
    for small in [
        Limits {
            max_buffer_size: 128 << 20,
            ..Limits::default()
        },
        Limits {
            max_texture_dimension_2d: 4096,
            ..Limits::default()
        },
    ] {
        let asked = request_limits_for(&Limits::default(), &small);
        assert_eq!(
            (asked.max_buffer_size, asked.max_texture_dimension_2d),
            (small.max_buffer_size, small.max_texture_dimension_2d)
        );
        assert_eq!(
            unmet_need(Features::empty(), &asked, Features::empty(), &small),
            None
        );
    }
}

#[test]
fn a_shortfall_names_missing_features_first_then_the_first_limit_short() {
    let none = Features::empty();
    assert_eq!(
        unmet_need(none, &Limits::default(), none, &Limits::default()),
        None
    );
    assert_eq!(
        unmet_need(
            none,
            &Limits::downlevel_defaults(),
            Features::all(),
            &Limits::default()
        ),
        None
    );
    let wanted = Features::DEPTH_CLIP_CONTROL | Features::TIMESTAMP_QUERY;
    let missing = unmet_need(
        wanted,
        &Limits::default(),
        Features::TIMESTAMP_QUERY,
        &Limits::default(),
    );
    assert_eq!(
        missing,
        Some(UnmetNeed::FeaturesLacking(
            Features::DEPTH_CLIP_CONTROL
        ))
    );
    // A limit best high, and one best low (a finer alignment than offered).
    let too_big = Limits {
        max_texture_dimension_2d: 1 << 20,
        ..Limits::default()
    };
    let short = unmet_need(none, &too_big, none, &Limits::default()).unwrap();
    let allowed = u64::from(Limits::default().max_texture_dimension_2d);
    assert_eq!(
        short,
        UnmetNeed::LimitUnmet {
            limit: "max_texture_dimension_2d",
            needs: 1 << 20,
            offers: allowed
        }
    );
    assert_eq!(
        short.to_string(),
        format!("limit max_texture_dimension_2d: required 1048576, adapter allows {allowed}")
    );
    let too_fine = Limits {
        min_uniform_buffer_offset_alignment: 1,
        ..Limits::default()
    };
    let allowed = u64::from(Limits::default().min_uniform_buffer_offset_alignment);
    assert_eq!(
        unmet_need(none, &too_fine, none, &Limits::default()),
        Some(UnmetNeed::LimitUnmet {
            limit: "min_uniform_buffer_offset_alignment",
            needs: 1,
            offers: allowed
        })
    );
    let both = unmet_need(
        Features::DEPTH_CLIP_CONTROL,
        &too_big,
        none,
        &Limits::default(),
    );
    assert!(matches!(
        both,
        Some(UnmetNeed::FeaturesLacking(_))
    ));
    assert!(both.unwrap().to_string().starts_with("missing features: "));
}

/// Runs `open_with_one_retry` over scripted results; returns its answer,
/// the tries made and the waits asked for.
fn retry<T: Clone>(
    eligible: bool,
    results: &[Result<T, &'static str>],
) -> (
    Result<(T, Option<&'static str>), Vec<&'static str>>,
    usize,
    Vec<Duration>,
) {
    let (mut tries, mut waits) = (0, Vec::new());
    let answer = open_with_second_try(
        eligible,
        Duration::from_millis(300),
        |wait| waits.push(wait),
        || {
            tries += 1;
            results[tries - 1].clone()
        },
    );
    (answer, tries, waits)
}

#[test]
fn a_refusal_is_tried_once_more_after_a_wait_unless_it_was_foreseen() {
    let wait = vec![Duration::from_millis(300)];
    assert_eq!(retry(true, &[Ok(1)]), (Ok((1, None)), 1, vec![]));
    // Got past on the second try: the first refusal is kept.
    assert_eq!(
        retry(true, &[Err("busy"), Ok(7)]),
        (Ok((7, Some("busy"))), 2, wait.clone())
    );
    assert_eq!(
        retry(true, &[Err("first"), Err("second")]),
        (Err::<(u8, _), _>(vec!["first", "second"]), 2, wait)
    );
    assert_eq!(
        retry::<u8>(false, &[Err("short")]),
        (Err(vec!["short"]), 1, vec![])
    );
}

#[test]
fn candidates_are_tried_in_order_until_one_opens() {
    let mut tried = Vec::new();
    let first = open_in_order(&[2, 0, 1], |i| {
        tried.push(i);
        Ok::<_, &str>(i * 10)
    });
    assert_eq!((first, tried), (Ok((2, 20, vec![])), vec![2]));
    let later = open_in_order(&[2, 0, 1], |i| if i == 1 { Ok("device") } else { Err(i) });
    assert_eq!(later, Ok((1, "device", vec![(2, 2), (0, 0)])));
    let none: Result<(usize, (), _), _> = open_in_order(&[1, 0], Err);
    assert_eq!(none, Err(vec![(1, 1), (0, 0)]));
    let empty: Result<(usize, (), RefusalList<()>), _> = open_in_order(&[], |_| Ok(()));
    assert_eq!(empty, Err(vec![]));
}

#[test]
fn failures_are_described_under_their_adapters_names() {
    let candidates = [
        adapter("Discrete Graphics", DeviceType::DiscreteGpu),
        adapter("Integrated Graphics", DeviceType::IntegratedGpu),
    ];
    assert_eq!(refusal_report::<&str>(&candidates, &[]), "");
    assert_eq!(
        refusal_report(
            &candidates,
            &[(0, "out of memory"), (1, "unsupported limits")]
        ),
        "Discrete Graphics: out of memory; Integrated Graphics: unsupported limits"
    );
    assert_eq!(
        refusal_report(&candidates, &[(7, "vanished")]),
        "unknown adapter: vanished"
    );
}
