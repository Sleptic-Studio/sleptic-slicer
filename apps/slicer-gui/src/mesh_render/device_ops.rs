// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! GPU calls that hand back their failure instead of sending it to the
//! device's uncaptured-error handler (which would only log it, or abort).
use slint::wgpu_29::wgpu;

/// Every kind of error an error scope can catch.
const ALL_KINDS: [wgpu::ErrorFilter; 3] =
    [wgpu::ErrorFilter::OutOfMemory, wgpu::ErrorFilter::Validation, wgpu::ErrorFilter::Internal];

/// Device calls whose refusal comes back as `Err`.
pub(super) trait Guarded {
    /// Runs `body` inside error scopes for every kind of error, and gives
    /// its value or the first error it caused on this thread. Scopes nest:
    /// one opened inside `body` catches first.
    fn guarded<T>(&self, body: impl FnOnce() -> T) -> Result<T, wgpu::Error>;

    /// A new buffer.
    fn try_buffer(&self, desc: &wgpu::BufferDescriptor<'_>) -> Result<wgpu::Buffer, wgpu::Error>;

    /// A buffer holding `contents`, written through `queue`: a mapped
    /// buffer would panic if the device refused it. `contents` must be a
    /// whole number of `COPY_BUFFER_ALIGNMENT` units; the buffer can also
    /// be copied into.
    fn try_upload(
        &self,
        queue: &wgpu::Queue,
        label: &'static str,
        contents: &[u8],
        usage: wgpu::BufferUsages,
    ) -> Result<wgpu::Buffer, wgpu::Error>;
}

impl Guarded for wgpu::Device {
    fn guarded<T>(&self, body: impl FnOnce() -> T) -> Result<T, wgpu::Error> {
        let opened: Vec<_> = ALL_KINDS.iter().map(|&kind| self.push_error_scope(kind)).collect();
        let value = body();
        // Close them all, last opened first, whatever they caught; native
        // wgpu answers a pop straight away.
        let caught: Vec<wgpu::Error> = opened.into_iter().rev().filter_map(|scope| pollster::block_on(scope.pop())).collect();
        match caught.into_iter().next() {
            Some(error) => Err(error),
            None => Ok(value),
        }
    }

    fn try_buffer(&self, desc: &wgpu::BufferDescriptor<'_>) -> Result<wgpu::Buffer, wgpu::Error> {
        self.guarded(|| self.create_buffer(desc))
    }

    fn try_upload(
        &self,
        queue: &wgpu::Queue,
        label: &'static str,
        contents: &[u8],
        usage: wgpu::BufferUsages,
    ) -> Result<wgpu::Buffer, wgpu::Error> {
        let usage = usage | wgpu::BufferUsages::COPY_DST;
        let size = contents.len() as u64;
        let buffer = self.try_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false })?;
        queue.write_buffer(&buffer, 0, contents);
        Ok(buffer)
    }
}
