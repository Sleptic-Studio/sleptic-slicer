// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::{
    args::cli_parser,
    documents::Document,
    json_catalog::{
        JsonCatalogError, JsonCatalogReader, MaterialConflict, Problem, RecordProblem,
    },
    library::save,
    options::{ConversionRequest, RawCliArgs},
    pipeline::convert,
};
use serde_json::{Value, json};
use std::{error::Error, fs, path::Path};

fn contracts() -> Vec<Value> {
    serde_json::from_str(include_str!("../tests/fixtures/contract.json")).unwrap()
}
fn sample(name: &str) -> Value {
    contracts().into_iter().find(|v| v["name"] == name).unwrap()["input"][0].clone()
}
fn read(records: Value) -> Result<Vec<Document>, JsonCatalogError> {
    let scratch = tempfile::tempdir().unwrap();
    let input = scratch.path().join("input.json");
    fs::write(&input, serde_json::to_vec(&records).unwrap()).unwrap();
    JsonCatalogReader::new().read(&[&input])
}
fn printer_id(profile: &Document) -> &str {
    match profile {
        Document::Printer { profile, .. } => profile.identity.id.as_str(),
        _ => panic!("printer"),
    }
}

#[test]
fn baseline_contracts_keep_bytes_and_counts() {
    for contract in contracts() {
        let profiles = read(contract["input"].clone()).unwrap();
        let output = tempfile::tempdir().unwrap();
        let report = save(&profiles, output.path(), false).unwrap();
        assert_eq!(
            json!([report.printers, report.materials, report.images]),
            contract["counts"],
            "{}",
            contract["name"]
        );
        for (name, expected) in contract["files"].as_object().unwrap() {
            let actual = fs::read_to_string(output.path().join(name)).unwrap();
            assert_eq!(
                actual,
                expected.as_str().unwrap(),
                "{}: {name}",
                contract["name"]
            );
        }
    }
}

#[test]
fn requests_and_cli_flags_preserve_order_and_images() {
    let args = cli_parser()
        .run_inner(&[
            "--format=json",
            "-i",
            "first.json",
            "--input=second.json",
            "-o",
            "output",
            "--with-images",
        ])
        .unwrap();
    let request = ConversionRequest::from_raw(args).unwrap();
    assert_eq!(
        request
            .inputs()
            .iter()
            .map(|p| p.as_path())
            .collect::<Vec<_>>(),
        [Path::new("first.json"), Path::new("second.json")]
    );
    assert_eq!(request.out(), Path::new("output"));
    assert!(request.with_images());
    assert!(request.summary().contains("images: on"));
    assert_eq!(request.format(), "json");
    assert!(
        cli_parser()
            .run_inner(&["-f", "JsonCatalog", "-o", "output"])
            .is_err()
    );
    assert!(
        ConversionRequest::from_raw(RawCliArgs {
            format: "json".into(),
            inputs: vec![],
            out: "output".into(),
            with_images: false
        })
        .is_err()
    );
    assert!(
        cli_parser()
            .run_inner(&["-f", "unknown", "-o", "output"])
            .is_err()
    );
}

#[test]
fn geometry_combines_explicit_and_derived_axes_and_mirrors() {
    let mut source = sample("printer");
    source["buildVolumeMm"]
        .as_object_mut()
        .unwrap()
        .remove("depth");
    source["pixelSize"] = json!({"x": -100, "y": 50});
    source["display"]["mirrorX"] = json!(true);
    source["display"]["mirrorY"] = json!(true);
    let profiles = read(json!([source])).unwrap();
    let Document::Printer { profile: p, .. } = &profiles[0] else {
        panic!("printer")
    };
    let hardware = &p.hardware;
    assert_eq!(
        (
            hardware.build_volume().x_mm(),
            hardware.build_volume().y_mm()
        ),
        (100.0, 30.0)
    );
    assert_eq!(
        (
            hardware.pixel_size_microns().x.value(),
            hardware.pixel_size_microns().y.value()
        ),
        (100.0, 50.0)
    );
    assert!(hardware.mirror().x && hardware.mirror().y);
}

