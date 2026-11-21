// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Coordinate loading and saving without touching the destination until loading succeeds.
use crate::{
    json_catalog::JsonCatalogReader,
    library::{self, Totals},
    options::ConversionRequest,
};
use std::{error::Error, fmt};

#[derive(Debug)]
pub struct ConversionFailure(Box<dyn Error + Send + Sync>);

impl fmt::Display for ConversionFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Error for ConversionFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.0.as_ref())
    }
}

pub fn convert(request: &ConversionRequest) -> Result<Totals, ConversionFailure> {
    let paths: Vec<_> = request.inputs().iter().map(|path| path.as_path()).collect();
    let records = JsonCatalogReader::new()
        .read(&paths)
        .map_err(|error| ConversionFailure(Box::new(error)))?;
    library::save(&records, request.out(), request.with_images())
        .map_err(|error| ConversionFailure(Box::new(error)))
}
