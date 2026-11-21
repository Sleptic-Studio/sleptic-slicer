// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::*;
use crate::{geometry::*, indexed_mesh::*, primitives::*, simplify::*, transform::*};
use indexed_mesh::{RepairAssessment, assess_repair};
use proptest::prelude::*;
use std::error::Error;

fn n() -> Normal {
    Normal::new([0., 0., 1.])
}
fn face(ids: [u32; 3]) -> IndexedTriangle {
    IndexedTriangle::from_raw_indices(n(), ids)
}
fn open() -> IndexedMesh {
    IndexedMesh::new(
        None,
        vec![
            Vertex::new([0., 0., 0.]),
            Vertex::new([1., 0., 0.]),
            Vertex::new([0., 1., 0.]),
        ],
        vec![face([0, 1, 2])],
    )
}
fn tetra() -> IndexedMesh {
    let mut mesh = IndexedMesh::new(
        None,
        vec![
            Vertex::new([0., 0., 0.]),
            Vertex::new([1., 0., 0.]),
            Vertex::new([0., 1., 0.]),
            Vertex::new([0., 0., 1.]),
        ],
        vec![
            face([0, 2, 1]),
            face([0, 1, 3]),
            face([0, 3, 2]),
            face([1, 2, 3]),
        ],
    );
    recalc(&mut mesh);
    mesh
}
fn recalc(mesh: &mut IndexedMesh) {
    for f in &mut mesh.faces {
        f.normal = compute_normal_from_triangle_vertices(
            &f.vertices.map(|v| mesh.vertices[usize::from(v)]),
        )
        .unwrap();
    }
}
fn closed(mesh: &IndexedMesh) {
    mesh.validated_faces()
        .unwrap()
        .validated_edge_manifold()
        .unwrap();
}
fn combine(a: &IndexedMesh, b: &IndexedMesh) -> IndexedMesh {
    let mut m = a.clone();
    let offset = m.vertices.len() as u32;
    m.vertices.extend_from_slice(&b.vertices);
    m.faces.extend(b.faces.iter().map(|f| IndexedTriangle {
        normal: f.normal,
        vertices: f.vertices.map(|v| v.offset(offset)),
    }));
    m
}
fn soup(mesh: &IndexedMesh) -> Vec<Triangle> {
    mesh.faces
        .iter()
        .map(|f| Triangle::new(f.normal, f.vertices.map(|v| mesh.vertices[usize::from(v)])))
        .collect()
}

