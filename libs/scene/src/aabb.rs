// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Expanded geometry bounds and common root displacement.
use crate::*;
use sc_mesh_core::{
    Aabb,
    transform::{AffineTransform3D, Transform3D},
};
impl Scene {
    fn bounds(&self, tight: bool) -> Result<Option<Aabb>, SceneError> {
        let mut bounds = Aabb::empty();
        self.for_each_expanded_leaf(CsgPolicy::Descend, |_, t, m| {
            if let Some(local) = m.local_aabb() {
                match t {
                    Transform3D::Identity => bounds.union(&local),
                    Transform3D::Affine(a) => {
                        if tight
                            && a.linear()
                                .iter()
                                .any(|row| row.iter().filter(|&&v| v != 0.).count() > 1)
                        {
                            for v in &m.vertices {
                                bounds.extend(t.apply_to_vertex(*v));
                            }
                        } else {
                            bounds.union(&local.transformed_arvo(a));
                        }
                    }
                }
            }
            Ok::<_, SceneError>(())
        })?;
        Ok((!bounds.is_empty()).then_some(bounds))
    }
    pub fn aabb(&self) -> Result<Option<Aabb>, SceneError> {
        self.bounds(false)
    }
    pub fn tight_aabb(&self) -> Result<Option<Aabb>, SceneError> {
        self.bounds(true)
    }
    pub fn translation_to_drop_at(
        &self,
        xy: [f32; 2],
        z: f32,
    ) -> Result<Option<[f64; 3]>, SceneError> {
        Ok(self.tight_aabb()?.map(|a| {
            let [x, y] = xy;
            let m = a.min();
            let big = a.max();
            [
                (x - 0.5 * (m[0] + big[0])) as f64,
                (y - 0.5 * (m[1] + big[1])) as f64,
                (z - m[2]) as f64,
            ]
        }))
    }
    pub fn drop_at(&mut self, xy: [f32; 2], z: f32) -> Result<bool, SceneError> {
        match self.translation_to_drop_at(xy, z)? {
            Some(displacement) if displacement != [0.; 3] => {
                let t = Transform3D::Affine(AffineTransform3D::from_translation(displacement));
                for id in self.roots.clone() {
                    let n = self.node_mut(id)?;
                    n.local_transform = t.compose(&n.local_transform);
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}
