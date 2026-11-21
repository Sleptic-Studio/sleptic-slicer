// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! The 3MF model part (`3dmodel.model`) as data, read from and written to
//! XML. One module per part of the specification: the core document
//! (units, objects, meshes, components, the build), the materials
//! extension (only what a resin print has to read past) and the slice
//! extension.
use crate::error::{ModelParseError, ModelWriteError};
use crate::names::*;
use std::io::Write;

/// An enum whose values are the specification's keywords. Each variant is
/// listed once beside its keyword; reading and writing both use that list.
macro_rules! spec_keywords {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident = $keyword:literal,)+ }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
        pub enum $name {
            $($(#[$vmeta])* $variant,)+
        }

        impl $name {
            const KEYWORDS: &'static [($name, &'static str)] = &[$(($name::$variant, $keyword),)+];

            /// The keyword the specification writes for this value.
            pub fn as_str(self) -> &'static str {
                $name::KEYWORDS.iter().find(|entry| entry.0 == self).map_or("", |entry| entry.1)
            }

            /// The value `keyword` names, if it is one of the listed keywords.
            pub fn parse(keyword: &str) -> Option<$name> {
                $name::KEYWORDS.iter().find(|entry| entry.1 == keyword).map(|entry| entry.0)
            }
        }
    };
}

/// A number the specification keeps inside the non-negative `i32` range,
/// from `lowest` up.
macro_rules! spec_number {
    ($(#[$meta:meta])* $name:ident from $lowest:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u32);

        impl $name {
            pub const fn new(value: u32) -> Option<$name> {
                // Below `lowest` wraps round past the top, so one comparison does.
                match value.wrapping_sub($lowest) <= i32::MAX as u32 - $lowest {
                    true => Some($name(value)),
                    false => None,
                }
            }

            pub const fn get(self) -> u32 {
                self.0
            }
        }
    };
}

mod document;
mod read;
mod slices;
mod vocabulary;
mod write;

pub use document::*;
pub use read::parse_model;
pub use slices::*;
pub use vocabulary::*;
pub use write::write_model;