#[test]
fn identifiers_and_indices() {
    for (i, a) in CoordAxis::ALL.into_iter().enumerate() {
        assert_eq!(a.as_index(), i);
        assert_eq!(a.to_string(), ["X", "Y", "Z"][i]);
    }
    for (i, s) in TriVertexSlot::ALL.into_iter().enumerate() {
        assert_eq!(s.as_index(), i);
        assert_eq!(s.to_string(), format!("v{i}"));
    }
    let v = VertexIndex::from(42);
    assert_eq!(v.get(), 42);
    assert_eq!(usize::from(v.offset(3)), 45);
    assert_eq!(FaceIndex::from(9).to_string(), "9");
}
#[test]
fn wrapper_array_operations() {
    let mut v = Vertex::new([1., 2., 3.]);
    assert_eq!(v.coords(), [1., 2., 3.]);
    assert_eq!(&v[1..], &[2., 3.]);
    v.as_mut()[0] = 4.;
    v[1] = 5.;
    for x in &mut v {
        *x += 1.;
    }
    assert_eq!(v.into_iter().collect::<Vec<_>>(), vec![5., 6., 4.]);
    assert_eq!(v.len(), 3);
    assert!(!v.is_empty());
    assert_eq!(v.as_array(), &[5., 6., 4.]);
    assert_eq!(v.to_f64().coords(), [5., 6., 4.]);
    assert_eq!(size_of::<Vertex>(), 12);
    assert_eq!(size_of::<Normal>(), 12);
}
#[test]
fn normal_values_not_normalized() {
    let v = Normal::new([3., 4., 0.]);
    assert_eq!(v.norm(), 5.);
    assert_eq!(v.negate().coords(), [-3., -4., -0.]);
}
#[test]
fn vector_math_and_order() {
    let a = Vector3D::new([1f64, 2., 3.]);
    let b = Vector3D::new([4., 5., 6.]);
    assert_eq!(a.dot_product(&b), 32.);
    assert_eq!(a.cross_product(&b).coords(), [-3., 6., -3.]);
    assert_eq!(a.diff(&b).coords(), [-3.; 3]);
    assert_eq!(a.squared_norm(), 14.);
    assert!(a < b);
    let f: Vector3D<f32> = a.into();
    assert_eq!(Vector3D::<f64>::from(f), a);
    assert_eq!(Vector3D::<f32>::default().coords(), [0.; 3]);
}
#[test]
fn normal_winding_and_scale() {
    for scale in [1e-15, 1., 1e15] {
        let v = [
            Vertex::new([0.; 3]),
            Vertex::new([scale, 0., 0.]),
            Vertex::new([0., scale, 0.]),
        ];
        assert_eq!(compute_normal_from_triangle_vertices(&v).unwrap(), n());
        assert_eq!(
            compute_normal_from_triangle_vertices(&[v[2], v[1], v[0]]).unwrap(),
            n().negate()
        );
    }
}
#[test]
fn normal_degeneracy_cases() {
    for v in [
        [Vertex::new([0.; 3]); 3],
        [
            Vertex::new([0.; 3]),
            Vertex::new([1., 0., 0.]),
            Vertex::new([2., 0., 0.]),
        ],
        [
            Vertex::new([f32::NAN, 0., 0.]),
            Vertex::new([1., 0., 0.]),
            Vertex::new([0., 1., 0.]),
        ],
    ] {
        assert_eq!(
            compute_normal_from_triangle_vertices(&v),
            Err(GeometryError::DegenerateTriangle)
        );
    }
    assert_eq!(
        GeometryError::DegenerateTriangle.to_string(),
        "degenerate (near-zero area) triangle"
    );
}
#[test]
fn angular_degeneracy_threshold() {
    let mut v = open().vertices;
    v[2] = Vertex::new([1., 0.0000005, 0.]);
    assert!(compute_normal_from_triangle_vertices(&v.clone().try_into().unwrap()).is_err());
    v[2] = Vertex::new([1., 0.000002, 0.]);
    assert!(compute_normal_from_triangle_vertices(&v.try_into().unwrap()).is_ok());
}
#[test]
fn bounds_all_vertices_and_live_updates() {
    let mut m = open();
    m.vertices.push(Vertex::new([100., -30., 7.]));
    let a = m.local_aabb().unwrap();
    assert_eq!(a.max().coords(), [100., 1., 7.]);
    assert_eq!(a.min().coords(), [0., -30., 0.]);
    m.vertices[3] = Vertex::new([200., 0., 0.]);
    assert_eq!(m.local_aabb().unwrap().max()[0], 200.);
    m.vertices.clear();
    assert_eq!(m.local_aabb(), None);
}
#[test]
fn bounds_include_unreferenced_and_nonfinite_vertices() {
    let mesh = IndexedMesh::new(
        None,
        vec![
            Vertex::new([f32::NAN, 2., f32::INFINITY]),
            Vertex::new([-4., f32::NEG_INFINITY, 8.]),
        ],
        vec![],
    );
    let bounds = mesh.local_aabb().unwrap();
    assert_eq!(bounds.min().coords(), [-4., f32::NEG_INFINITY, 8.]);
    assert_eq!(bounds.max().coords(), [-4., 2., f32::INFINITY]);
}
#[test]
fn empty_box_transform() {
    let a = Aabb::empty();
    assert!(a.is_empty());
    assert_eq!(
        a.transformed_arvo(&AffineTransform3D::from_translation([4., 3., 2.])),
        a
    );
}
#[test]
fn bounds_nan_and_union() {
    let mut a = Aabb::empty();
    a.extend(Vertex::new([f32::NAN, 2., 3.]));
    a.extend(Vertex::new([1., 4., 0.]));
    assert_eq!(a.min().coords(), [1., 2., 0.]);
    let b = Aabb::from_min_max(Vertex::new([-5., 1., -2.]), Vertex::new([8., 2., 9.]));
    a.union(&b);
    assert_eq!(a.min(), b.min());
    assert_eq!(a.max().coords(), [8., 4., 9.]);
}
#[test]
fn affine_composition_and_inverse() {
    let a = AffineTransform3D::from_rotation_z(0.8)
        .compose(&AffineTransform3D::from_non_uniform_scale([2., 3., 4.]));
    let a = AffineTransform3D::from_translation([6., -2., 4.]).compose(&a);
    let p = Vertex::new([1., 2., 3.]);
    let q = a.inverse().unwrap().apply_to_vertex(a.apply_to_vertex(p));
    for i in 0..3 {
        assert!((p[i] - q[i]).abs() < 1e-5);
    }
    assert!((a.determinant() - 24.).abs() < 1e-12);
}
#[test]
fn affine_axis_angle() {
    let a = AffineTransform3D::from_axis_angle([0., 0., 7.], 0.7).unwrap();
    let b = AffineTransform3D::from_rotation_z(0.7);
    for i in 0..3 {
        for j in 0..3 {
            assert!((a.linear()[i][j] - b.linear()[i][j]).abs() < 1e-15);
        }
    }
    for axis in [[0.; 3], [f64::NAN, 0., 0.], [1e-10, 0., 0.]] {
        assert_eq!(
            AffineTransform3D::from_axis_angle(axis, 0.),
            Err(TransformError::ZeroAxis)
        );
    }
}
#[test]
fn transform_identity_preserves_invalid_values() {
    let normal = Normal::new([0.; 3]);
    assert_eq!(Transform3D::Identity.apply_to_normal(normal), Ok(normal));
    assert!(Transform3D::Identity.is_identity());
    let a = Transform3D::Affine(AffineTransform3D::identity());
    assert!(!a.is_identity());
    assert_eq!(a.compose(&Transform3D::Identity), a);
    assert_eq!(Transform3D::Identity.compose(&a), a);
    assert_eq!(
        Transform3D::Identity.inverse().unwrap(),
        Transform3D::Identity
    );
}
#[test]
fn transform_normal_inverse_transpose_and_reflection() {
    let a = AffineTransform3D::from_non_uniform_scale([-2., 3., 4.]);
    assert_eq!(
        a.apply_to_normal(Normal::new([1., 0., 0.])).unwrap(),
        Normal::new([-1., 0., 0.])
    );
    let input = Normal::new([1., 1., 0.]);
    let out = a.apply_to_normal(input).unwrap();
    assert!((out.norm() - 1.).abs() < 1e-6);
    assert!((out[0] / out[1] + 1.5).abs() < 1e-6);
    assert_eq!(
        a.apply_to_normal(Normal::new([0.; 3])),
        Err(TransformError::ZeroMagnitudeNormal)
    );
}
#[test]
fn transform_errors_and_thresholds() {
    for s in [0., f64::NAN, 1e-8] {
        let a = AffineTransform3D::from_uniform_scale(s);
        assert_eq!(a.inverse(), Err(TransformError::SingularLinear));
        assert_eq!(a.apply_to_normal(n()), Err(TransformError::SingularLinear));
    }
    let a = AffineTransform3D::from_uniform_scale(1e-4);
    assert_eq!(
        a.apply_to_normal(n()),
        Err(TransformError::ZeroMagnitudeNormal)
    );
    assert_eq!(
        TransformError::ZeroAxis.to_string(),
        "rotation axis must be non-zero"
    );
}
#[test]
fn vertex_error_order() {
    let mut m = open();
    m.vertices[0] = Vertex::new([f32::NAN, f32::INFINITY, f32::NEG_INFINITY]);
    m.vertices.push(Vertex::new([f32::NAN, 0., 0.]));
    let e = m.validate_vertices().unwrap_err();
    assert_eq!(e.len(), 4);
    assert_eq!(
        e[0],
        MeshVertexError::NaNCoordinate {
            vertex_idx: 0,
            coord: CoordAxis::X
        }
    );
    assert_eq!(e[1].to_string(), "vertex #0 has an infinite Y coordinate");
    assert_eq!(e[3].to_string(), "vertex #3 has a NaN X coordinate");
}
#[test]
fn finite_signed_zero_and_subnormal() {
    let mut m = open();
    m.vertices
        .push(Vertex::new([-0., f32::from_bits(1), f32::MAX]));
    assert!(m.validate_vertices().is_ok());
}
#[test]
fn face_error_order_and_multiplicity() {
    let mut m = open();
    m.faces = vec![face([9, 9, 9])];
    let e = m.validate_faces().unwrap_err();
    assert_eq!(e.len(), 6);
    assert!(matches!(
        e[0],
        MeshFaceError::OutOfRangeVertexIndex {
            vertex_pos: TriVertexSlot::V0,
            ..
        }
    ));
    assert!(matches!(
        e[2],
        MeshFaceError::OutOfRangeVertexIndex {
            vertex_pos: TriVertexSlot::V2,
            ..
        }
    ));
    assert_eq!(
        e[5].to_string(),
        "face #0 has a zero-length edge: slots v2 and v0 both bound to vertex #9"
    );
}
#[test]
fn face_validation_only_identity() {
    let mut m = open();
    m.vertices[1] = m.vertices[0];
    m.vertices[2] = m.vertices[0];
    m.faces[0].normal = Normal::new([f32::NAN; 3]);
    assert!(m.validated_faces().is_ok());
    assert!(matches!(
        m.validated_faces()
            .unwrap()
            .validate_normal_vectors()
            .unwrap_err()[0],
        MeshNormalVectorError::ZeroAreaTriangle { .. }
    ));
}
#[test]
fn normal_error_precedence_and_order() {
    let mut m = open();
    m.faces = vec![face([0, 1, 2]); 3];
    m.faces[0].normal = Normal::new([f32::NAN; 3]);
    m.faces[1].normal = n().negate();
    let e = m
        .validated_faces()
        .unwrap()
        .validate_normal_vectors()
        .unwrap_err();
    assert_eq!(e.len(), 2);
    assert!(matches!(e[0],MeshNormalVectorError::NonUnitNorm {face_idx,..} if face_idx.get()==0));
    assert!(
        matches!(e[1],MeshNormalVectorError::ExpectationMismatch {face_idx} if face_idx.get()==1)
    );
}
#[test]
fn normal_tolerances() {
    let mut m = open();
    m.faces[0].normal = Normal::new([0., 0., 1.0000004]);
    assert!(
        m.validated_faces()
            .unwrap()
            .validate_normal_vectors()
            .is_ok()
    );
    m.faces[0].normal = Normal::new([0., 0., 1.000001]);
    assert!(matches!(
        m.validated_faces()
            .unwrap()
            .validate_normal_vectors()
            .unwrap_err()[0],
        MeshNormalVectorError::NonUnitNorm { .. }
    ));
    for (angle, pass) in [(4.99f32, true), (5.01, false)] {
        let r = angle.to_radians();
        m.faces[0].normal = Normal::new([r.sin(), 0., r.cos()]);
        assert_eq!(
            m.validated_faces()
                .unwrap()
                .validate_normal_vectors()
                .is_ok(),
            pass
        );
    }
}
#[test]
fn edge_errors_sorted_and_empty() {
    let m = open();
    let e = m
        .validated_faces()
        .unwrap()
        .validate_edge_manifold()
        .unwrap_err();
    assert_eq!(e.len(), 3);
    assert!(
        matches!(e[0],MeshEdgeManifoldError::BoundaryEdge {edge,..} if edge==(0.into(),1.into()))
    );
    assert!(
        matches!(e[1],MeshEdgeManifoldError::BoundaryEdge {edge,..} if edge==(0.into(),2.into()))
    );
    let m = IndexedMesh::new(None, vec![], vec![]);
    assert!(m.validate_faces().is_ok());
    assert_eq!(
        m.validated_faces().unwrap().validate_edge_manifold(),
        Err(vec![MeshEdgeManifoldError::EmptyMesh])
    );
}
#[test]
fn edge_closed_disconnected_and_inward() {
    let a = tetra();
    closed(&a);
    let mut b = a.clone();
    for f in &mut b.faces {
        f.vertices.reverse();
    }
    closed(&b);
    closed(&combine(&a, &b));
}
#[test]
fn edge_accepts_invalid_coordinates_and_normals() {
    let mut m = tetra();
    m.vertices[0] = Vertex::new([f32::NAN; 3]);
    m.faces[0].normal = Normal::new([0.; 3]);
    closed(&m);
}
#[test]
fn edge_winding_error_faces_sorted() {
    let mut m = tetra();
    m.faces[2].vertices.reverse();
    let e = m
        .validated_faces()
        .unwrap()
        .validate_edge_manifold()
        .unwrap_err();
    assert_eq!(e.len(), 3);
    for e in e {
        assert!(
            matches!(e,MeshEdgeManifoldError::InconsistentWindingOrder {face_indices,..} if face_indices[0]<face_indices[1])
        );
    }
}
#[test]
fn diagnosis_collects_independent_checks() {
    let mut m = open();
    m.vertices.push(Vertex::new([f32::NAN; 3]));
    m.faces[0].normal = n().negate();
    let d = m.diagnose();
    assert_eq!(d.vertex_errors.len(), 3);
    assert!(d.has_problems());
    assert!(
        matches!(d.faces,FaceDiagnosis::Valid {normal_vector_errors,edge_manifold_errors} if normal_vector_errors.len()==1 && edge_manifold_errors.len()==3)
    );
    m.faces[0] = face([0, 0, 9]);
    assert!(matches!(m.diagnose().faces, FaceDiagnosis::Invalid(_)));
    assert!(!tetra().diagnose().has_problems());
}
#[test]
fn grouped_errors_preserve_references_and_order() {
    let e = vec![
        MeshVertexError::NaNCoordinate {
            vertex_idx: 0,
            coord: CoordAxis::X,
        },
        MeshVertexError::InfiniteCoordinate {
            vertex_idx: 1,
            coord: CoordAxis::Y,
        },
        MeshVertexError::NaNCoordinate {
            vertex_idx: 2,
            coord: CoordAxis::Z,
        },
    ];
    let g = group_errors_by_variant(&e);
    assert_eq!(g.len(), 2);
    assert_eq!(g[0].variant_name, "NaNCoordinate");
    assert!(std::ptr::eq(g[0].members[1], &e[2]));
    assert_eq!(e[0].example_face(), None);
    assert_eq!(
        MeshEdgeManifoldError::NonManifoldEdge {
            edge: (0.into(), 1.into()),
            face_indices: vec![]
        }
        .example_face(),
        None
    );
}
#[test]
fn repair_defaults_and_assessment() {
    assert_eq!(
        RepairPasses::default(),
        RepairPasses {
            zero_length_edges: false,
            flipped_faces: false,
            face_normals: false,
            separate_shells: false
        }
    );
    assert_eq!(
        assess_repair(&tetra(), RepairPasses::default_repair()),
        RepairAssessment::Clean
    );
    let m = open();
    assert!(matches!(
        assess_repair(&m, RepairPasses::default_repair()),
        RepairAssessment::Refused(RepairRefusal::OrientationFixFailed(
            MeshFixError::TooManyFlippedFaces
        ))
    ));
}
#[test]
fn repair_vertex_refusal_no_mutation_but_edge_verdict() {
    let mut m = tetra();
    m.vertices[0] = Vertex::new([f32::NAN; 3]);
    let old = soup(&m);
    let r = m.repair(RepairPasses::default_repair());
    assert!(r.edge_manifold_after);
    assert!(r.fixes.is_empty());
    assert!(matches!(r.refusal, Some(RepairRefusal::InvalidVertices(_))));
    assert_eq!(soup(&m)[1].normal(), old[1].normal());
}
#[test]
fn repair_face_refusal_transactional_residue() {
    let mut m = tetra();
    m.faces.push(face([0, 0, 0]));
    m.faces.push(face([0, 9, 9]));
    let before = m.clone();
    let r = m.repair(RepairPasses::default_repair());
    assert_eq!(m, before);
    assert!(matches!(r.refusal,Some(RepairRefusal::UnfixableFaceErrors(ref e)) if e.len()==2));
    let r = m.repair(RepairPasses::default());
    assert!(matches!(r.refusal,Some(RepairRefusal::UnfixableFaceErrors(ref e)) if e.len()==6));
}
#[test]
fn repeated_index_removal_log_and_stable_order() {
    let mut m = tetra();
    let original = m.clone();
    m.faces.insert(1, face([0, 0, 0]));
    let r = m.repair(RepairPasses::default_repair());
    assert_eq!(
        r.fixes,
        vec![MeshFix::RemovedZeroLengthEdgeFace { face_idx: 1.into() }; 3]
    );
    assert_eq!(m, original);
    assert!(r.edge_manifold_after);
    assert_eq!(r.refusal, None);
}
#[test]
fn assess_early_work_precedes_later_refusal() {
    let mut m = open();
    m.faces.push(face([0, 0, 0]));
    assert_eq!(
        assess_repair(&m, RepairPasses::default_repair()),
        RepairAssessment::Work
    );
    let r = m.repair(RepairPasses::default_repair());
    assert_eq!(m.faces.len(), 1);
    assert_eq!(r.fixes.len(), 3);
    assert!(matches!(
        r.refusal,
        Some(RepairRefusal::OrientationFixFailed(
            MeshFixError::TooManyFlippedFaces
        ))
    ));
}
#[test]
fn isolated_flip_restores_full_order_and_normal_bits() {
    let original = tetra();
    let mut m = original.clone();
    m.faces[1].vertices.reverse();
    m.faces[1].normal = m.faces[1].normal.negate();
    let r = m.repair(RepairPasses::default_repair());
    assert_eq!(m, original);
    assert_eq!(
        r.fixes,
        vec![MeshFix::FlippedFaceOrientation { face_idx: 1.into() }]
    );
}
#[test]
fn indices_only_flip_negates_normal() {
    let mut m = tetra();
    let normal = m.faces[1].normal;
    m.faces[1].vertices.reverse();
    m.repair(RepairPasses::default_repair());
    assert_eq!(m.faces[1].normal, normal.negate());
}
#[test]
fn normal_only_repair_and_clean_assessment() {
    let mut m = tetra();
    m.faces[2].normal = Normal::new([0.; 3]);
    let p = RepairPasses {
        face_normals: true,
        ..Default::default()
    };
    let before = m.clone();
    assert_eq!(assess_repair(&m, p), RepairAssessment::Work);
    assert_eq!(m, before);
    let r = m.repair(p);
    assert!(
        matches!(r.fixes.as_slice(),[MeshFix::RecomputedNormal {face_idx,..}] if face_idx.get()==2)
    );
    assert_eq!(assess_repair(&m, p), RepairAssessment::Clean);
}
#[test]
fn flip_and_normal_records_order() {
    let mut m = tetra();
    m.faces[0].vertices.reverse();
    m.faces[0].normal = Normal::new([0.; 3]);
    let r = m.repair(RepairPasses {
        face_normals: true,
        ..RepairPasses::default_repair()
    });
    assert!(
        matches!(r.fixes.as_slice(),[MeshFix::FlippedFaceOrientation {..},MeshFix::RecomputedNormal {old_normal,..}] if *old_normal==Normal::new([0.;3]))
    );
    assert!(!m.diagnose().has_problems());
}
#[test]
fn orientation_stage_transactional_recompute_failure() {
    let mut m = tetra();
    m.vertices[3] = m.vertices[0];
    let before = m.clone();
    let ns = vec![
        MeshNormalVectorError::ExpectationMismatch { face_idx: 0.into() },
        MeshNormalVectorError::NonUnitNorm {
            face_idx: 1.into(),
            norm: 0.,
        },
    ];
    let mut w = m.validated_faces_mut().unwrap();
    assert_eq!(
        w.fix_faces_orientation(
            &[],
            &ns,
            FixFacesOptions {
                flipped: false,
                normals: true
            }
        ),
        Err(MeshFixError::NormalRecompute(
            GeometryError::DegenerateTriangle
        ))
    );
    assert_eq!(m, before);
}
#[test]
fn orientation_thresholds_and_excess_complaints() {
    let mut m = tetra();
    let e = MeshEdgeManifoldError::InconsistentWindingOrder {
        edge: (0.into(), 1.into()),
        face_indices: [0.into(), 1.into()],
    };
    let mut w = m.validated_faces_mut().unwrap();
    assert_eq!(
        w.fix_faces_orientation(
            &[e.clone(), e.clone()],
            &[],
            FixFacesOptions {
                flipped: true,
                normals: false
            }
        ),
        Err(MeshFixError::TooManyFlippedFaces)
    );
    assert_eq!(
        w.fix_faces_orientation(
            &vec![e; 4],
            &[],
            FixFacesOptions {
                flipped: true,
                normals: false
            }
        ),
        Err(MeshFixError::FaceWithTooManyWindingErrors { face_idx: 0.into() })
    );
}
#[test]
fn zero_area_does_not_select_normal_repair() {
    let mut m = open();
    m.vertices[2] = Vertex::new([2., 0., 0.]);
    let r = m.repair(RepairPasses {
        face_normals: true,
        ..Default::default()
    });
    assert_eq!(r.refusal, None);
    assert!(r.fixes.is_empty());
}
#[test]
fn error_sources() {
    let e = MeshFixError::from(GeometryError::DegenerateTriangle);
    assert!(e.source().is_some());
    let r = RepairRefusal::OrientationFixFailed(e);
    assert!(r.source().is_some());
    assert!(RepairRefusal::InvalidVertices(vec![]).source().is_none());
    assert_eq!(
        r.to_string(),
        "face orientation fix failed: normal recompute failed: degenerate (near-zero area) triangle"
    );
}
#[test]
fn soup_bit_identity_and_first_appearance() {
    let neg = Vertex::new([-0., 1., 2.]);
    let pos = Vertex::new([0., 1., 2.]);
    let nan1 = Vertex::new([f32::from_bits(0x7fc00001), 0., 0.]);
    let nan2 = Vertex::new([f32::from_bits(0x7fc00002), 0., 0.]);
    let m = indexed_mesh_from_soup(
        None,
        vec![
            Triangle::new(n(), [neg, nan1, pos]),
            Triangle::new(n().negate(), [nan2, nan1, neg]),
        ],
        3,
    )
    .unwrap();
    assert_eq!(m.vertices.len(), 3);
    assert_eq!(m.vertices[0][0].to_bits(), (-0f32).to_bits());
    assert_eq!(m.faces[0].vertices.map(VertexIndex::get), [0, 1, 0]);
    assert_eq!(m.faces[1].vertices.map(VertexIndex::get), [2, 1, 0]);
    assert_eq!(m.faces[1].normal, n().negate());
}
#[test]
fn soup_cap_metadata_and_no_validation() {
    let meta = Some(MeshMetadata {
        name: Some(String::new()),
        header_comment: None,
    });
    let m = indexed_mesh_from_soup(meta.clone(), vec![], 0).unwrap();
    assert_eq!(m.meta, meta);
    let t = Triangle::new(n(), [Vertex::new([0.; 3]); 3]);
    assert!(indexed_mesh_from_soup(None, vec![t], 1).is_ok());
    let e = indexed_mesh_from_soup(None, vec![t], 0).unwrap_err();
    assert_eq!(e, IndexedMeshBuildError::TooManyVertices { limit: 0 });
    assert_eq!(
        e.to_string(),
        "indexed-mesh build: unique vertex count exceeds the 0 index limit"
    );
}
#[derive(Debug, PartialEq)]
enum StreamError {
    Read,
    Build(IndexedMeshBuildError),
}
impl From<IndexedMeshBuildError> for StreamError {
    fn from(e: IndexedMeshBuildError) -> Self {
        Self::Build(e)
    }
}
struct Stream(std::vec::IntoIter<Result<Triangle, StreamError>>);
impl Iterator for Stream {
    type Item = Result<Triangle, StreamError>;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}