#[test]
fn printer_validation_is_typed_and_points_to_the_record() {
    let mut bad = sample("printer");
    bad["presetId"] = json!("Bad-ID");
    let error = read(json!([sample("printer"), bad])).unwrap_err();
    assert!(matches!(
        error,
        JsonCatalogError::Record {
            index: 1,
            source: RecordProblem { problem: Problem::BadId(_), field: "presetId", .. },
            ..
        }
    ));
    assert!(error.source().unwrap().source().is_some());
    for field in ["manufacturer", "name"] {
        let mut bad = sample("printer");
        bad[field] = json!("");
        assert!(matches!(
            read(json!([bad])),
            Err(JsonCatalogError::Record {
                source: RecordProblem { problem: Problem::Empty, .. },
                ..
            })
        ));
    }
    let mut bad = sample("printer");
    bad["buildVolumeMm"]
        .as_object_mut()
        .unwrap()
        .remove("width");
    assert!(matches!(
        read(json!([bad])),
        Err(JsonCatalogError::Record {
            source: RecordProblem { problem: Problem::NoGeometry, field: "x", .. },
            ..
        })
    ));
    let mut bad = sample("printer");
    bad["display"]["resolutionY"] = json!(0);
    assert!(matches!(
        read(json!([bad])),
        Err(JsonCatalogError::Record {
            source: RecordProblem { problem: Problem::Refused(_), field: "display resolution", .. },
            ..
        })
    ));
    let mut bad = sample("printer");
    bad["buildVolumeMm"]["height"] = json!(0);
    assert!(matches!(
        read(json!([bad])),
        Err(JsonCatalogError::Record {
            source: RecordProblem { problem: Problem::Refused(_), field: "build volume", .. },
            ..
        })
    ));
    let mut bad = sample("printer");
    bad["display"]["outputFormat"] = json!(" \t ");
    assert!(matches!(
        read(json!([bad])),
        Err(JsonCatalogError::Record {
            source: RecordProblem { problem: Problem::Empty, field: "outputFormat", .. },
            ..
        })
    ));
}

#[test]
fn material_values_are_checked_even_without_calibration_targets() {
    for (key, value) in [
        ("layerHeightMm", 0.0),
        ("normalExposureSec", -1.0),
        ("bottomExposureSec", 0.0),
        ("liftDistanceMm", -1.0),
        ("liftSpeedMmMin", 0.0),
        ("retractSpeedMmMin", -1.0),
    ] {
        let mut source = sample("material");
        source[key] = json!(value);
        let error = read(json!([source])).unwrap_err();
        assert!(matches!(
            error,
            JsonCatalogError::Record { index: 0, .. }
        ));
        assert!(error.source().unwrap().source().is_some());
    }
    let mut source = sample("material");
    source["validForPresets"] = json!(["UPPER"]);
    assert!(matches!(
        read(json!([source])),
        Err(JsonCatalogError::Record {
            source: RecordProblem { problem: Problem::BadTarget { .. }, .. },
            ..
        })
    ));
    let mut source = sample("material");
    source["scaleCompensationPct"] = json!({"x":0,"y":11,"z":0});
    assert!(matches!(
        read(json!([source])),
        Err(JsonCatalogError::Record {
            source: RecordProblem { problem: Problem::Refused(_), field: "y", .. },
            ..
        })
    ));
}

#[test]
fn conflicting_shared_data_and_calibrations_are_not_silently_selected() {
    let mut first = sample("material");
    first["validForPresets"] = json!(["target"]);
    let mut second = first.clone();
    second["brand"] = json!("Another");
    assert!(matches!(
        read(json!([first.clone(), second])),
        Err(JsonCatalogError::ConflictingMaterial {
            conflict: MaterialConflict::SharedFields,
            ..
        })
    ));
    let mut second = first.clone();
    second["normalExposureSec"] = json!(9.0);
    assert!(matches!(
        read(json!([first, second])),
        Err(JsonCatalogError::ConflictingMaterial {
            conflict: MaterialConflict::PrinterSettings,
            ..
        })
    ));
}

