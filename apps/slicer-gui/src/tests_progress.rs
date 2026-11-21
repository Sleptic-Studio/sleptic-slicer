// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

use crate::{export_progress as export, load_progress as load, save_progress as save};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

#[test]
fn fractions_truncate_clamp_and_handle_full_width_counters() {
    for total in 1..=127_u64 {
        for done in 0..=total + 1 {
            let expected = ((done.min(total) * 1000) / total) as u32;
            assert_eq!(load::per_mille_of(done, total), expected);
        }
    }
    for (done, total, expected) in [
        (0, 0, 1000),
        (u64::MAX, 0, 1000),
        (0, u64::MAX, 0),
        (u64::MAX / 2, u64::MAX, 499),
        (u64::MAX - 1, u64::MAX, 999),
        (u64::MAX, u64::MAX, 1000),
        (u64::MAX, 1, 1000),
        (1, 3, 333),
    ] {
        assert_eq!(load::per_mille_of(done, total), expected);
    }
    assert_eq!(load::PROGRESS_PERMILLE_MAX, 1000);
}

#[test]
fn every_phase_tag_decodes_with_raw_fraction_and_last_phase_fallback() {
    for tag in 0..=u16::MAX as u32 {
        for fraction in [0, 1, 999, 1000, 1001, 65_535] {
            let word = tag * 65_536 + fraction;
            let (phase, value) = load::decode(word);
            assert_eq!((phase as u32, value), (tag.min(2), fraction));
            let (phase, value) = save::decode(word);
            assert_eq!((phase as u32, value), (tag.min(2), fraction));
            let (phase, value) = export::decode(word);
            assert_eq!((phase as u32, value), (tag.min(3), fraction));
        }
    }
}

macro_rules! check_sink {
    ($name:ident, $module:ident, $sink:ident, [$($phase:path),+]) => {
        #[test]
        fn $name() {
            let word = Arc::new(AtomicU32::new(37));
            let sink = $module::$sink::new(word.clone());
            assert_eq!(word.load(Ordering::Relaxed), 37, "construction must preserve state");
            let phases = [$($phase),+];
            for (ordinal, phase) in phases.iter().copied().enumerate() {
                for fraction in 0..=1000_u32 {
                    sink.reset();
                    sink.set(phase, u64::from(fraction), 1000);
                    assert_eq!(word.load(Ordering::Relaxed), (ordinal as u32) * 65_536 + fraction);
                    assert_eq!($module::decode(word.load(Ordering::Relaxed)), (phase, fraction));
                }
                sink.reset();
                sink.set(phase, 0, 0);
                assert_eq!($module::decode(word.load(Ordering::Relaxed)), (phase, 1000));
                sink.reset();
                sink.set(phase, u64::MAX, 1);
                assert_eq!($module::decode(word.load(Ordering::Relaxed)), (phase, 1000));
            }
            sink.reset();
            let cloned = sink.clone();
            for phase in phases.iter().copied() {
                cloned.set(phase, 0, 1000);
                assert_eq!($module::decode(word.load(Ordering::Relaxed)), (phase, 0));
                sink.set(phase, 900, 1000);
                cloned.set(phase, 100, 1000);
                assert_eq!($module::decode(word.load(Ordering::Relaxed)), (phase, 900));
                sink.set(phases[0], 1000, 1000);
                let expected = if phase == phases[0] { 1000 } else { 900 };
                assert_eq!($module::decode(word.load(Ordering::Relaxed)), (phase, expected));
            }
            cloned.reset();
            assert_eq!($module::decode(word.load(Ordering::Relaxed)), (phases[0], 0));
            std::thread::scope(|scope| {
                for phase in phases.iter().copied() {
                    let shared = &sink;
                    scope.spawn(move || {
                        for fraction in (0..=1000).rev() {
                            shared.set(phase, fraction, 1000);
                        }
                    });
                }
            });
            assert_eq!($module::decode(word.load(Ordering::Relaxed)), (*phases.last().unwrap(), 1000));
            sink.reset();
            assert_eq!(word.load(Ordering::Relaxed), 0);
        }
    };
}

check_sink!(
    load_publication,
    load,
    LoadProgressSink,
    [
        load::OpenStep::Parse,
        load::OpenStep::Priming,
        load::OpenStep::GpuUpload
    ]
);
check_sink!(
    save_publication,
    save,
    SaveProgressSink,
    [
        save::WriteStep::Serialize,
        save::WriteStep::Compress,
        save::WriteStep::Writing
    ]
);
check_sink!(
    export_publication,
    export,
    ExportProgressSink,
    [
        export::ExportPhase::Flatten,
        export::ExportPhase::Serialize,
        export::ExportPhase::Compress,
        export::ExportPhase::Write
    ]
);
