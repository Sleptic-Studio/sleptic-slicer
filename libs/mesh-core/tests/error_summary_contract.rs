// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Public tags, witnesses and borrowed grouping are compatibility contracts.
use sc_mesh_core::indexed_mesh::{
    MeshEdgeManifoldError as Edge, MeshErrorReport, MeshFaceError as Face,
    MeshVertexError as VertexError, group_errors_by_variant,
};
use sc_mesh_core::primitives::{CoordAxis, FaceIndex, TriVertexSlot};

#[test]
fn every_builtin_tag_and_face_witness_is_stable() {
    type Case = (Box<dyn MeshErrorReport>, &'static str, Option<usize>);
    let cases: Vec<Case> = vec![
        (
            Box::new(VertexError::InfiniteCoordinate {
                vertex_idx: 6,
                coord: CoordAxis::Y,
            }),
            "InfiniteCoordinate",
            None,
        ),
        (
            Box::new(VertexError::NaNCoordinate {
                vertex_idx: 3,
                coord: CoordAxis::Z,
            }),
            "NaNCoordinate",
            None,
        ),
        (
            Box::new(Face::OutOfRangeVertexIndex {
                face_idx: 11usize.into(),
                vertex_pos: TriVertexSlot::V2,
                vertex_idx: 99u32.into(),
            }),
            "OutOfRangeVertexIndex",
            Some(11),
        ),
        (
            Box::new(Face::ZeroLengthEdge {
                face_idx: 7usize.into(),
                vertex_pos_a: TriVertexSlot::V0,
                vertex_pos_b: TriVertexSlot::V2,
                vertex_idx: 5u32.into(),
            }),
            "ZeroLengthEdge",
            Some(7),
        ),
        (Box::new(Edge::EmptyMesh), "EmptyMesh", None),
        (
            Box::new(Edge::BoundaryEdge {
                edge: (1u32.into(), 4u32.into()),
                face_idx: 12usize.into(),
            }),
            "BoundaryEdge",
            Some(12),
        ),
        (
            Box::new(Edge::NonManifoldEdge {
                edge: (1u32.into(), 4u32.into()),
                face_indices: vec![8usize.into(), 2usize.into(), 5usize.into()],
            }),
            "NonManifoldEdge",
            Some(8),
        ),
        (
            Box::new(Edge::NonManifoldEdge {
                edge: (1u32.into(), 4u32.into()),
                face_indices: vec![],
            }),
            "NonManifoldEdge",
            None,
        ),
        (
            Box::new(Edge::InconsistentWindingOrder {
                edge: (1u32.into(), 4u32.into()),
                face_indices: [9usize.into(), 1usize.into()],
            }),
            "InconsistentWindingOrder",
            Some(9),
        ),
    ];
    for (error, tag, face) in cases {
        assert_eq!(error.variant_name(), tag);
        assert_eq!(error.example_face().map(FaceIndex::get), face, "{tag}");
    }
}

#[derive(Debug)]
struct Finding {
    tag: &'static str,
    payload: usize,
}
impl MeshErrorReport for Finding {
    fn variant_name(&self) -> &'static str {
        self.tag
    }
    fn example_face(&self) -> Option<FaceIndex> {
        panic!("Grouping uses the tag and does not request a face witness")
    }
}

#[test]
fn generic_grouping_preserves_first_seen_tags_and_exact_member_references() {
    let input = ["zeta", "beta", "alpha", "zeta", "beta", "zeta"]
        .into_iter()
        .enumerate()
        .map(|(payload, tag)| Finding { tag, payload })
        .collect::<Vec<_>>();
    let grouped = group_errors_by_variant(&input);
    let expected = [
        ("zeta", vec![0, 3, 5]),
        ("beta", vec![1, 4]),
        ("alpha", vec![2]),
    ];
    assert_eq!(grouped.len(), expected.len());
    for (group, (tag, positions)) in grouped.iter().zip(expected) {
        assert_eq!(group.variant_name, tag);
        assert_eq!(group.members.len(), positions.len());
        for (member, index) in group.members.iter().zip(positions) {
            assert_eq!(member.payload, index);
            assert!(std::ptr::eq(*member, &input[index]));
        }
    }
}

#[test]
fn empty_single_and_repeated_inputs_do_not_invent_or_deduplicate_findings() {
    assert!(group_errors_by_variant::<Finding>(&[]).is_empty());
    let input = [Finding {
        tag: "one",
        payload: 42,
    }];
    let grouped = group_errors_by_variant(&input);
    assert_eq!(grouped.len(), 1);
    assert_eq!(grouped[0].variant_name, "one");
    assert_eq!(grouped[0].members.len(), 1);
    assert!(std::ptr::eq(grouped[0].members[0], &input[0]));
    let input = (0..257)
        .map(|_| Finding {
            tag: "repeat",
            payload: 42,
        })
        .collect::<Vec<_>>();
    let grouped = group_errors_by_variant(&input);
    assert_eq!(grouped.len(), 1);
    assert_eq!(grouped[0].members.len(), input.len());
    for (actual, expected) in grouped[0].members.iter().zip(&input) {
        assert!(std::ptr::eq(*actual, expected));
    }
}