#[test]
fn material_merging_spans_files_and_printers_stay_first() {
    let scratch = tempfile::tempdir().unwrap();
    let first = scratch.path().join("first");
    let second = scratch.path().join("second");
    let mut a = sample("material");
    a["validForPresets"] = json!(["first-printer"]);
    let mut b = a.clone();
    b["validForPresets"] = json!(["generic-machine"]);
    b["normalExposureSec"] = json!(4.0);
    fs::write(&first, serde_json::to_vec(&json!([a])).unwrap()).unwrap();
    fs::write(&second, serde_json::to_vec(&json!([b])).unwrap()).unwrap();
    let printer = scratch.path().join("printer");
    fs::write(
        &printer,
        serde_json::to_vec(&json!([sample("printer")])).unwrap(),
    )
    .unwrap();
    let profiles = JsonCatalogReader::new()
        .read(&[&first, &printer, &second])
        .unwrap();
    assert_eq!(profiles.len(), 2);
    assert_eq!(printer_id(&profiles[0]), "probe-printer");
    let Document::Material(m) = &profiles[1] else {
        panic!("material")
    };
    assert_eq!(m.per_printer.len(), 2);
    assert_eq!(
        m.per_printer[&sc_print_profiles::CalibrationTarget::Generic]
            .normal_exposure_sec
            .unwrap()
            .value(),
        4.0
    );
}

#[test]
fn directories_are_sorted_case_insensitive_in_extension_and_nonrecursive() {
    let scratch = tempfile::tempdir().unwrap();
    for (name, id) in [
        ("a.json", "a-printer"),
        ("B.JSON", "b-printer"),
        ("ignored.txt", "ignored"),
    ] {
        let mut source = sample("printer");
        source["presetId"] = json!(id);
        fs::write(
            scratch.path().join(name),
            serde_json::to_vec(&json!([source])).unwrap(),
        )
        .unwrap();
    }
    fs::create_dir(scratch.path().join("nested.json")).unwrap();
    fs::write(scratch.path().join("nested.json/bad.json"), b"not JSON").unwrap();
    let profiles = JsonCatalogReader::new().read(&[scratch.path()]).unwrap();
    assert_eq!(
        profiles.iter().map(printer_id).collect::<Vec<_>>(),
        ["b-printer", "a-printer"]
    );
}

#[test]
fn empty_data_never_creates_output() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("out");
    assert!(read(json!([])).unwrap().is_empty());
    let report = save(&[], &out, true).unwrap();
    assert_eq!(report, Default::default());
    assert!(!out.exists());
    assert!(JsonCatalogReader::new().read(&[]).unwrap().is_empty());
}

#[test]
fn malformed_unknown_mixed_and_duplicate_keys_remain_errors() {
    assert!(matches!(
        read(json!({})),
        Err(JsonCatalogError::ParseJson { .. })
    ));
    assert!(matches!(
        read(json!([{}])),
        Err(JsonCatalogError::UnrecognisedRecord { .. })
    ));
    assert!(matches!(
        read(json!([sample("printer"), sample("material")])),
        Err(JsonCatalogError::ParseJson { .. })
    ));
    let scratch = tempfile::tempdir().unwrap();
    let input = scratch.path().join("duplicate.json");
    let text = serde_json::to_string(&json!([sample("printer")]))
        .unwrap()
        .replacen('{', "{\"presetId\":\"duplicate\",", 1);
    fs::write(&input, text).unwrap();
    let JsonCatalogError::ParseJson { source, .. } =
        JsonCatalogReader::new().read(&[&input]).unwrap_err()
    else {
        panic!("parse error")
    };
    assert!(source.line() > 0 && source.column() > 0);
}

#[test]
fn duplicate_detection_distinguishes_known_and_ignored_fields() {
    let scratch = tempfile::tempdir().unwrap();
    let input = scratch.path().join("fields.json");
    let printer = serde_json::to_string(&json!([sample("printer")])).unwrap();
    let ignored = printer.replacen('{', "{\"extra\":1,\"extra\":2,", 1);
    fs::write(&input, ignored).unwrap();
    assert!(JsonCatalogReader::new().read(&[&input]).is_ok());

    let duplicate = printer.replacen("\"resolutionX\":", "\"resolutionX\":1,\"resolutionX\":", 1);
    assert_ne!(duplicate, printer);
    fs::write(&input, duplicate).unwrap();
    assert!(matches!(
        JsonCatalogReader::new().read(&[&input]),
        Err(JsonCatalogError::ParseJson { .. })
    ));
}

