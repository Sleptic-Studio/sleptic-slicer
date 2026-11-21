// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Ranges with a strictly smaller lower end than upper end, read from
//! profiles as `{ min, max }` and refused if the ends are the wrong way
//! round.
use crate::{PositiveFiniteMm, PositiveFiniteMmPerMin};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

/// The two ends as a profile has them, before the order is checked.
#[derive(Deserialize)]
struct Ends<T> {
    min: T,
    max: T,
}

/// A `min < max` range of `$bound`, its error, and the error's message
/// (given the two ends in that order).
macro_rules! ordered_range {
    ($(#[$doc:meta])* $name:ident read as $ends:literal of $bound:ty, $error:ident: $message:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
        #[serde(try_from = $ends)]
        pub struct $name {
            min: $bound,
            max: $bound,
        }

        /// Ends that are equal or the wrong way round.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct $error {
            pub min: f64,
            pub max: f64,
        }

        impl fmt::Display for $error {
            fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(out, $message, self.min, self.max)
            }
        }

        impl Error for $error {}

        impl $name {
            pub fn new(min: $bound, max: $bound) -> Result<$name, $error> {
                let (low, high) = (min.value(), max.value());
                match low < high {
                    true => Ok($name { min, max }),
                    false => Err($error { min: low, max: high }),
                }
            }

            pub fn min(self) -> $bound {
                self.min
            }

            pub fn max(self) -> $bound {
                self.max
            }
        }

        impl TryFrom<Ends<$bound>> for $name {
            type Error = $error;

            fn try_from(ends: Ends<$bound>) -> Result<$name, $error> {
                $name::new(ends.min, ends.max)
            }
        }
    };
}

ordered_range!(
    /// A span of lengths, such as a resin's usable layer heights.
    LengthRange read as "Ends<PositiveFiniteMm>" of PositiveFiniteMm,
    LengthRangeBoundsInverted: "length range min ({} mm) is not strictly less than max ({} mm)"
);

ordered_range!(
    /// A span of speeds, such as a printer's lift speeds.
    SpeedRangeMmPerMin read as "Ends<PositiveFiniteMmPerMin>" of PositiveFiniteMmPerMin,
    SpeedRangeMmPerMinBoundsInverted: "speed range min ({} mm/min) is not strictly less than max ({} mm/min)"
);
