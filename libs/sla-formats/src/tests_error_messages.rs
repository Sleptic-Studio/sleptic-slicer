// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Every `WriterError` message, word for word, and which ones name a source.
use crate::{Format, WriterError};
use std::error::Error as _;

#[test]
fn writer_error_messages_are_stable() {
    let node = sc_scene::Scene::new().add_group(None, Default::default());
    let graph = sc_scene::SceneError::InvalidPlates;
    let model = sc_3mf::error::ModelWriteError::Io(std::io::Error::other("m"));
    let model_text = model.to_string();
    let package = sc_3mf::error::PackageWriteError::Io(std::io::Error::other("p"));
    let package_text = package.to_string();
    let cases: Vec<(WriterError, String, bool)> = vec![
        (WriterError::Io(std::io::Error::other("disk")), "filesystem operation failed: disk".into(), true),
        (WriterError::OutputPathInvalid { reason: "empty".into() }, "output path is invalid: empty".into(), false),
        (
            WriterError::OutputPathExistsAndIsNotADirectory { path: "out".into() },
            "output path \"out\" exists and is not a directory".into(),
            false,
        ),
        (
            WriterError::OutputPathExistsAndIsNotEmpty { path: "out".into() },
            "output directory \"out\" exists and is not empty".into(),
            false,
        ),
        (
            WriterError::EncodingFailed { format: Format::PngPerLayer, reason: "too big".into() },
            "encoder for format \"png-per-layer\" rejected the request: too big".into(),
            false,
        ),
        (
            WriterError::SceneContextRequired { format: Format::ThreeMf },
            "format \"3mf\" stores per-model slice stacks; it cannot be written from a merged contour stack alone".into(),
            false,
        ),
        (WriterError::NoModelsToWrite, "the per-leaf slices list is empty; nothing to write".into(), false),
        (
            WriterError::LayerGridMismatch { leaf: node, schedule_layers: 4, stack_layers: 3 },
            "leaf #1 carries 3 layer(s) but the layer schedule defines 4".into(),
            false,
        ),
        (WriterError::SceneBakeFailed(graph.clone()), format!("baking a leaf mesh into world space failed: {graph}"), true),
        (WriterError::ModelSerializationFailed(model), format!("serializing a 3MF model document failed: {model_text}"), true),
        (WriterError::PackageWriteFailed(package), format!("writing the 3MF package failed: {package_text}"), true),
    ];
    for (error, text, has_source) in cases {
        assert_eq!(error.to_string(), text);
        assert_eq!(error.source().is_some(), has_source, "{text}");
    }
    let wrapped = crate::error::encoding(Format::SvgCombined, "bad");
    assert_eq!(wrapped.to_string(), "encoder for format \"svg-combined\" rejected the request: bad");
}