#[test]
fn duplicate_fields_in_later_records_follow_the_same_rules() {
    let scratch = tempfile::tempdir().unwrap();
    let input = scratch.path().join("later.json");
    let first = serde_json::to_string(&sample("printer")).unwrap();
    let second = first.replacen('{', "{\"extra\":1,\"extra\":2,", 1);
    fs::write(&input, format!("[{first},{second}]")).unwrap();
    assert_eq!(JsonCatalogReader::new().read(&[&input]).unwrap().len(), 2);

    let second = first.replacen("\"resolutionX\":", "\"resolutionX\":1,\"resolutionX\":", 1);
    assert_ne!(second, first);
    fs::write(&input, format!("[{first},{second}]")).unwrap();
    assert!(matches!(
        JsonCatalogReader::new().read(&[&input]),
        Err(JsonCatalogError::ParseJson { .. })
    ));
}

#[test]
fn material_nested_duplicate_keys_follow_schema_rules() {
    let scratch = tempfile::tempdir().unwrap();
    let input = scratch.path().join("resin.json");
    let mut material = sample("material");
    material["scaleCompensationPct"] = json!({"x": 0, "y": 0, "z": 0});
    let original = serde_json::to_string(&json!([material])).unwrap();
    let ignored = original.replacen("\"x\":0", "\"extra\":1,\"extra\":2,\"x\":0", 1);
    assert_ne!(ignored, original);
    fs::write(&input, ignored).unwrap();
    assert!(JsonCatalogReader::new().read(&[&input]).is_ok());

    let duplicate = original.replacen("\"x\":0", "\"x\":1,\"x\":0", 1);
    assert_ne!(duplicate, original);
    fs::write(&input, duplicate).unwrap();
    assert!(matches!(
        JsonCatalogReader::new().read(&[&input]),
        Err(JsonCatalogError::ParseJson { .. })
    ));
}

#[test]
fn input_failure_happens_before_any_output_write() {
    let scratch = tempfile::tempdir().unwrap();
    let valid = scratch.path().join("valid.json");
    let bad = scratch.path().join("bad.json");
    let out = scratch.path().join("out");
    fs::write(
        &valid,
        serde_json::to_vec(&json!([sample("printer")])).unwrap(),
    )
    .unwrap();
    fs::write(&bad, b"broken").unwrap();
    let request = ConversionRequest::from_raw(RawCliArgs {
        format: "json".into(),
        inputs: vec![valid, bad],
        out: out.clone(),
        with_images: true,
    })
    .unwrap();
    let failure = convert(&request).unwrap_err();
    assert!(failure.source().is_some());
    assert!(failure.to_string().contains("bad.json"));
    assert!(!out.exists());
}

#[test]
fn copying_images_is_explicit_and_preserves_the_callers_profile() {
    let scratch = tempfile::tempdir().unwrap();
    let input = scratch.path().join("input.json");
    fs::write(scratch.path().join("picture.PNG"), b"synthetic image bytes").unwrap();
    let mut source = sample("printer");
    source["imageAssetPath"] = json!("picture.PNG");
    fs::write(&input, serde_json::to_vec(&json!([source])).unwrap()).unwrap();
    let profiles = JsonCatalogReader::new().read(&[&input]).unwrap();
    let output = scratch.path().join("images-on");
    assert_eq!(save(&profiles, &output, true).unwrap().images, 1);
    assert_eq!(
        fs::read(output.join("images/probe-printer.png")).unwrap(),
        b"synthetic image bytes"
    );
    let saved = sc_print_profiles::load_printer_profile_from_path(
        &output.join("printers/probe-printer.toml"),
    )
    .unwrap();
    assert_eq!(
        saved.identity.image_asset_path.unwrap().as_str(),
        "images/probe-printer.png"
    );
    let Document::Printer {
        profile: original, ..
    } = &profiles[0]
    else {
        panic!("printer")
    };
    assert!(original.identity.image_asset_path.is_none());
    fs::remove_file(scratch.path().join("picture.PNG")).unwrap();
    assert_eq!(
        save(&profiles, &scratch.path().join("images-off"), false)
            .unwrap()
            .images,
        0
    );
    let error = save(&profiles, &scratch.path().join("missing-image"), true).unwrap_err();
    assert!(error.to_string().contains("copy printer picture"));
    assert!(error.source().is_some());
}

