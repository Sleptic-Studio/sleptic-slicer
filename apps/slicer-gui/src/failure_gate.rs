// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Repeated failures share one notification until the operation recovers.

use std::{cell::Cell, num::Saturating};

#[derive(Debug, Default)]
pub(crate) struct FailureStreak {
    failures: Cell<Saturating<u32>>,
}

impl FailureStreak {
    pub(crate) fn count_failure(&self) -> bool {
        let previous = self.failures.replace(self.failures.get() + Saturating(1));
        previous.0 == 0
    }

    pub(crate) fn reset(&self) {
        self.failures.set(Saturating(0));
    }

    #[cfg(test)]
    pub(crate) fn in_a_row(&self) -> u32 {
        self.failures.get().0
    }

    #[cfg(test)]
    fn seeded_for_test(count: u32) -> Self {
        Self {
            failures: Cell::new(Saturating(count)),
        }
    }
}

#[cfg(test)]
#[path = "tests_failure_gate.rs"]
mod tests_failure_gate;