impl TriangleIterator for Stream {
    type Error = StreamError;
}
#[test]
fn iterator_remaining_and_first_error() {
    let t = soup(&open())[0];
    let mut s = Stream(vec![Ok(t), Err(StreamError::Read), Ok(t)].into_iter());
    assert!(s.next().unwrap().is_ok());
    assert_eq!(s.as_indexed_mesh(None), Err(StreamError::Read));
    assert_eq!(s.as_indexed_mesh(None).unwrap().faces.len(), 1);
}
#[test]
fn topology_snapshot_edges_and_fans() {
    let mut m = tetra();
    let topology = MeshTopology::from_edge_manifold(
        &m.validated_faces()
            .unwrap()
            .validated_edge_manifold()
            .unwrap(),
    );
    assert_eq!(topology.edge_count(), 6);
    assert_eq!(
        topology.edge_faces(0.into(), 1.into()),
        topology.edge_faces(1.into(), 0.into())
    );
    assert_eq!(topology.edge_faces(1.into(), 99.into()), None);
    assert!(topology.vertex_fans(99.into()).is_none());
    for v in 0..4 {
        let fans: Vec<_> = topology.vertex_fans(v.into()).unwrap().collect();
        assert_eq!(fans.len(), 1);
        assert_eq!(fans[0].len(), 3);
        for i in 0..3 {
            let a = &m.faces[fans[0][i].get()];
            let b = &m.faces[fans[0][(i + 1) % 3].get()];
            assert_eq!(
                a.vertices.iter().filter(|v| b.vertices.contains(v)).count(),
                2
            );
        }
    }
    m.faces.clear();
    assert_eq!(topology.edge_count(), 6);
}
#[test]
fn topology_pinch_fans() {
    let a = tetra();
    let mut b = a.clone();
    for v in &mut b.vertices {
        *v = Vertex::new(v.coords().map(|x| -x));
    }
    let mut m = combine(&a, &b);
    for f in &mut m.faces[4..] {
        for v in &mut f.vertices {
            if v.get() == 4 {
                *v = 0.into();
            }
        }
    }
    closed(&m);
    let top = MeshTopology::from_edge_manifold(
        &m.validated_faces()
            .unwrap()
            .validated_edge_manifold()
            .unwrap(),
    );
    assert_eq!(top.vertex_fans(0.into()).unwrap().count(), 2);
    let before = m.clone();
    assert!(m.repair(RepairPasses::default_repair()).fixes.is_empty());
    assert_eq!(m, before);
}
#[test]
fn duplicate_same_winding_shell_separated() {
    let mut m = tetra();
    m.faces.extend(m.faces.clone());
    let r = m.repair(RepairPasses::default_repair());
    assert_eq!(r.refusal, None);
    assert!(r.edge_manifold_after, "{r:?}");
    assert_eq!(m.faces.len(), 8);
    assert_eq!(m.vertices.len(), 8);
    assert_eq!(
        r.fixes
            .iter()
            .filter(|f| matches!(f, MeshFix::DetachedSharedVertex { .. }))
            .count(),
        4
    );
}
#[test]
fn opposite_duplicate_cancellation_earliest_first() {
    let mut m = tetra();
    let original = m.clone();
    let mut opposite = m.faces.clone();
    for f in &mut opposite {
        f.vertices.rotate_left(1);
        f.vertices.reverse();
    }
    m.faces.extend(opposite);
    m.faces.extend(original.faces.clone());
    let r = m.repair(RepairPasses {
        separate_shells: true,
        ..Default::default()
    });
    assert_eq!(m.faces, original.faces);
    assert_eq!(
        r.fixes,
        (0..8)
            .map(|i| MeshFix::RemovedDuplicateFace { face_idx: i.into() })
            .collect::<Vec<_>>()
    );
    assert!(r.edge_manifold_after);
}
#[test]
fn opposite_duplicates_not_cancelled_without_overload() {
    let mut m = open();
    let mut f = m.faces[0];
    f.vertices.reverse();
    m.faces.push(f);
    let before = m.clone();
    m.repair(RepairPasses::default_repair());
    assert_eq!(m, before);
}
#[test]
fn simplify_setting_and_callback_precedence() {
    let mut m = open();
    m.vertices[0][0] = f32::NAN;
    assert_eq!(
        simplify_with_progress(&m, 0, 0., &mut |_| false).unwrap_err(),
        SimplifyError::Cancelled
    );
    for (target, d) in [
        (3, 1.),
        (4, 0.),
        (4, -1.),
        (4, f64::INFINITY),
        (4, f64::NAN),
    ] {
        assert_eq!(
            simplify(&m, target, d).unwrap_err(),
            SimplifyError::InvalidSettings
        );
    }
    assert_eq!(simplify(&m, 4, 1.).unwrap_err(), SimplifyError::InvalidMesh);
}
#[test]
fn simplify_no_reduction_compaction_order_and_normals() {
    let mut m = tetra();
    m.vertices.push(Vertex::new([99.; 3]));
    m.meta = Some(MeshMetadata {
        name: Some("shape".into()),
        header_comment: None,
    });
    for f in &mut m.faces {
        f.normal = Normal::new([0.; 3]);
    }
    let original = m.clone();
    let r = simplify(&m, 4, 0.1).unwrap();
    assert_eq!(m, original);
    assert_eq!(r.mesh.meta, m.meta);
    assert_eq!(r.mesh.vertices.len(), 4);
    assert_eq!(r.mesh.vertices[1], m.vertices[2]);
    assert!(!r.mesh.diagnose().has_problems());
    assert!(r.deviation_bound_mm < 1e-12);
}
#[test]
fn simplify_collinear_and_open_input() {
    let mut m = open();
    m.vertices[2] = Vertex::new([2., 0., 0.]);
    let r = simplify(&m, 4, 1.).unwrap();
    assert_eq!(r.mesh.faces[0].normal, n());
    assert_eq!(r.mesh.faces.len(), 1);
}
#[test]
fn simplify_final_callback_can_cancel() {
    let mut values = Vec::new();
    let r = simplify_with_progress(&tetra(), 4, 0.1, &mut |p| {
        values.push(p);
        p != 1.
    });
    assert_eq!(r.unwrap_err(), SimplifyError::Cancelled);
    assert_eq!(values, vec![0., 1.]);
}

