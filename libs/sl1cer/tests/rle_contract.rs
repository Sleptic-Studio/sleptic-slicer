// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use sc_sl1cer::raster::rle::{LayerRaster, RleRun, RleRunAccumulator};

#[test]
fn additions_merge_only_equal_neighbors_and_zero_lengths_are_invisible() {
    let mut accumulator = RleRunAccumulator::new();
    assert!(accumulator.is_empty());
    assert_eq!(accumulator.len(), 0);
    let additions = [
        (0, 0),
        (10, 2),
        (10, 0),
        (10, 3),
        (0, 1),
        (10, 2),
        (10, 1),
        (255, 0),
        (255, 4),
    ];
    for ((value, length), count) in additions.into_iter().zip([0, 1, 1, 1, 2, 3, 3, 3, 4]) {
        accumulator.push(value, length);
        assert_eq!(accumulator.len(), count);
        assert_eq!(accumulator.is_empty(), count == 0);
    }
    let runs = accumulator.into_runs();
    let actual = runs
        .iter()
        .map(|run| (run.value, run.length))
        .collect::<Vec<_>>();
    assert_eq!(actual, [(10, 5), (0, 1), (10, 3), (255, 4)]);
    assert!(RleRunAccumulator::default().into_runs().is_empty());
}

#[test]
fn bounded_pushes_preserve_every_pixel_and_the_full_unsigned_length() {
    let mut accumulator = RleRunAccumulator::new();
    let mut pixels = Vec::new();
    for index in 0..513u32 {
        let value = ((index / 3) % 4) as u8 * 85;
        let length = index % 7;
        accumulator.push(value, length);
        pixels.extend(std::iter::repeat_n(value, length as usize));
    }
    let runs = accumulator.into_runs();
    assert!(runs.iter().all(|run| run.length > 0));
    assert!(runs.windows(2).all(|pair| pair[0].value != pair[1].value));
    let decoded = runs
        .iter()
        .flat_map(|run| std::iter::repeat_n(run.value, run.length as usize))
        .collect::<Vec<_>>();
    assert_eq!(decoded, pixels);
    let mut full = RleRunAccumulator::new();
    full.push(17, 1);
    full.push(17, u32::MAX - 1);
    full.push(17, 0);
    full.push(18, 1);
    let actual = full
        .into_runs()
        .into_iter()
        .map(|run| (run.value, run.length))
        .collect::<Vec<_>>();
    assert_eq!(actual, [(17, u32::MAX), (18, 1)]);
}

#[test]
fn raster_storage_keeps_float_bits_dimensions_and_unvalidated_run_data() {
    for (z, width, height) in [
        (-0.0, 3, 7),
        (f64::from_bits(0x7ff8_0000_0000_0042), 0, 9),
        (42.25, u32::MAX, 0),
    ] {
        let runs = vec![
            RleRun {
                value: 9,
                length: 0,
            },
            RleRun {
                value: 5,
                length: 2,
            },
        ];
        let raster = LayerRaster::new(z, width, height, runs);
        assert_eq!(raster.z_mm().to_bits(), z.to_bits());
        assert_eq!(raster.width(), width);
        assert_eq!(raster.height(), height);
        assert_eq!(
            raster
                .runs()
                .iter()
                .map(|run| (run.value, run.length))
                .collect::<Vec<_>>(),
            [(9, 0), (5, 2)]
        );
    }
}

#[test]
fn overflow_and_zero_pushes_preserve_run_boundaries() {
    let mut runs = RleRunAccumulator::new();
    runs.push(4, u32::MAX - 2);
    runs.push(4, 5);
    runs.push(9, 0);
    runs.push(4, 1);
    runs.push(9, 2);
    assert_eq!(
        runs.into_runs(),
        vec![
            RleRun {
                value: 4,
                length: u32::MAX
            },
            RleRun {
                value: 4,
                length: 4
            },
            RleRun {
                value: 9,
                length: 2
            },
        ]
    );
}