#[test]
fn io_errors_retain_sources_and_paths() {
    let scratch = tempfile::tempdir().unwrap();
    let missing = scratch.path().join("absent.json");
    let error = JsonCatalogReader::new().read(&[&missing]).unwrap_err();
    assert!(matches!(&error, JsonCatalogError::ReadFile { path, .. } if path == &missing));
    assert!(error.source().is_some());
    fs::write(&missing, [0xff]).unwrap();
    assert!(matches!(
        JsonCatalogReader::new().read(&[&missing]),
        Err(JsonCatalogError::ReadFile { .. })
    ));
}

#[test]
fn printer_error_causes_distinguish_validation_from_missing_fields() {
    for (path, value, has_cause) in [
        ("id", json!("Bad-ID"), true),
        ("resolution", json!(0), true),
        ("height", json!(0), true),
        ("name", json!(""), false),
        ("format", json!(""), false),
    ] {
        let mut record = sample("printer");
        match path {
            "id" => record["presetId"] = value,
            "resolution" => record["display"]["resolutionY"] = value,
            "height" => record["buildVolumeMm"]["height"] = value,
            "name" => record["name"] = value,
            _ => record["display"]["outputFormat"] = value,
        }
        let error = read(json!([record])).unwrap_err();
        let JsonCatalogError::Record { source, .. } = error else {
            panic!("{path}")
        };
        assert_eq!(source.source().is_some(), has_cause, "{path}");
    }
}

#[test]
fn material_error_causes_cover_each_validation_kind() {
    for (field, value, has_cause) in [
        ("templateId", json!("BAD"), true),
        ("validForPresets", json!(["BAD"]), true),
        ("layerHeightMm", json!(0), true),
        ("normalExposureSec", json!(0), true),
        ("liftSpeedMmMin", json!(0), true),
        (
            "scaleCompensationPct",
            json!({"x": 11, "y": 0, "z": 0}),
            true,
        ),
        ("name", json!(""), false),
    ] {
        let mut record = sample("material");
        record[field] = value;
        let JsonCatalogError::Record { source, .. } = read(json!([record])).unwrap_err()
        else {
            panic!("{field}")
        };
        assert_eq!(source.source().is_some(), has_cause, "{field}");
    }
}

#[test]
fn reader_error_causes_cover_parse_and_mapping() {
    let scratch = tempfile::tempdir().unwrap();
    let input = scratch.path().join("bad.json");
    fs::write(&input, "[").unwrap();
    let parse = JsonCatalogReader::new().read(&[&input]).unwrap_err();
    assert!(matches!(parse, JsonCatalogError::ParseJson { .. }));
    assert!(parse.source().is_some());
    for (kind, field) in [("printer", "presetId"), ("material", "templateId")] {
        let mut record = sample(kind);
        record[field] = json!("BAD");
        let error = read(json!([record])).unwrap_err();
        assert!(matches!(
            error,
            JsonCatalogError::Record { .. }
        ));
        assert!(error.source().is_some(), "{kind}");
    }
    let error = JsonCatalogError::UnrecognisedRecord { path: input };
    assert!(error.source().is_none());
}