// Construct an independently triangulated cube surface and weld through the public soup API.
fn grid_cube(divisions: usize) -> IndexedMesh {
    let mut triangles = Vec::new();
    for axis in 0..3 {
        for sign in [-1f32, 1.] {
            for i in 0..divisions {
                for j in 0..divisions {
                    let point = |u: usize, v: usize| {
                        let mut p = [0.; 3];
                        p[axis] = sign * 10.;
                        p[(axis + 1) % 3] = -10. + 20. * u as f32 / divisions as f32;
                        p[(axis + 2) % 3] = -10. + 20. * v as f32 / divisions as f32;
                        Vertex::new(p)
                    };
                    let p = [
                        point(i, j),
                        point(i + 1, j),
                        point(i + 1, j + 1),
                        point(i, j + 1),
                    ];
                    for mut f in [[p[0], p[1], p[2]], [p[0], p[2], p[3]]] {
                        if sign < 0. {
                            f.reverse();
                        }
                        triangles.push(Triangle::new(
                            compute_normal_from_triangle_vertices(&f).unwrap(),
                            f,
                        ));
                    }
                }
            }
        }
    }
    indexed_mesh_from_soup(None, triangles, u32::MAX as usize).unwrap()
}
fn sphere(divisions: usize) -> IndexedMesh {
    let mut m = grid_cube(divisions);
    for v in &mut m.vertices {
        let p = v.to_f64();
        let length = p.norm();
        *v = Vertex::new(p.coords().map(|x| (x * 10. / length) as f32));
    }
    recalc(&mut m);
    m
}
#[test]
fn simplify_subdivided_cube_reduction_and_planes() {
    let m = grid_cube(40);
    let start = std::time::Instant::now();
    let r = simplify(&m, 12, 0.02).unwrap();
    eprintln!(
        "cube: {} -> {} faces, bound {} mm, {:?}",
        m.faces.len(),
        r.mesh.faces.len(),
        r.deviation_bound_mm,
        start.elapsed()
    );
    assert!(
        r.mesh.faces.len() * 20 < m.faces.len(),
        "{} -> {}",
        m.faces.len(),
        r.mesh.faces.len()
    );
    closed(&r.mesh);
    assert!(r.deviation_bound_mm <= 0.02);
    for v in &r.mesh.vertices {
        assert!(v.iter().all(|x| x.abs() <= 10.02));
        assert!(v.iter().any(|x| (x.abs() - 10.).abs() < 0.02));
    }
    for f in &r.mesh.faces {
        let v = f.vertices.map(|v| r.mesh.vertices[usize::from(v)]);
        let center = std::array::from_fn::<_, 3, _>(|i| (v[0][i] + v[1][i] + v[2][i]) / 3.);
        assert!(
            f.normal
                .to_f64()
                .dot_product(&Vector3D::new(center.map(f64::from)))
                > 0.
        );
    }
}
#[test]
fn simplify_sphere_closed_outward_and_deterministic() {
    let m = sphere(8);
    let a = simplify(&m, 200, 0.08).unwrap();
    eprintln!(
        "sphere: {} -> {} faces, bound {} mm",
        m.faces.len(),
        a.mesh.faces.len(),
        a.deviation_bound_mm
    );
    let b = simplify(&m, 200, 0.08).unwrap();
    assert!(a.mesh.faces.len() < m.faces.len());
    assert_eq!(a.mesh, b.mesh);
    assert_eq!(a.deviation_bound_mm, b.deviation_bound_mm);
    assert!(a.deviation_bound_mm <= 0.08);
    closed(&a.mesh);
    for f in &a.mesh.faces {
        assert!(
            f.normal
                .to_f64()
                .dot_product(&a.mesh.vertices[usize::from(f.vertices[0])].to_f64())
                > 0.
        );
    }
}
#[test]
fn simplify_cancels_during_work_without_mutation() {
    let m = sphere(10);
    let before = m.clone();
    let mut saw_reduction = false;
    let r = simplify_with_progress(&m, 4, 0.2, &mut |value| {
        if value > 0. && value < 1. {
            saw_reduction = true;
            false
        } else {
            true
        }
    });
    assert_eq!(r.unwrap_err(), SimplifyError::Cancelled);
    assert!(saw_reduction);
    assert_eq!(m, before);
}
#[test]
fn simplify_fixed_open_boundary() {
    let mut m = grid_cube(5);
    m.faces.truncate(m.faces.len() - 2);
    let before = m.clone();
    let r = simplify(&m, 4, 0.1).unwrap();
    let edges = before
        .validated_faces()
        .unwrap()
        .validate_edge_manifold()
        .unwrap_err();
    for e in edges {
        if let MeshEdgeManifoldError::BoundaryEdge { edge, .. } = e {
            for v in [edge.0, edge.1] {
                assert!(r.mesh.vertices.contains(&before.vertices[usize::from(v)]));
            }
        }
    }
}

