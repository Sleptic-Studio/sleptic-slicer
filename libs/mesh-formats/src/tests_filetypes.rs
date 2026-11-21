// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::filetypes::{
    MeshFileType as M, WriteFormat as W, infer_mesh_file_format_from_extension,
};
use std::path::Path;
#[test]
fn names_extensions_and_write_targets() {
    for (names, kind, target) in [
        (&["stl"][..], M::Stl, Some(W::StlBin)),
        (&["stla", "stl_ascii"][..], M::StlAscii, Some(W::StlAscii)),
        (&["stlb", "stl_bin"][..], M::StlBin, Some(W::StlBin)),
        (&["obj"][..], M::Obj, Some(W::Obj)),
        (&["ply"][..], M::Ply, None),
        (&["amf"][..], M::Amf, None),
        (&["3mf"][..], M::_3mf, Some(W::ThreeMf)),
    ] {
        for name in names {
            for spelling in [name.to_string(), name.to_ascii_uppercase()] {
                assert_eq!(spelling.parse::<M>(), Ok(kind));
                let path = format!("part.{spelling}");
                let inferred = infer_mesh_file_format_from_extension(Path::new(&path));
                if name.contains('_') {
                    assert!(inferred.is_err());
                } else {
                    assert_eq!(inferred, Ok(kind));
                }
            }
        }
        assert_eq!(W::try_from(kind).ok(), target);
    }
    for name in ["", " stl", "stl ", "stl-ascii", "unknown", "?tl"] {
        assert!(name.parse::<M>().is_err(), "{name}");
    }
    for path in [
        "part",
        "part.",
        ".stl",
        "part.stl_ascii",
        "part.stl_bin",
        "part.unknown",
    ] {
        let error = infer_mesh_file_format_from_extension(Path::new(path)).unwrap_err();
        assert!(
            matches!(error, crate::error::InferFileTypeError::UnknownExtension { path: p } if p == Path::new(path))
        );
    }
}
