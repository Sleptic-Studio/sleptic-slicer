// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    #[default]
    Balanced,
    FewestSupports,
    Fastest,
    BestQuality,
    #[serde(rename = "most_per_plate", alias = "smallest_footprint")]
    SmallestFootprint,
}
impl Preset {
    pub fn weights(self) -> [f64; 4] {
        match self {
            Self::Balanced => [1., 1., 1., 0.02],
            Self::FewestSupports => [2.5, 0.6, 0.5, 0.01],
            Self::Fastest => [0.2, 4., 0.3, 0.01],
            Self::BestQuality => [0.7, 0.7, 4., 0.01],
            Self::SmallestFootprint => [0.25, 0.4, 0.3, 20.],
        }
    }
}
pub const ZERO_SUPPORT_HEAD_START_COPIES: usize = 10;
pub const FILL_PLATE_WEIGHTS: [f64; 4] = [2., 0.6, 1., 6.];