proptest! {
    #[test] fn soup_roundtrip_order_bits(indices in prop::collection::vec(0u32..20,0..200)) {let triangles:Vec<_>=indices.chunks(3).map(|chunk| {let v=std::array::from_fn(|i| Vertex::new([chunk[i%chunk.len()] as f32,0.,0.]));Triangle::new(n(),v)}).collect();let m=indexed_mesh_from_soup(None,triangles.clone(),20).unwrap();prop_assert_eq!(soup(&m),triangles);let mut seen=Vec::new();for t in soup(&m) {for v in t.vertices() {if !seen.contains(&v) {seen.push(v);}}}prop_assert_eq!(m.vertices,seen);}
    #[test] fn transformed_bounds_enclose_corners(min in prop::array::uniform3(-100f32..0.),extent in prop::array::uniform3(0f32..100.),matrix in prop::array::uniform3(prop::array::uniform3(-3f64..3.)),translation in prop::array::uniform3(-10f64..10.)) {let max=std::array::from_fn(|i| min[i]+extent[i]);let t=AffineTransform3D::from_linear_and_translation(matrix,translation);let a=Aabb::from_min_max(Vertex::new(min),Vertex::new(max)).transformed_arvo(&t);for bits in 0..8 {let p=Vertex::new(std::array::from_fn(|i| if bits&(1<<i)==0 {min[i]} else {max[i]}));let p=t.apply_to_vertex(p);for i in 0..3 {prop_assert!(p[i]>=a.min()[i]-0.0001 && p[i]<=a.max()[i]+0.0001);}}}
    #[test] fn finite_triangle_normals_are_unit(x in 0.001f32..1000.,y in 0.001f32..1000.) {let v=[Vertex::new([0.;3]),Vertex::new([x,0.,0.]),Vertex::new([0.,y,0.])];prop_assert_eq!(compute_normal_from_triangle_vertices(&v).unwrap(),n());}
}