#[test]
fn request_raw_boundary_contract() {
    let raw = |inputs: Vec<&str>, with_images| RawCliArgs {
        format: "json".into(),
        inputs: inputs.into_iter().map(Into::into).collect(),
        out: "destination".into(),
        with_images,
    };
    let mut unsupported = raw(vec!["a"], false);
    unsupported.format = "unknown".into();
    assert!(ConversionRequest::from_raw(unsupported).is_err());
    let error = ConversionRequest::from_raw(raw(vec![], false)).unwrap_err();
    assert_eq!(
        error.to_string(),
        "At least one source file or directory is required"
    );
    for images in [false, true] {
        let request = ConversionRequest::from_raw(raw(vec!["a", "b"], images)).unwrap();
        assert_eq!(request.format(), "json");
        assert_eq!(
            request
                .inputs()
                .iter()
                .map(|p| p.as_path())
                .collect::<Vec<_>>(),
            vec![Path::new("a"), Path::new("b")]
        );
        assert_eq!(request.out(), Path::new("destination"));
        assert_eq!(request.with_images(), images);
        assert_eq!(
            request.summary(),
            format!(
                "format: json; inputs: 2; output: destination; images: {}",
                if images { "on" } else { "off" }
            )
        );
    }
}

#[test]
fn material_conflict_identifies_the_incoming_file_and_record() {
    let scratch = tempfile::tempdir().unwrap();
    let first_path = scratch.path().join("first.json");
    let second_path = scratch.path().join("second.json");
    let first = sample("material");
    let mut second = first.clone();
    second["brand"] = json!("Different");
    fs::write(&first_path, serde_json::to_vec(&json!([first])).unwrap()).unwrap();
    fs::write(&second_path, serde_json::to_vec(&json!([second])).unwrap()).unwrap();
    let error = JsonCatalogReader::new()
        .read(&[&first_path, &second_path])
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("second.json"), "{message}");
    assert!(message.contains("record 0"), "{message}");
}

#[test]
fn unrecognised_record_diagnostic_identifies_its_position() {
    let error = read(json!([{"unexpected": true}])).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("record 0"), "{message}");
    assert!(message.contains("json"), "{message}");
}

#[test]
fn omitted_defaults_and_explicit_null_have_distinct_json_rules() {
    let mut printer = sample("printer");
    let fields = printer.as_object_mut().unwrap();
    fields.remove("antiAliasing");
    fields.remove("hasCamera");
    fields.remove("family");
    fields.remove("pixelSize");
    let display = fields["display"].as_object_mut().unwrap();
    display.remove("mirrorX");
    display.remove("mirrorY");
    assert!(read(json!([printer.clone()])).is_ok());
    printer["antiAliasing"] = Value::Null;
    assert!(matches!(
        read(json!([printer])),
        Err(JsonCatalogError::ParseJson { .. })
    ));

    let mut resin = sample("material");
    resin.as_object_mut().unwrap().remove("validForPresets");
    resin["layerHeightMm"] = Value::Null;
    assert!(read(json!([resin.clone()])).is_ok());
    resin["validForPresets"] = Value::Null;
    assert!(matches!(
        read(json!([resin])),
        Err(JsonCatalogError::ParseJson { .. })
    ));
}

#[test]
fn derived_pitch_preserves_the_source_float() {
    let mut source = sample("printer");
    source["buildVolumeMm"]
        .as_object_mut()
        .unwrap()
        .remove("depth");
    source["pixelSize"] = json!({"x": 100, "y": 0.1});
    let profiles = read(json!([source])).unwrap();
    let Document::Printer { profile, .. } = &profiles[0] else {
        panic!("printer")
    };
    assert_eq!(profile.hardware.pixel_size_microns().y.value(), 0.1);
}

#[test]
fn duplicate_optional_fields_are_rejected_even_when_first_value_is_null() {
    let scratch = tempfile::tempdir().unwrap();
    let input = scratch.path().join("optional.json");
    for (kind, field) in [("printer", "family"), ("material", "layerHeightMm")] {
        let mut record = sample(kind);
        record[field] = Value::Null;
        let original = serde_json::to_string(&json!([record])).unwrap();
        let needle = format!("\"{field}\":null");
        let duplicate = original.replacen(&needle, &format!("{needle},{needle}"), 1);
        assert_ne!(duplicate, original);
        fs::write(&input, duplicate).unwrap();
        assert!(
            matches!(
                JsonCatalogReader::new().read(&[&input]),
                Err(JsonCatalogError::ParseJson { .. })
            ),
            "{kind}.{field}"
        );
    }
}
