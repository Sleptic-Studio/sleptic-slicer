// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Persist converted records using the checked profile library writers.
use crate::documents::Document;
use sc_print_profiles::{
    save_material_profile_to_library, save_printer_image_to_library,
    save_printer_profile_to_library,
};
use std::{borrow::Cow, fmt, path::Path};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Totals {
    pub printers: usize,
    pub materials: usize,
    pub images: usize,
}

#[derive(Debug)]
pub struct SaveFailure {
    operation: &'static str,
    cause: Box<dyn std::error::Error + Send + Sync>,
}

impl SaveFailure {
    fn during(
        operation: &'static str,
        cause: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            operation,
            cause: Box::new(cause),
        }
    }
}

impl fmt::Display for SaveFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cannot {}: {}", self.operation, self.cause)
    }
}

impl std::error::Error for SaveFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.cause.as_ref())
    }
}

pub fn save(records: &[Document], destination: &Path, images: bool) -> Result<Totals, SaveFailure> {
    records
        .iter()
        .try_fold(Totals::default(), |mut count, item| {
            match item {
                Document::Printer {
                    profile,
                    image_source,
                } => {
                    count.images += usize::from(save_machine(
                        destination,
                        profile,
                        image_source.as_deref(),
                        images,
                    )?);
                    count.printers += 1;
                }
                Document::Material(resin) => {
                    save_material_profile_to_library(destination, resin)
                        .map_err(|e| SaveFailure::during("write resin", e))?;
                    count.materials += 1;
                }
            };
            Ok(count)
        })
}

fn save_machine(
    destination: &Path,
    profile: &sc_print_profiles::PrinterProfile,
    image_source: Option<&Path>,
    images: bool,
) -> Result<bool, SaveFailure> {
    let asset = match image_source.filter(|_| images) {
        Some(path) => Some(
            save_printer_image_to_library(destination, &profile.identity.id, path)
                .map_err(|e| SaveFailure::during("copy printer picture", e))?,
        ),
        None => None,
    };
    let mut document = Cow::Borrowed(profile);
    let copied = asset.is_some();
    if let Some(reference) = asset {
        document.to_mut().identity.image_asset_path = Some(reference);
    }
    save_printer_profile_to_library(destination, &document)
        .map_err(|e| SaveFailure::during("write printer", e))?;
    Ok(copied)
}