#[test]
fn union_empty_is_identity() {
    let a = Aabb::from_min_max(Vertex::new([1., 2., 3.]), Vertex::new([4., 5., 6.]));
    let mut b = a;
    b.union(&Aabb::empty());
    assert_eq!(a, b);
    let mut b = Aabb::empty();
    b.union(&a);
    assert_eq!(a, b);
}
#[test]
fn two_cubes_shared_edge_detach_in_vertex_order() {
    let a = grid_cube(1);
    let mut b = a.clone();
    for v in &mut b.vertices {
        v[0] += 20.;
        v[1] += 20.;
    }
    let mut triangles = soup(&a);
    triangles.extend(soup(&b));
    let mut m = indexed_mesh_from_soup(None, triangles, 100).unwrap();
    let original = m.clone();
    let mut shared: Vec<_> = m
        .vertices
        .iter()
        .enumerate()
        .filter(|(_, v)| v[0] == 10. && v[1] == 10.)
        .map(|(i, _)| VertexIndex::from(i as u32))
        .collect();
    shared.sort_unstable();
    let r = m.repair(RepairPasses::default_repair());
    assert_eq!(r.refusal, None);
    assert!(r.edge_manifold_after, "{r:?}");
    assert_eq!(m.faces.len(), 24);
    assert_eq!(m.vertices.len(), 16);
    assert_eq!(
        r.fixes,
        shared
            .iter()
            .map(|&vertex| MeshFix::DetachedSharedVertex { vertex })
            .collect::<Vec<_>>()
    );
    for (old, new) in original.faces.iter().zip(&m.faces) {
        assert_eq!(old.normal, new.normal);
        assert_eq!(
            old.vertices.map(|v| original.vertices[usize::from(v)]),
            new.vertices.map(|v| m.vertices[usize::from(v)])
        );
    }
    let mut again = original;
    let r2 = again.repair(RepairPasses::default_repair());
    assert_eq!(r, r2);
    assert_eq!(m, again);
}
#[test]
fn cubes_shared_wall_cancels_opposite_triangles() {
    let a = grid_cube(1);
    let mut b = a.clone();
    for v in &mut b.vertices {
        v[0] += 20.;
    }
    let mut triangles = soup(&a);
    triangles.extend(soup(&b));
    let mut m = indexed_mesh_from_soup(None, triangles, 100).unwrap();
    let r = m.repair(RepairPasses::default_repair());
    assert_eq!(r.refusal, None);
    assert!(r.edge_manifold_after, "{r:?}");
    assert_eq!(m.faces.len(), 20);
    assert_eq!(m.vertices.len(), 12);
    let indices: Vec<_> = r
        .fixes
        .iter()
        .map(|f| match f {
            MeshFix::RemovedDuplicateFace { face_idx } => face_idx.get(),
            _ => panic!("unexpected {f:?}"),
        })
        .collect();
    assert_eq!(indices.len(), 4);
    assert!(indices.windows(2).all(|w| w[0] < w[1]));
}
#[test]
fn empty_repair_inclusive_flip_threshold() {
    let mut m = IndexedMesh::new(None, vec![], vec![]);
    let r = m.repair(RepairPasses::default_repair());
    assert_eq!(
        r.refusal,
        Some(RepairRefusal::OrientationFixFailed(
            MeshFixError::TooManyFlippedFaces
        ))
    );
    assert!(!r.edge_manifold_after);
    assert_eq!(m.repair(RepairPasses::default()).refusal, None);
}
#[test]
fn zero_length_primitive_logs_each_supplied_error() {
    let mut m = tetra();
    m.faces.insert(0, face([0, 0, 0]));
    let e = m.validate_faces().unwrap_err();
    let (fixes, w) = m.fix_zero_length_edges_into_validated(&e);
    assert!(w.is_ok());
    assert_eq!(
        fixes,
        vec![MeshFix::RemovedZeroLengthEdgeFace { face_idx: 0.into() }; 3]
    );
    assert_eq!(m, tetra());
}
#[test]
fn ordinary_error_displays() {
    let edge = (VertexIndex::from(1), VertexIndex::from(7));
    assert_eq!(
        MeshEdgeManifoldError::EmptyMesh.to_string(),
        "mesh has no faces"
    );
    assert_eq!(
        MeshEdgeManifoldError::BoundaryEdge {
            edge,
            face_idx: 3.into()
        }
        .to_string(),
        "edge (#1, #7) is shared by only face #3 (a manifold edge must be shared by exactly 2 faces)"
    );
    assert_eq!(
        MeshEdgeManifoldError::NonManifoldEdge {
            edge,
            face_indices: vec![1.into(), 2.into(), 3.into()]
        }
        .to_string(),
        "edge (#1, #7) is shared by 3 faces (a manifold edge must be shared by exactly 2)"
    );
    assert_eq!(
        MeshEdgeManifoldError::InconsistentWindingOrder {
            edge,
            face_indices: [2.into(), 3.into()]
        }
        .to_string(),
        "edge (#1, #7) is traversed in the same direction by faces #2 and #3 (windings disagree)"
    );
    assert_eq!(
        MeshNormalVectorError::ZeroAreaTriangle { face_idx: 5.into() }.to_string(),
        "face #5 is degenerate: zero area"
    );
    assert_eq!(
        MeshNormalVectorError::NonUnitNorm {
            face_idx: 2.into(),
            norm: 2.
        }
        .to_string(),
        "face #2 has a non-unit normal vector (norm = 2)"
    );
    assert_eq!(
        MeshNormalVectorError::ExpectationMismatch { face_idx: 1.into() }.to_string(),
        "face #1 declares a normal vector that disagrees with the triangle plane"
    );
    assert_eq!(
        MeshFixError::TooManyFlippedFaces.to_string(),
        "too many flipped faces"
    );
    assert_eq!(
        MeshFixError::FaceWithTooManyWindingErrors { face_idx: 3.into() }.to_string(),
        "face 3 has more than 3 winding-order errors"
    );
    assert_eq!(
        RepairRefusal::InvalidVertices(vec![]).to_string(),
        "mesh has 0 invalid vertex coordinates"
    );
    assert_eq!(
        RepairRefusal::UnfixableFaceErrors(vec![]).to_string(),
        "0 face errors the enabled repair passes cannot address"
    );
    assert_eq!(
        TransformError::SingularLinear.to_string(),
        "linear part of the transform is singular"
    );
    assert_eq!(
        TransformError::ZeroMagnitudeNormal.to_string(),
        "transformed normal has zero magnitude"
    );
    assert_eq!(SimplifyError::Cancelled.to_string(), "Cancelled");
    assert_eq!(
        SimplifyError::InvalidSettings.to_string(),
        "InvalidSettings"
    );
    assert_eq!(SimplifyError::InvalidMesh.to_string(), "InvalidMesh");
}
#[test]
fn affine_arbitrary_shear_and_vector_translation() {
    let t = AffineTransform3D::from_linear_and_translation(
        [[1., 2., 3.], [0., 2., 1.], [0., 0., 3.]],
        [4., 5., 6.],
    );
    let v = Vertex::new([2., 3., 4.]);
    let p = t.apply_to_vertex(v);
    assert_eq!(p.coords(), [24., 15., 18.]);
    assert_eq!(
        t.apply_to_vector(Vector3D::new(v.coords())).coords(),
        [20., 10., 12.]
    );
    let inverse = t.inverse().unwrap();
    let p = inverse.apply_to_vertex(p);
    for i in 0..3 {
        assert!((p[i] - v[i]).abs() < 1e-6);
    }
    let (min, max) = aabb::arvo_transformed_aabb(
        [0f32; 3],
        [1f32; 3],
        &[[1., -2., 0.], [0., 1., 3.], [2., 0., -1.]],
        &[1., 2., 3.],
    );
    assert_eq!(min, [-1., 2., 2.]);
    assert_eq!(max, [2., 6., 5.]);
}
#[test]
fn simplify_cancellation_after_candidate_batch() {
    let m = sphere(60);
    let mut calls = 0;
    assert_eq!(
        simplify_with_progress(&m, 4, 0.05, &mut |_| {
            calls += 1;
            calls < 3
        })
        .unwrap_err(),
        SimplifyError::Cancelled
    );
    assert_eq!(calls, 3);
}
#[test]
fn simplify_preserves_overloaded_edge_endpoints() {
    let mut m = grid_cube(5);
    m.faces.push(m.faces[0]);
    let fixed = m.faces[0].vertices.map(|v| m.vertices[usize::from(v)]);
    let r = simplify(&m, 4, 0.1).unwrap();
    for v in fixed {
        assert!(r.mesh.vertices.contains(&v));
    }
}
#[test]
fn metadata_absent_empty_and_clone_are_distinct() {
    let mut m = tetra();
    assert_eq!(m.meta, None);
    let meta = Some(MeshMetadata {
        name: Some(String::new()),
        header_comment: Some("comment".into()),
    });
    m.meta = meta.clone();
    assert_eq!(m.meta, meta);
    assert_ne!(meta, Some(MeshMetadata::default()));
    assert_eq!(m.faces(), m.faces.as_slice());
    assert_eq!(m.vertices(), m.vertices.as_slice());
}

