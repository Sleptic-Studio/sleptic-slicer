// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::{MeshEdgeManifoldError, MeshFaceError, MeshVertexError};
use crate::primitives::FaceIndex;
pub trait MeshErrorReport {
    fn variant_name(&self) -> &'static str;
    fn example_face(&self) -> Option<FaceIndex>;
}
#[derive(Debug)]
pub struct ErrorVariantGroup<'a, E> {
    pub members: Vec<&'a E>,
    pub variant_name: &'static str,
}
// Keep each diagnostic's label tied to its enum variant, so adding or renaming
// a variant requires updating this exhaustive mapping at compile time.
macro_rules! report_labels {
    ($kind:ty, $face:expr; $($pattern:pat => $label:ident),+ $(,)?) => {
        impl MeshErrorReport for $kind {
            fn variant_name(&self) -> &'static str {
                match self { $($pattern => stringify!($label)),+ }
            }
            fn example_face(&self) -> Option<FaceIndex> {
                ($face)(self)
            }
        }
    };
}

report_labels!(MeshVertexError, |_: &MeshVertexError| None;
    MeshVertexError::InfiniteCoordinate { .. } => InfiniteCoordinate,
    MeshVertexError::NaNCoordinate { .. } => NaNCoordinate,
);
report_labels!(MeshFaceError, |error: &MeshFaceError| {
    let (MeshFaceError::OutOfRangeVertexIndex { face_idx, .. }
        | MeshFaceError::ZeroLengthEdge { face_idx, .. }) = error;
    Some(*face_idx)
};
    MeshFaceError::OutOfRangeVertexIndex { .. } => OutOfRangeVertexIndex,
    MeshFaceError::ZeroLengthEdge { .. } => ZeroLengthEdge,
);
report_labels!(MeshEdgeManifoldError, |error: &MeshEdgeManifoldError| {
    match error {
        MeshEdgeManifoldError::EmptyMesh => None,
        MeshEdgeManifoldError::BoundaryEdge { face_idx, .. } => Some(*face_idx),
        MeshEdgeManifoldError::NonManifoldEdge { face_indices, .. } => face_indices.first().copied(),
        MeshEdgeManifoldError::InconsistentWindingOrder { face_indices, .. } => Some(face_indices[0]),
    }
};
    MeshEdgeManifoldError::EmptyMesh => EmptyMesh,
    MeshEdgeManifoldError::BoundaryEdge { .. } => BoundaryEdge,
    MeshEdgeManifoldError::NonManifoldEdge { .. } => NonManifoldEdge,
    MeshEdgeManifoldError::InconsistentWindingOrder { .. } => InconsistentWindingOrder,
);
pub fn group_errors_by_variant<E: MeshErrorReport>(errors: &[E]) -> Vec<ErrorVariantGroup<'_, E>> {
    let mut positions = std::collections::HashMap::new();
    let mut result: Vec<ErrorVariantGroup<'_, E>> = Vec::new();
    for item in errors {
        let name = item.variant_name();
        let slot = *positions.entry(name).or_insert_with(|| {
            result.push(ErrorVariantGroup {
                variant_name: name,
                members: Vec::new(),
            });
            result.len() - 1
        });
        result[slot].members.push(item);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sample(&'static str, u8);
    impl MeshErrorReport for Sample {
        fn variant_name(&self) -> &'static str {
            self.0
        }
        fn example_face(&self) -> Option<FaceIndex> {
            None
        }
    }

    #[test]
    fn groups_retain_first_seen_order_and_member_order() {
        let input = [
            Sample("z", 1),
            Sample("a", 2),
            Sample("z", 3),
            Sample("a", 4),
        ];
        let groups = group_errors_by_variant(&input);
        assert_eq!(
            groups.iter().map(|g| g.variant_name).collect::<Vec<_>>(),
            ["z", "a"]
        );
        assert_eq!(
            groups[0].members.iter().map(|v| v.1).collect::<Vec<_>>(),
            [1, 3]
        );
        assert_eq!(
            groups[1].members.iter().map(|v| v.1).collect::<Vec<_>>(),
            [2, 4]
        );
        assert!(group_errors_by_variant::<Sample>(&[]).is_empty());
        assert!(std::ptr::eq(groups[0].members[0], &input[0]));
    }
}
