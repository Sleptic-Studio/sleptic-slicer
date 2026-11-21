// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Keeping the render target within the largest texture the GPU allows.

/// The view's size, scaled down evenly when a side passes `max_side`, so
/// the longer side becomes exactly `max_side`. Scaled sides round down,
/// but never to zero unless they were zero.
pub(super) fn fit_texture_limit(width: u32, height: u32, max_side: u32) -> (u32, u32) {
    let longer = u64::from(width.max(height));
    if longer <= u64::from(max_side) {
        return (width, height);
    }
    // Wide arithmetic: side * max_side can pass u32, the quotient cannot.
    let scaled = |side: u32| -> u32 {
        if side == 0 {
            return 0;
        }
        let kept = u64::from(side) * u64::from(max_side) / longer;
        u32::try_from(kept).unwrap_or(max_side).max(1)
    };
    (scaled(width), scaled(height))
}

#[cfg(test)]
mod tests {
    use super::fit_texture_limit as fit;

    #[test]
    fn a_view_inside_the_limit_is_left_alone() {
        for (w, h) in [(0, 0), (1, 1), (640, 480), (4096, 4096), (4096, 7)] {
            assert_eq!(fit(w, h, 4096), (w, h));
        }
    }

    #[test]
    fn the_longer_side_lands_on_the_limit_and_the_other_follows() {
        assert_eq!(fit(6000, 3000, 4096), (4096, 2048));
        assert_eq!(fit(3000, 6000, 4096), (2048, 4096));
        // 700 * 512 / 2000 = 179.2, rounded down.
        assert_eq!(fit(2000, 700, 512), (512, 179));
        assert_eq!(fit(u32::MAX, u32::MAX, 16384), (16384, 16384));
    }

    #[test]
    fn a_sliver_keeps_a_pixel_and_nothing_stays_nothing() {
        assert_eq!(fit(100_000, 2, 1000), (1000, 1));
        assert_eq!(fit(5000, 0, 1000), (1000, 0));
        assert_eq!(fit(0, 3000, 1000), (0, 1000));
    }
}
