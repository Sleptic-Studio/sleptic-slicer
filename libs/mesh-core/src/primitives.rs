// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use std::{fmt, ops::Index, slice::SliceIndex};

macro_rules! identifier {
    ($name:ident, $($variant:ident = $value:literal => $text:literal),+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum $name { $($variant = $value),+ }
        impl $name {
            pub const ALL: [Self; 3] = [$(Self::$variant),+];
            pub const fn as_index(self) -> usize { self as usize }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(match self { $(Self::$variant => $text),+ })
            }
        }
    }
}
identifier!(CoordAxis, X = 0 => "X", Y = 1 => "Y", Z = 2 => "Z");
identifier!(TriVertexSlot, V0 = 0 => "v0", V1 = 1 => "v1", V2 = 2 => "v2");

macro_rules! triple {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq)]
        #[repr(transparent)]
        pub struct $name([f32; 3]);
        impl $name {
            pub const fn new(coords: [f32; 3]) -> Self {
                Self(coords)
            }
            pub const fn coords(self) -> [f32; 3] {
                self.0
            }
            pub fn to_f64(self) -> Vector3D<f64> {
                Vector3D(self.0.map(f64::from))
            }
            pub const fn len(&self) -> usize {
                3
            }
            pub const fn is_empty(&self) -> bool {
                false
            }
            pub fn iter(&self) -> std::slice::Iter<'_, f32> {
                self.0.iter()
            }
            pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, f32> {
                self.0.iter_mut()
            }
            pub const fn as_array(&self) -> &[f32; 3] {
                &self.0
            }
            pub fn as_array_mut(&mut self) -> &mut [f32; 3] {
                &mut self.0
            }
        }
        impl<I: SliceIndex<[f32]>> Index<I> for $name {
            type Output = I::Output;
            fn index(&self, i: I) -> &Self::Output {
                &self.0[i]
            }
        }
        impl<I: SliceIndex<[f32]>> std::ops::IndexMut<I> for $name {
            fn index_mut(&mut self, i: I) -> &mut Self::Output {
                &mut self.0[i]
            }
        }
        impl AsRef<[f32]> for $name {
            fn as_ref(&self) -> &[f32] {
                &self.0
            }
        }
        impl AsMut<[f32]> for $name {
            fn as_mut(&mut self) -> &mut [f32] {
                &mut self.0
            }
        }
        impl From<$name> for [f32; 3] {
            fn from(v: $name) -> Self {
                v.0
            }
        }
        impl IntoIterator for $name {
            type Item = f32;
            type IntoIter = std::array::IntoIter<f32, 3>;
            fn into_iter(self) -> Self::IntoIter {
                self.0.into_iter()
            }
        }
        impl<'a> IntoIterator for &'a $name {
            type Item = &'a f32;
            type IntoIter = std::slice::Iter<'a, f32>;
            fn into_iter(self) -> Self::IntoIter {
                self.iter()
            }
        }
        impl<'a> IntoIterator for &'a mut $name {
            type Item = &'a mut f32;
            type IntoIter = std::slice::IterMut<'a, f32>;
            fn into_iter(self) -> Self::IntoIter {
                self.iter_mut()
            }
        }
    };
}
triple!(Vertex);
triple!(Normal);
impl Normal {
    pub fn negate(self) -> Self {
        Self(self.0.map(|x| -x))
    }
    pub fn norm(self) -> f32 {
        (self[0] * self[0] + self[1] * self[1] + self[2] * self[2]).sqrt()
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Vector3D<F>([F; 3]);
impl<F: Copy> Vector3D<F> {
    pub const fn new(coords: [F; 3]) -> Self {
        Self(coords)
    }
    pub fn coords(&self) -> [F; 3] {
        self.0
    }
}
impl<F> From<Vector3D<F>> for [F; 3] {
    fn from(v: Vector3D<F>) -> Self {
        v.0
    }
}
impl<F> Index<usize> for Vector3D<F> {
    type Output = F;
    fn index(&self, i: usize) -> &F {
        &self.0[i]
    }
}
macro_rules! vector_math {
    ($f:ty) => {
        impl Vector3D<$f> {
            pub fn squared_norm(&self) -> $f {
                self.dot_product(self)
            }
            pub fn norm(&self) -> $f {
                self.squared_norm().sqrt()
            }
            pub fn diff(&self, b: &Self) -> Self {
                Self(std::array::from_fn(|i| self[i] - b[i]))
            }
            pub fn dot_product(&self, b: &Self) -> $f {
                self[0].mul_add(b[0], self[1].mul_add(b[1], self[2] * b[2]))
            }
            pub fn cross_product(&self, b: &Self) -> Self {
                Self([
                    self[1].mul_add(b[2], -self[2] * b[1]),
                    self[2].mul_add(b[0], -self[0] * b[2]),
                    self[0].mul_add(b[1], -self[1] * b[0]),
                ])
            }
        }
    };
}
vector_math!(f32);
vector_math!(f64);
impl From<Vector3D<f32>> for Vector3D<f64> {
    fn from(v: Vector3D<f32>) -> Self {
        Self(v.0.map(f64::from))
    }
}
impl From<Vector3D<f64>> for Vector3D<f32> {
    fn from(v: Vector3D<f64>) -> Self {
        Self(v.0.map(|x| x as f32))
    }
}

macro_rules! index_type {
    ($name:ident,$raw:ty) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name($raw);
        impl $name {
            pub const fn get(self) -> $raw {
                self.0
            }
        }
        impl From<$raw> for $name {
            fn from(v: $raw) -> Self {
                Self(v)
            }
        }
        impl From<$name> for $raw {
            fn from(v: $name) -> Self {
                v.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}
index_type!(VertexIndex, u32);
index_type!(FaceIndex, usize);
impl VertexIndex {
    pub fn offset(self, n: u32) -> Self {
        Self(self.0 + n)
    }
}
impl From<VertexIndex> for usize {
    fn from(v: VertexIndex) -> Self {
        v.0 as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Triangle {
    normal: Normal,
    vertices: [Vertex; 3],
}
impl Triangle {
    pub const fn new(normal: Normal, vertices: [Vertex; 3]) -> Self {
        Self { normal, vertices }
    }
    pub const fn normal(&self) -> Normal {
        self.normal
    }
    pub const fn vertices(&self) -> [Vertex; 3] {
        self.vertices
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IndexedTriangle {
    pub normal: Normal,
    pub vertices: [VertexIndex; 3],
}
impl IndexedTriangle {
    pub fn from_raw_indices(normal: Normal, indices: [u32; 3]) -> Self {
        Self {
            normal,
            vertices: indices.map(VertexIndex::from),
        }
    }
}
