// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Keys that tell whether saved slices still match their scene: the same
//! on every platform, and versioned so an encoding change retires old keys.
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use sc_scene::{Scene, SceneError};
use sc_sl1cer::{error::SliceSceneError, profile::LayerSchedule, sliceable_leaves};
use sha2::{Digest, Sha384};
use std::fmt;
/// A SHA-384 key for a slice's inputs. The encoding's version string is
/// hashed first, so a new encoding never matches an old key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SliceInputFingerprint([u8; 48]);
impl SliceInputFingerprint {
    /// The digest's 48 bytes, always in the same order.
    pub fn as_bytes(&self) -> &[u8; 48] {
        &self.0
    }
    /// The digest in lowercase hex, the form stored next to saved slices.
    pub fn to_token(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
/// Why no key could be made for a scene.
#[derive(Debug)]
pub enum SliceInputFingerprintError {
    /// The slicer could not list the scene's parts.
    Enumeration(SliceSceneError),
    /// A part's mesh or placement could not be read.
    Scene(SceneError),
}
impl fmt::Display for SliceInputFingerprintError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scene(why) => write!(out, "Cannot resolve fingerprint inputs: {why}"),
            Self::Enumeration(why) => write!(out, "Cannot enumerate fingerprint inputs: {why}"),
        }
    }
}
impl std::error::Error for SliceInputFingerprintError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Scene(why) => Some(why),
            Self::Enumeration(why) => Some(why),
        }
    }
}
/// Whether slices saved with a project still belong to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistedSliceValidity {
    /// The saved key is this version's key for the scene as it is.
    InSync,
    /// Anything else: changed inputs, another version's key, or no key
    /// could be made. The slices are made again.
    Stale,
}

/// Everything goes into the hash little-endian, floats by their bits.
struct Feed(Sha384);
impl Feed {
    fn count(&mut self, n: usize) {
        self.0.update((n as u64).to_le_bytes());
    }
    fn f32s(&mut self, values: impl IntoIterator<Item = f32>) {
        values.into_iter().for_each(|v| self.0.update(v.to_bits().to_le_bytes()));
    }
    fn f64s(&mut self, values: impl IntoIterator<Item = f64>) {
        values.into_iter().for_each(|v| self.0.update(v.to_bits().to_le_bytes()));
    }
    fn u32s(&mut self, values: impl IntoIterator<Item = u32>) {
        values.into_iter().for_each(|v| self.0.update(v.to_le_bytes()));
    }
    /// A placement as its 3x3 linear part row by row, then its shift.
    fn placement(&mut self, transform: Transform3D) {
        let affine = match transform {
            Transform3D::Affine(affine) => affine,
            Transform3D::Identity => AffineTransform3D::identity(),
        };
        self.f64s(affine.linear().iter().flatten().copied());
        self.f64s(affine.translation().iter().copied());
    }
}

/// The key for slicing `scene` on `schedule`: every part's mesh (vertices
/// and faces, not normals or names), in the order the slicer finds them,
/// each with its world placement, then the layers. The `v2` version string
/// retired the keys of older saved slices; the scenes themselves stay.
pub fn slice_input_fingerprint(
    scene: &Scene,
    schedule: &LayerSchedule,
) -> Result<SliceInputFingerprint, SliceInputFingerprintError> {
    use SliceInputFingerprintError::{Enumeration, Scene as Lookup};
    let parts = sliceable_leaves(scene).map_err(Enumeration)?;
    let mut feed = Feed(Sha384::new());
    feed.0.update(b"Sleptic/vector-slice/input/v2\0");
    feed.count(parts.len());
    for part in parts {
        let mesh = scene.mesh_at_leaf(part).map_err(Lookup)?;
        feed.count(mesh.vertices.len());
        feed.f32s(mesh.vertices.iter().flat_map(|vertex| vertex.coords()));
        feed.count(mesh.faces.len());
        feed.u32s(mesh.faces.iter().flat_map(|face| face.vertices.map(|index| index.get())));
        feed.placement(scene.world_transform_of(part).map_err(Lookup)?);
    }
    let LayerSchedule::Uniform(layers) = schedule;
    feed.0.update([0u8]); // the uniform kind
    feed.f64s([layers.layer_height().value(), layers.first_layer_height().value()]);
    feed.u32s([layers.total_layers().get()]);
    Ok(SliceInputFingerprint(feed.0.finalize().into()))
}
/// Saved slices are kept only when `stored_token` is exactly the key the
/// scene has now; anything else (including no key at all) means slicing
/// again, which is always possible.
pub fn validate_persisted_slices(
    stored_token: &str,
    scene: &Scene,
    schedule: &LayerSchedule,
) -> PersistedSliceValidity {
    let current = slice_input_fingerprint(scene, schedule).map(|key| key.to_token());
    if current.is_ok_and(|token| token == stored_token) {
        PersistedSliceValidity::InSync
    } else {
        PersistedSliceValidity::Stale
    }
}