#[test]
#[ignore = "synthetic large-mesh timing probe; run explicitly with --nocapture"]
fn large_mesh_scaling_probe() {
    for divisions in [100, 200] {
        let build_start = std::time::Instant::now();
        let m = grid_cube(divisions);
        let indexing = build_start.elapsed();
        let start = std::time::Instant::now();
        let r = simplify(&m, 12, 0.02).unwrap();
        let elapsed = start.elapsed();
        closed(&r.mesh);
        assert!(r.mesh.faces.len() * 20 < m.faces.len());
        assert!(r.deviation_bound_mm <= 0.02);
        eprintln!(
            "large cube: {} -> {} faces; soup construction/indexing {:?}; simplification {:?}; deviation {} mm",
            m.faces.len(),
            r.mesh.faces.len(),
            indexing,
            elapsed,
            r.deviation_bound_mm
        );
    }
}

#[test]
fn overloaded_wedges_pair_by_angular_nesting() {
    let mut vertices = vec![Vertex::new([0., 0., -1.]), Vertex::new([0., 0., 1.])];
    for angle in [-2f32, -1., 0., 0.2, 1., 2.] {
        vertices.push(Vertex::new([-angle.sin(), angle.cos(), 0.]));
    }
    let faces = (0..6)
        .map(|i| {
            if i < 3 {
                face([1, 0, i + 2])
            } else {
                face([0, 1, i + 2])
            }
        })
        .collect();
    let mut m = IndexedMesh::new(None, vertices, faces);
    let r = m.repair(RepairPasses {
        separate_shells: true,
        ..Default::default()
    });
    assert_eq!(r.refusal, None);
    assert!(!r.edge_manifold_after);
    let edge = |i: usize| {
        let f = m.faces[i];
        let a = if i < 3 { f.vertices[1] } else { f.vertices[0] };
        let b = if i < 3 { f.vertices[0] } else { f.vertices[1] };
        (a, b)
    };
    assert_eq!(edge(0), edge(5));
    assert_eq!(edge(1), edge(4));
    assert_eq!(edge(2), edge(3));
    assert_ne!(edge(0), edge(1));
    assert_ne!(edge(1), edge(2));
}

