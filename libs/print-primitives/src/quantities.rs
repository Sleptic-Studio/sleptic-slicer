// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Single numbers with a unit and a valid range, checked once when made
//! (or read from a profile) so nothing downstream has to check again.
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt, str::FromStr};

/// A number refused by a quantity's range. Each quantity has its own,
/// so the message can name the unit.
macro_rules! range_error {
    ($(#[$doc:meta])* $name:ident: $message:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct $name {
            pub value: f64,
        }

        impl fmt::Display for $name {
            fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(out, $message, self.value)
            }
        }

        impl Error for $name {}
    };
}

/// An `f64` quantity accepted only when `$accepts` holds. It reads from
/// and writes to profiles as a bare number, and prints as one.
macro_rules! quantity {
    ($(#[$doc:meta])* $name:ident, $error:ty, |$value:ident| $accepts:expr, $refuse:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
        #[serde(try_from = "f64", into = "f64")]
        pub struct $name(f64);

        impl $name {
            pub fn new($value: f64) -> Result<$name, $error> {
                match $accepts {
                    true => Ok($name($value)),
                    false => Err($refuse),
                }
            }

            pub fn value(self) -> f64 {
                self.0
            }
        }

        impl TryFrom<f64> for $name {
            type Error = $error;

            fn try_from(value: f64) -> Result<$name, $error> {
                $name::new(value)
            }
        }

        impl From<$name> for f64 {
            fn from(quantity: $name) -> f64 {
                quantity.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, out)
            }
        }
    };
}

/// A quantity that must be finite and above zero.
macro_rules! positive {
    ($(#[$doc:meta])* $name:ident, $error:ident) => {
        quantity!($(#[$doc])* $name, $error, |value| value.is_finite() && value > 0.0, $error { value });
    };
}

range_error!(
    /// A length that is zero, negative or not finite.
    LengthOutOfRange: "length must be strictly positive and finite, got {}"
);
range_error!(SecOutOfRange: "duration must be strictly positive and finite, got {} s");
range_error!(SpeedMmPerMinOutOfRange: "speed must be strictly positive and finite, got {} mm/min");
range_error!(MjPerCm2OutOfRange: "energy density must be strictly positive and finite, got {} mJ/cm^2");
range_error!(MwPerCm2OutOfRange: "light intensity must be strictly positive and finite, got {} mW/cm^2");
range_error!(CurrencyPerLiterOutOfRange: "price must be strictly positive and finite, got {} per litre");

positive!(
    /// Millimetres.
    PositiveFiniteMm, LengthOutOfRange
);
positive!(
    /// Microns (pixel pitch).
    PositiveFiniteMicrons, LengthOutOfRange
);
positive!(
    /// Seconds.
    PositiveFiniteSec, SecOutOfRange
);
positive!(
    /// Millimetres per minute.
    PositiveFiniteMmPerMin, SpeedMmPerMinOutOfRange
);
positive!(
    /// An exposure dose, mJ/cm².
    PositiveFiniteMjPerCm2, MjPerCm2OutOfRange
);
positive!(
    /// A light source's intensity, mW/cm².
    PositiveFiniteMwPerCm2, MwPerCm2OutOfRange
);
positive!(
    /// A resin's price per litre, in whatever currency the user keeps.
    PositiveFiniteCurrencyPerLiter, CurrencyPerLiterOutOfRange
);

// A finite number always equals itself, so lengths can be keys.
impl Eq for PositiveFiniteMm {}
impl Eq for PositiveFiniteMicrons {}

/// How far a part may be scaled to make up for resin shrinking, in percent.
const SCALE_LIMIT_PERCENT: f64 = 10.0;

/// A scale correction outside ±[`SCALE_LIMIT_PERCENT`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaleCompensationPercentOutOfBound {
    pub value: f64,
    pub min: f64,
    pub max: f64,
}

impl fmt::Display for ScaleCompensationPercentOutOfBound {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ScaleCompensationPercentOutOfBound { value, min, max } = self;
        write!(out, "scale compensation must be finite and in [{min}%, {max}%], got {value}%")
    }
}

impl Error for ScaleCompensationPercentOutOfBound {}

quantity!(
    /// A shrink correction for one axis, in percent either way.
    ScaleCompensationPercent,
    ScaleCompensationPercentOutOfBound,
    |value| value.is_finite() && value.abs() <= SCALE_LIMIT_PERCENT,
    ScaleCompensationPercentOutOfBound { value, min: -SCALE_LIMIT_PERCENT, max: SCALE_LIMIT_PERCENT }
);

/// The shrink corrections for the three axes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScaleCompensation {
    pub x: ScaleCompensationPercent,
    pub y: ScaleCompensationPercent,
    pub z: ScaleCompensationPercent,
}

/// A length typed as text (command line, settings field) that is not a
/// number, or not a valid length.
#[derive(Debug)]
pub enum ParseLengthError {
    NotANumber(std::num::ParseFloatError),
    OutOfRange(LengthOutOfRange),
}

impl fmt::Display for ParseLengthError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseLengthError::NotANumber(cause) => write!(out, "invalid length value: {cause}"),
            ParseLengthError::OutOfRange(cause) => cause.fmt(out),
        }
    }
}

impl Error for ParseLengthError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ParseLengthError::NotANumber(cause) => Some(cause),
            ParseLengthError::OutOfRange(cause) => Some(cause),
        }
    }
}

impl FromStr for PositiveFiniteMm {
    type Err = ParseLengthError;

    fn from_str(text: &str) -> Result<PositiveFiniteMm, ParseLengthError> {
        let number = text.parse::<f64>().map_err(ParseLengthError::NotANumber)?;
        PositiveFiniteMm::new(number).map_err(ParseLengthError::OutOfRange)
    }
}

impl FromStr for PositiveFiniteMicrons {
    type Err = ParseLengthError;

    fn from_str(text: &str) -> Result<PositiveFiniteMicrons, ParseLengthError> {
        let number = text.parse::<f64>().map_err(ParseLengthError::NotANumber)?;
        PositiveFiniteMicrons::new(number).map_err(ParseLengthError::OutOfRange)
    }
}
