// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! What can be wrong with one JsonCatalog record, for printers and resins
//! alike: which record, which field, and what about it.
use std::error::Error;
use std::fmt;

use sc_print_profiles::InvalidProfileId;

/// The kind of record a problem is in, which the message names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordKind {
    Printer,
    Resin,
}

/// What is wrong with the field.
#[derive(Debug)]
pub enum Problem {
    /// The record's id is not a valid profile id.
    BadId(InvalidProfileId),
    /// A text that must say something is empty.
    Empty,
    /// An axis has neither a length nor a pixel pitch to work it out from.
    NoGeometry,
    /// A printer the resin lists as a target has an invalid id.
    BadTarget { target: String, why: InvalidProfileId },
    /// The profile's own rules refuse the value; the cause says how.
    Refused(Box<dyn Error + Send + Sync>),
}

/// One record that could not be turned into a profile.
#[derive(Debug)]
pub struct RecordProblem {
    pub kind: RecordKind,
    /// The record's id as written in the file.
    pub record: String,
    /// The JSON key (or axis, or setting) at fault.
    pub field: &'static str,
    pub problem: Problem,
}

impl RecordProblem {
    pub(super) fn new(kind: RecordKind, record: &str, field: &'static str, problem: Problem) -> Self {
        RecordProblem { kind, record: record.to_owned(), field, problem }
    }

    /// A refused value: `why` is the profile rule's own error.
    pub(super) fn refused(kind: RecordKind, record: &str, field: &'static str, why: impl Error + Send + Sync + 'static) -> Self {
        Self::new(kind, record, field, Problem::Refused(Box::new(why)))
    }
}

impl fmt::Display for RecordProblem {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.kind {
            RecordKind::Printer => "printer",
            RecordKind::Resin => "resin",
        };
        let (id, field) = (&self.record, self.field);
        match &self.problem {
            Problem::BadId(why) => write!(out, "the {what} id {id:?} is not valid: {why}"),
            Problem::Empty => write!(out, "{what} {id:?}: {field} is empty"),
            Problem::NoGeometry => write!(out, "{what} {id:?}: axis {field} has neither a size nor a pixel pitch"),
            Problem::BadTarget { target, why } => write!(out, "{what} {id:?}: target printer {target:?} is not a valid id: {why}"),
            Problem::Refused(why) => write!(out, "{what} {id:?}: {field} is not allowed: {why}"),
        }
    }
}

impl Error for RecordProblem {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.problem {
            Problem::BadId(why) | Problem::BadTarget { why, .. } => Some(why),
            Problem::Refused(why) => Some(why.as_ref()),
            Problem::Empty | Problem::NoGeometry => None,
        }
    }
}