#[test]
fn unmatched_overloaded_directions_remain_open() {
    let mut m = open();
    m.vertices.push(Vertex::new([0., -1., 0.]));
    m.vertices.push(Vertex::new([0., 0., 1.]));
    m.faces = vec![face([0, 1, 2]), face([0, 1, 3]), face([0, 1, 4])];
    let r = m.repair(RepairPasses {
        separate_shells: true,
        ..Default::default()
    });
    assert_eq!(r.refusal, None);
    assert!(!r.edge_manifold_after);
    assert_eq!(m.faces.len(), 3);
    assert_eq!(m.vertices.len(), 9);
}

#[test]
fn topology_edge_lookup_contract() {
    assert_eq!(edge_key(7.into(), 2.into()), (2.into(), 7.into()));
    assert_eq!(edge_key(2.into(), 7.into()), (2.into(), 7.into()));
    assert_eq!(edge_key(2.into(), 2.into()), (2.into(), 2.into()));
    let mut mesh = tetra();
    mesh.vertices.push(Vertex::new([5., 5., 5.]));
    let topology = MeshTopology::from_edge_manifold(
        &mesh
            .validated_faces()
            .unwrap()
            .validated_edge_manifold()
            .unwrap(),
    );
    assert_eq!(
        topology.edge_faces(0.into(), 1.into()),
        Some([0.into(), 1.into()])
    );
    assert_eq!(
        topology.edge_faces(1.into(), 0.into()),
        Some([0.into(), 1.into()])
    );
    assert_eq!(topology.edge_faces(0.into(), 0.into()), None);
    assert!(topology.vertex_fans(4.into()).is_none());
}

#[test]
fn error_groups_preserve_first_seen_order_and_membership() {
    struct Sample(&'static str, u8);
    impl MeshErrorReport for Sample {
        fn variant_name(&self) -> &'static str {
            self.0
        }
        fn example_face(&self) -> Option<FaceIndex> {
            None
        }
    }
    assert!(group_errors_by_variant::<Sample>(&[]).is_empty());
    let errors = [
        Sample("b", 1),
        Sample("a", 2),
        Sample("b", 3),
        Sample("c", 4),
        Sample("a", 5),
    ];
    let groups = group_errors_by_variant(&errors);
    assert_eq!(
        groups.iter().map(|g| g.variant_name).collect::<Vec<_>>(),
        ["b", "a", "c"]
    );
    assert_eq!(
        groups
            .iter()
            .map(|g| g.members.iter().map(|e| e.1).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        [vec![1, 3], vec![2, 5], vec![4]]
    );
}

#[test]
fn validation_reports_all_coordinates_and_face_edges_in_order() {
    let mut m = open();
    m.vertices = vec![Vertex::new([f32::NAN, f32::INFINITY, f32::NEG_INFINITY])];
    m.faces = vec![face([2, 2, 2]), face([0, 0, 0])];
    let vertices = m.validate_vertices().unwrap_err();
    assert_eq!(vertices.len(), 3);
    assert!(matches!(
        vertices[0],
        MeshVertexError::NaNCoordinate {
            coord: CoordAxis::X,
            ..
        }
    ));
    assert!(matches!(
        vertices[1],
        MeshVertexError::InfiniteCoordinate {
            coord: CoordAxis::Y,
            ..
        }
    ));
    assert!(matches!(
        vertices[2],
        MeshVertexError::InfiniteCoordinate {
            coord: CoordAxis::Z,
            ..
        }
    ));
    let boundary = IndexedMesh::new(None, vec![Vertex::new([0., 0., 0.])], vec![face([0, 1, 0])]);
    assert!(matches!(
        boundary.validate_faces().unwrap_err()[0],
        MeshFaceError::OutOfRangeVertexIndex {
            vertex_pos: TriVertexSlot::V1,
            ..
        }
    ));
    let faces = m.validate_faces().unwrap_err();
    assert_eq!(faces.len(), 9);
    for error in &faces[..3] {
        assert!(matches!(error, MeshFaceError::OutOfRangeVertexIndex { .. }));
    }
    for error in &faces[3..] {
        assert!(matches!(error, MeshFaceError::ZeroLengthEdge { .. }));
    }
    assert!(matches!(
        faces[5],
        MeshFaceError::ZeroLengthEdge {
            vertex_pos_a: TriVertexSlot::V2,
            vertex_pos_b: TriVertexSlot::V0,
            ..
        }
    ));
    assert!(
        matches!(faces[6], MeshFaceError::ZeroLengthEdge { face_idx, .. } if face_idx.get() == 1)
    );
}

#[test]
fn aabb_empty_axis_and_unordered_contract() {
    use crate::aabb::Aabb;
    assert!(Aabb::empty().is_empty());
    assert!(!Aabb::from_min_max(Vertex::new([1.0; 3]), Vertex::new([1.0; 3])).is_empty());
    for axis in 0..3 {
        let mut min = [0.0; 3];
        min[axis] = f32::INFINITY;
        assert!(Aabb::from_min_max(Vertex::new(min), Vertex::new([1.0; 3])).is_empty());
        min[axis] = f32::NAN;
        assert!(!Aabb::from_min_max(Vertex::new(min), Vertex::new([1.0; 3])).is_empty());
    }
}

#[test]
fn manifold_error_example_face_contract() {
    let edge = (0.into(), 1.into());
    let examples = [
        (MeshEdgeManifoldError::EmptyMesh, None),
        (MeshEdgeManifoldError::BoundaryEdge { edge, face_idx: 7.into() }, Some(7.into())),
        (MeshEdgeManifoldError::NonManifoldEdge { edge, face_indices: vec![] }, None),
        (MeshEdgeManifoldError::NonManifoldEdge { edge, face_indices: vec![8.into(), 9.into()] }, Some(8.into())),
        (MeshEdgeManifoldError::InconsistentWindingOrder { edge, face_indices: [10.into(), 11.into()] }, Some(10.into())),
    ];
    for (error, expected) in examples {
        assert_eq!(error.example_face(), expected);
    }
}
