// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Names and keywords: profile ids, required text, anti-aliasing levels,
//! and the output formats and network protocols a printer profile names.
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt, str::FromStr};

/// A text value checked by `$check` whenever one is made or read. It
/// reads and writes as a plain string.
macro_rules! checked_text {
    ($(#[$doc:meta])* $name:ident, $error:ty, $check:path $(, $extra:ident)*) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize $(, $extra)*)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(text: &str) -> Result<$name, $error> {
                $name::try_from(text.to_owned())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = $error;

            fn try_from(text: String) -> Result<$name, $error> {
                $check(&text).map(|()| $name(text))
            }
        }

        impl FromStr for $name {
            type Err = $error;

            fn from_str(text: &str) -> Result<$name, $error> {
                $name::new(text)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> String {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
                out.write_str(self.as_str())
            }
        }
    };
}

/// Why text is not a profile id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidProfileId {
    Empty,
    LeadingHyphen { value: String },
    InvalidCharAt { value: String, index: usize },
}

impl fmt::Display for InvalidProfileId {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvalidProfileId::Empty => out.write_str("profile id must not be empty"),
            InvalidProfileId::LeadingHyphen { value } => write!(out, "profile id {value:?} must not start with a hyphen"),
            InvalidProfileId::InvalidCharAt { value, index } => write!(
                out,
                "profile id {value:?} has an invalid character at byte {index}; allowed characters are [a-z0-9-]"
            ),
        }
    }
}

impl Error for InvalidProfileId {}

/// Profile ids are file names on every system: lower-case letters, digits
/// and hyphens, not starting with a hyphen.
fn profile_id(text: &str) -> Result<(), InvalidProfileId> {
    let allowed = |byte: u8| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-');
    match (text.as_bytes().first(), text.bytes().position(|byte| !allowed(byte))) {
        (None, _) => Err(InvalidProfileId::Empty),
        (Some(b'-'), _) => Err(InvalidProfileId::LeadingHyphen { value: text.to_owned() }),
        (_, Some(index)) => Err(InvalidProfileId::InvalidCharAt { value: text.to_owned(), index }),
        (_, None) => Ok(()),
    }
}

/// Empty text where some is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyStringError;

impl fmt::Display for EmptyStringError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str("value must not be empty")
    }
}

impl Error for EmptyStringError {}

fn some_text(text: &str) -> Result<(), EmptyStringError> {
    if text.is_empty() { Err(EmptyStringError) } else { Ok(()) }
}

checked_text!(
    /// A printer profile's id.
    PrinterId, InvalidProfileId, profile_id, PartialOrd, Ord
);
checked_text!(
    /// A resin profile's id.
    MaterialId, InvalidProfileId, profile_id, PartialOrd, Ord
);
checked_text!(
    /// Text that is not empty (names, versions).
    NonEmptyString, EmptyStringError, some_text
);

/// Anti-aliasing: off, or 2, 4 or 8 grey levels per edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AaLevel {
    Off,
    X2,
    X4,
    X8,
}

const AA_NAMES: [(AaLevel, &str); 4] = [(AaLevel::Off, "off"), (AaLevel::X2, "x2"), (AaLevel::X4, "x4"), (AaLevel::X8, "x8")];

/// A name that is no anti-aliasing level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownAaLevel {
    pub got: String,
}

impl fmt::Display for UnknownAaLevel {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = AA_NAMES.iter().map(|(_, name)| *name).collect();
        write!(out, "unknown AA level \"{}\"; expected one of: {}", self.got, names.join(", "))
    }
}

impl Error for UnknownAaLevel {}

impl FromStr for AaLevel {
    type Err = UnknownAaLevel;

    fn from_str(text: &str) -> Result<AaLevel, UnknownAaLevel> {
        let found = AA_NAMES.iter().find(|(_, name)| *name == text);
        found.map(|(level, _)| *level).ok_or_else(|| UnknownAaLevel { got: text.to_owned() })
    }
}

/// A keyword set that keeps words it does not know (a newer profile may
/// name a format this build lacks) instead of refusing them.
macro_rules! open_keywords {
    ($(#[$doc:meta])* $name:ident { $($variant:ident = $word:literal,)+ }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(from = "String", into = "String")]
        pub enum $name {
            $($variant,)+
            Unknown(String),
        }

        impl $name {
            pub fn from_canonical(word: &str) -> $name {
                $(if word == $word {
                    return $name::$variant;
                })+
                $name::Unknown(word.to_owned())
            }

            pub fn as_canonical(&self) -> &str {
                match self {
                    $name::Unknown(word) => word,
                    $($name::$variant => $word,)+
                }
            }
        }

        impl From<String> for $name {
            fn from(word: String) -> $name {
                $name::from_canonical(&word)
            }
        }

        impl From<$name> for String {
            fn from(keyword: $name) -> String {
                keyword.as_canonical().to_owned()
            }
        }
    };
}

open_keywords! {
    /// A sliced-file format a printer takes.
    FormatKind {
        Sl1 = "sl1",
        Ctb = "ctb",
        Goo = "goo",
        Pwmx = "pwmx",
        PngPerLayer = "png_per_layer",
        SvgPerLayer = "svg_per_layer",
        SvgCombined = "svg_combined",
    }
}

open_keywords! {
    /// How a printer is reached over the network.
    NetworkProtocol {
        ElegooCentauri = "elegoo_centauri",
        AnycubicHomie = "anycubic_homie",
        Moonraker = "moonraker",
        Sdcp = "sdcp",
    }
}

/// A format a printer takes, and the format version it expects if any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputFormatBinding {
    pub format: FormatKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<NonEmptyString>,
}

/// A printer's formats, at least one, preferred first.
pub type OutputFormatBindings = nonempty_collections::NEVec<OutputFormatBinding>;
