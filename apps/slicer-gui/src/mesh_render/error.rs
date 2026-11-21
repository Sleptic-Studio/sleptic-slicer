// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! What the renderer reports when the GPU refuses it something: at start
//! (a pipeline or the gizmo's shapes) or for a frame (something sized to
//! the view or the scene), always as one readable line.
use std::error::Error;
use std::fmt;

use slint::wgpu_29::wgpu;

/// A list of the renderer's parts, each named for messages.
macro_rules! named_parts {
    ($(#[$doc:meta])* $name:ident { $($part:ident: $words:literal,)+ }) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name {
            $($part,)+
        }

        impl $name {
            #[cfg(test)]
            const EVERY: &'static [$name] = &[$($name::$part,)+];

            fn words(self) -> &'static str {
                match self {
                    $($name::$part => $words,)+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
                out.write_str(self.words())
            }
        }
    };
}

named_parts! {
    /// What the renderer builds once, when it starts.
    StartStep {
        PartsShader: "mesh pipeline",
        OutlineShader: "outline pipeline",
        HandleShapes: "gizmo geometry",
        HandlesShader: "gizmo pipeline",
        PlateShader: "build plate pipeline",
        CutShader: "slice cap pipeline",
    }
}

named_parts! {
    /// What a frame makes for the view's size or the scene.
    FrameResource {
        PlateBuffers: "build plate geometry",
        Multisampled: "multisample buffers",
        Outputs: "output image",
        CutTexture: "slice cap texture",
    }
}

/// The renderer could not start: the device refused `step`, the first
/// thing it refused.
#[derive(Debug)]
pub struct StartRefusal {
    pub step: StartStep,
    pub source: wgpu::Error,
}

impl fmt::Display for StartRefusal {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "The 3D view could not build its {}: {}", self.step, single_line(&self.source))
    }
}

impl Error for StartRefusal {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// Why a frame was not drawn.
#[derive(Debug)]
pub enum FrameRefusal {
    /// The device is gone, so the frame was not tried.
    Lost,
    /// The device refused something the frame needs.
    Refused { resource: FrameResource, source: wgpu::Error },
}

impl fmt::Display for FrameRefusal {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let FrameRefusal::Refused { resource, source } = self else {
            return out.write_str("The graphics device was lost.");
        };
        write!(out, "The graphics device refused the {resource}: {}", single_line(source))
    }
}

impl Error for FrameRefusal {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        if let FrameRefusal::Refused { source, .. } = self { Some(source) } else { None }
    }
}

/// wgpu writes an error over several lines: its kind, then "Caused by:"
/// and the chain of causes down to the root. The kind and the root are
/// what a reader needs, on one line.
fn single_line(error: &wgpu::Error) -> String {
    let text = error.to_string();
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let Some(kind) = lines.next() else {
        return String::new();
    };
    match lines.last() {
        Some(root) => format!("{kind}: {root}"),
        None => kind.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An error laid out the way wgpu lays out a validation error.
    fn validation(root: &str) -> wgpu::Error {
        let description = format!("Validation Error\n\nCaused by:\n  In Device::create_texture, label = 'y'\n    {root}\n");
        wgpu::Error::Validation { source: Box::new(std::io::Error::other(root.to_owned())), description }
    }

    #[test]
    fn a_refusal_reads_as_the_part_then_the_kind_and_root_cause() {
        let start = StartRefusal { step: StartStep::PlateShader, source: validation("Shader failed to compile") };
        assert_eq!(start.to_string(), "The 3D view could not build its build plate pipeline: Validation Error: Shader failed to compile");
        let frame = FrameRefusal::Refused { resource: FrameResource::Outputs, source: validation("Texture too large") };
        assert_eq!(frame.to_string(), "The graphics device refused the output image: Validation Error: Texture too large");
        assert!(start.source().is_some() && frame.source().is_some());
    }

    #[test]
    fn a_lost_device_has_no_cause_to_give() {
        assert_eq!(FrameRefusal::Lost.to_string(), "The graphics device was lost.");
        assert!(FrameRefusal::Lost.source().is_none());
    }

    #[test]
    fn a_short_error_stays_on_its_one_line() {
        let short = wgpu::Error::OutOfMemory { source: Box::new(std::io::Error::other("gone")) };
        let text = FrameRefusal::Refused { resource: FrameResource::Multisampled, source: short }.to_string();
        assert!(text.starts_with("The graphics device refused the multisample buffers: "), "{text}");
        assert_eq!(text.lines().count(), 1, "{text}");
    }

    #[test]
    fn no_two_parts_share_a_name() {
        let mut names: Vec<&str> = StartStep::EVERY.iter().map(|step| step.words()).collect();
        names.extend(FrameResource::EVERY.iter().map(|resource| resource.words()));
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!((before, names.len()), (10, 10));
    }
}
