// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use crate::*;
use sc_sl1cer::{contour::*, geom2d::Point2D, profile::*};
use std::{
    io::{Cursor, Read},
    path::Path,
};
fn options(n: u32) -> WriterOptions {
    WriterOptions::from_profile(
        &SliceProfile::new(
            BuildVolume::new(20.0, 20.0, 100.0).unwrap(),
            PixelResolution::new(200, 200).unwrap(),
            LayerSchedule::uniform(0.1, 0.2, n.max(1)).unwrap(),
            Mirror::default(),
            AaLevel::Off,
            PackingMode::None,
        ),
        "test-version",
    )
}
fn polygon(points: &[[f64; 2]]) -> Polygon {
    Polygon::new(
        ClosedRing::new(points.iter().map(|p| Point2D::new(p[0], p[1])).collect()),
        vec![],
    )
    .unwrap()
}
fn stack(n: usize) -> ContourStack {
    ContourStack::new(
        (0..n)
            .map(|i| {
                ContourLayer::new(
                    i as f64 + 0.1,
                    vec![polygon(&[
                        [-5.0, -5.0],
                        [5.0, -5.0],
                        [5.0, 5.0],
                        [-5.0, 5.0],
                    ])],
                )
            })
            .collect(),
    )
    .unwrap()
}
fn job(n: usize) -> PreparedJob {
    PreparedJob {
        regions: vec![],
        layer_z_mm: (0..n).map(|i| i as f64).collect(),
        exposure_s: vec![1.0; n],
        script: "G1 Z1\r\n; opaque \u{0} script".into(),
    }
}
fn decode(b: &[u8]) -> (u32, u32, png::ColorType, Vec<u8>) {
    let mut r = png::Decoder::new(Cursor::new(b)).read_info().unwrap();
    let mut v = vec![0; r.output_buffer_size().unwrap()];
    let i = r.next_frame(&mut v).unwrap();
    v.truncate(i.buffer_size());
    (i.width, i.height, i.color_type, v)
}
fn entry(z: &mut zip::ZipArchive<Cursor<Vec<u8>>>, n: &str) -> Vec<u8> {
    let mut v = vec![];
    z.by_name(n).unwrap().read_to_end(&mut v).unwrap();
    v
}
#[test]
fn formats_and_path_inference() {
    for (f, s) in [
        (Format::ThreeMf, "3mf"),
        (Format::SvgCombined, "svg-combined"),
        (Format::SvgPerLayer, "svg-per-layer"),
        (Format::PngPerLayer, "png-per-layer"),
        (Format::GcodeZip, "gcode-zip"),
    ] {
        assert_eq!(s.parse::<Format>().unwrap(), f);
        assert_eq!(f.to_string(), s);
        assert_eq!(
            f.is_directory_format(),
            matches!(f, Format::SvgPerLayer | Format::PngPerLayer)
        );
    }
    assert_eq!(
        "SVG".parse::<Format>().unwrap_err().to_string(),
        "unknown format \"SVG\"; expected one of: svg-per-layer, svg-combined, png-per-layer, 3mf, gcode-zip"
    );
    for (p, f) in [
        ("x.SVG", Some(Format::SvgCombined)),
        ("a.slices.3MF", Some(Format::ThreeMf)),
        ("x.ZIP", Some(Format::GcodeZip)),
        ("png", None),
        ("x.png", None),
    ] {
        assert_eq!(format::format_from_path(Path::new(p)), f)
    }
}
#[test]
fn paths_and_dispatch() {
    let d = tempfile::tempdir().unwrap();
    let s = stack(0);
    let o = options(1);
    for f in [Format::SvgPerLayer, Format::PngPerLayer] {
        let p = d.path().join(f.as_str()).join("child");
        write_slice(&s, &p, f, &o).unwrap();
        assert_eq!(std::fs::read_dir(&p).unwrap().count(), 0);
        std::fs::write(p.join("a"), b"x").unwrap();
        assert!(matches!(
            write_slice(&s, &p, f, &o),
            Err(WriterError::OutputPathExistsAndIsNotEmpty { .. })
        ));
    }
    let p = d.path().join("file");
    std::fs::write(&p, b"").unwrap();
    assert!(matches!(
        write_slice(&s, &p, Format::PngPerLayer, &o),
        Err(WriterError::OutputPathExistsAndIsNotADirectory { .. })
    ));
    assert!(matches!(
        write_slice(&s, d.path(), Format::ThreeMf, &o),
        Err(WriterError::OutputPathInvalid { .. })
    ));
    assert!(matches!(
        write_slice(&s, &p, Format::ThreeMf, &o),
        Err(WriterError::SceneContextRequired { .. })
    ));
    assert_eq!(
        write_slice(&s, &p, Format::GcodeZip, &o)
            .unwrap_err()
            .to_string(),
        "encoder for format \"gcode-zip\" rejected the request: gcode-zip requires an explicit PreparedJob; use write_gcode_zip"
    );
    assert!(matches!(
        write_scene_slices(&sc_scene::Scene::new(), &[], d.path(), &o),
        Err(WriterError::NoModelsToWrite)
    ));
}
#[test]
fn zip_validation_precedes_creation() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("out.zip");
    let s = stack(2);
    let o = options(2);
    let mut j = job(1);
    assert!(
        write_gcode_zip(&s, &p, &o, &j)
            .unwrap_err()
            .to_string()
            .contains("layer count differs")
    );
    j = job(2);
    j.layer_z_mm[1] = 0.0;
    assert!(
        write_gcode_zip(&s, &p, &o, &j)
            .unwrap_err()
            .to_string()
            .contains("invalid Z or exposure")
    );
    j = job(2);
    j.exposure_s[0] = f64::NAN;
    assert!(write_gcode_zip(&s, &p, &o, &j).is_err());
    j = job(2);
    j.regions = vec![([0.0, 0.0], [2.0, 2.0]), ([1.0, 1.0], [3.0, 3.0])];
    assert!(
        write_gcode_zip(&s, &p, &o, &j)
            .unwrap_err()
            .to_string()
            .contains("invalid or overlapping")
    );
    assert!(!p.exists());
    j.regions[1] = ([2.0, 0.0], [4.0, 2.0]);
    write_gcode_zip(&s, &p, &o, &j).unwrap();
}
#[test]
fn png_zip_tiles_metadata_preview_and_thread_determinism() {
    let d = tempfile::tempdir().unwrap();
    let s = stack(3);
    let o = options(3);
    let mut j = job(3);
    j.regions = vec![([-10.0, -10.0], [0.0, 10.0])];
    let mut bytes = vec![];
    for n in [1, 4] {
        let p = d.path().join(format!("{n}.zip"));
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build()
            .unwrap()
            .install(|| write_gcode_zip(&s, &p, &o, &j))
            .unwrap();
        bytes.push(std::fs::read(p).unwrap());
    }
    assert_eq!(bytes[0], bytes[1]);
    let mut z = zip::ZipArchive::new(Cursor::new(bytes.remove(0))).unwrap();
    assert_eq!(z.len(), 8);
    let names: Vec<_> = (0..z.len())
        .map(|i| {
            let f = z.by_index(i).unwrap();
            assert_eq!(f.compression(), zip::CompressionMethod::Stored);
            f.name().unwrap().into_owned()
        })
        .collect();
    assert_eq!(
        names,
        [
            "1.png",
            "1-tile1.png",
            "2.png",
            "2-tile1.png",
            "3.png",
            "3-tile1.png",
            "run.gcode",
            "preview.png"
        ]
    );
    assert_eq!(entry(&mut z, "run.gcode"), j.script.as_bytes());
    let full = decode(&entry(&mut z, "1.png"));
    assert_eq!(
        (full.0, full.1, full.2),
        (200, 200, png::ColorType::Grayscale)
    );
    assert_eq!(full.3.iter().filter(|&&v| v == 255).count(), 10000);
    let tile = decode(&entry(&mut z, "1-tile1.png"));
    for y in 0..200 {
        for x in 0..200 {
            assert_eq!(
                tile.3[y * 200 + x],
                if x < 100 { full.3[y * 200 + x] } else { 0 }
            )
        }
    }
    assert_eq!(decode(&entry(&mut z, "preview.png")), full);
    let p = d.path().join("png");
    write_slice(&s, &p, Format::PngPerLayer, &o).unwrap();
    let a = d.path().join("one.zip");
    let b = d.path().join("four.zip");
    let mut tiles = job(3);
    tiles.regions = vec![([-5.0, -5.0], [0.0, 5.0]), ([0.0, -5.0], [5.0, 5.0])];
    for (threads, path) in [(1, &a), (4, &b)] {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| write_gcode_zip(&s, path, &o, &tiles))
            .unwrap();
    }
    assert_eq!(std::fs::read(a).unwrap(), std::fs::read(b).unwrap());
    let png = std::fs::read(p.join("layer_0000.png")).unwrap();
    assert_eq!(decode(&png), full);
    let reader = png::Decoder::new(Cursor::new(png)).read_info().unwrap();
    let meta = &reader.info().uncompressed_latin1_text;
    assert_eq!(
        meta.iter().map(|m| m.keyword.as_str()).collect::<Vec<_>>(),
        [
            "slicer-version",
            "build-volume-x-mm",
            "build-volume-y-mm",
            "build-volume-z-mm",
            "pixel-resolution-x-px",
            "pixel-resolution-y-px",
            "layer-schedule",
            "aa-level",
            "mirror-x",
            "mirror-y",
            "min-alpha-pct",
            "layer-z-mm",
            "layer-index"
        ]
    );
    assert_eq!(
        meta[6].text,
        "uniform layer-height-mm=0.1 first-layer-height-mm=0.2 total-layers=3"
    );
}
#[test]
fn all_sixteen_orientations() {
    let d = tempfile::tempdir().unwrap();
    let s = ContourStack::new(vec![ContourLayer::new(
        0.1,
        vec![polygon(&[[1.0, 2.0], [3.0, 2.0], [3.0, 5.0], [1.0, 5.0]])],
    )])
    .unwrap();
    for deg in [0_u16, 90, 180, 270] {
        for mx in [false, true] {
            for my in [false, true] {
                let c = MachineCalibration::new(deg.try_into().unwrap(), [1.1, 0.9]).unwrap();
                let profile = SliceProfile::new(
                    BuildVolume::new(20.0, 30.0, 100.0).unwrap(),
                    PixelResolution::new(200, 150).unwrap(),
                    LayerSchedule::uniform(0.1, 0.1, 1).unwrap(),
                    Mirror { x: mx, y: my },
                    AaLevel::X4,
                    PackingMode::None,
                )
                .with_calibration(c);
                let o = WriterOptions::from_profile(&profile, "v");
                let path = d.path().join(format!("{deg}-{mx}-{my}.zip"));
                write_gcode_zip(&s, &path, &o, &job(1)).unwrap();
                let mut z =
                    zip::ZipArchive::new(Cursor::new(std::fs::read(&path).unwrap())).unwrap();
                let img = decode(&entry(&mut z, "1.png"));
                assert_eq!(
                    (img.0, img.1),
                    if deg == 90 || deg == 270 {
                        (150, 200)
                    } else {
                        (200, 150)
                    }
                );
                let mut u: f64 = (2.0 * 1.1 + 10.0) * 10.0;
                let mut v: f64 = (3.0 * 0.9 + 15.0) * 5.0;
                if mx {
                    u = 200.0 - u
                }
                if my {
                    v = 150.0 - v
                }
                let (x, y) = match deg {
                    0 => (u, v),
                    90 => (v, 200.0 - u),
                    180 => (200.0 - u, 150.0 - v),
                    _ => (150.0 - v, u),
                };
                let at =
                    (img.1 as usize - 1 - y.floor() as usize) * img.0 as usize + x.floor() as usize;
                assert_eq!(img.3[at], 255, "{deg} {mx} {my}");
                let path = d.path().join(format!("p-{deg}-{mx}-{my}"));
                write_slice(&s, &path, Format::PngPerLayer, &o).unwrap();
                assert_eq!(
                    decode(&std::fs::read(path.join("layer_0000.png")).unwrap()),
                    img
                );
            }
        }
    }
}
#[test]
fn accurate_bottom_transition_and_normal_layers() {
    let s = stack(3);
    let profile = SliceProfile::new(
        *options(3).build_volume(),
        *options(3).pixel_resolution(),
        *options(3).layer_schedule(),
        Mirror::default(),
        AaLevel::X4,
        PackingMode::None,
    )
    .with_accuracy(sc_sl1cer::raster::accuracy::Accuracy {
        xy_compensation_mm: 0.2,
        bottom_compensation_mm: 0.5,
        bottom_layers: 1,
        transition_layers: 1,
        ..Default::default()
    });
    let o = WriterOptions::from_profile(&profile, "v");
    for (i, side) in [94, 98, 104].into_iter().enumerate() {
        let data = debug::png::pixels(&s.layers()[i], i, &o);
        assert_eq!(data.iter().filter(|&&v| v == 255).count(), side * side);
    }
}
#[test]
fn empty_zip_and_small_projection() {
    let d = tempfile::tempdir().unwrap();
    let o = options(1);
    let p = d.path().join("empty.zip");
    write_gcode_zip(&stack(0), &p, &o, &job(0)).unwrap();
    let mut z = zip::ZipArchive::new(Cursor::new(std::fs::read(p).unwrap())).unwrap();
    assert_eq!(z.len(), 2);
    let img = decode(&entry(&mut z, "preview.png"));
    assert_eq!((img.0, img.1), (200, 200));
    assert!(img.3.iter().all(|&v| v == 0));
}
#[test]
fn svg_metadata_empty_layers_and_calibration() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("a.svg");
    let s = ContourStack::new(vec![ContourLayer::new(0.1, vec![])]).unwrap();
    write_slice(&s, &p, Format::SvgCombined, &options(1)).unwrap();
    let text = std::fs::read_to_string(p).unwrap();
    assert!(text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"));
    assert!(text.contains("viewBox=\"-10 -10 20 20\""));
    assert!(text.contains(
        "id=\"layer-0000\" data-z-mm=\"0.1\" data-layer-index=\"0\" transform=\"scale(1 -1)\""
    ));
    assert!(!text.contains("matrix("));
    assert!(!text.contains('\r'));
}
#[test]
fn thousands_of_layers_remain_ordered() {
    let d = tempfile::tempdir().unwrap();
    let profile = SliceProfile::new(
        BuildVolume::new(20.0, 20.0, 100.0).unwrap(),
        PixelResolution::new(8, 8).unwrap(),
        LayerSchedule::uniform(0.01, 0.01, 2001).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let o = WriterOptions::from_profile(&profile, "v");
    let p = d.path().join("many.zip");
    write_gcode_zip(&stack(2001), &p, &o, &job(2001)).unwrap();
    let mut z = zip::ZipArchive::new(Cursor::new(std::fs::read(p).unwrap())).unwrap();
    assert_eq!(z.len(), 2003);
    assert_eq!(z.by_index(2000).unwrap().name().unwrap(), "2001.png");
}

#[test]
fn three_leaf_slice_package_is_core_loadable_and_retains_void_layers() {
    use sc_3mf::{model::*, package::PartName};
    use sc_mesh_core::transform::Transform3D;
    use sc_scene::{LeafPayload, Scene, SceneMetadata};
    let mut scene = Scene::new();
    scene.set_meta(Some(SceneMetadata {
        name: Some("title".into()),
        comment: Some("description".into()),
    }));
    let mesh = std::sync::Arc::new(sc_mesh_fixtures::unit_cube_mesh());
    for i in 0..3 {
        let id = scene
            .add_leaf(
                Some(SceneMetadata {
                    name: Some(format!("cube & {i}")),
                    comment: None,
                }),
                Transform3D::Identity,
                LeafPayload::Mesh(mesh.clone()),
            )
            .unwrap();
        scene.add_root(id).unwrap();
    }
    let profile = SliceProfile::new(
        BuildVolume::new(20.0, 20.0, 20.0).unwrap(),
        PixelResolution::new(40, 40).unwrap(),
        LayerSchedule::uniform(0.25, 0.25, 4).unwrap(),
        Mirror::default(),
        AaLevel::Off,
        PackingMode::None,
    );
    let leaves = sc_sl1cer::slice_scene(&scene, &profile).unwrap();
    let o = WriterOptions::from_profile(&profile, "v");
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("slices.3mf");
    write_scene_slices(&scene, &leaves, &path, &o).unwrap();
    let p = sc_3mf::package::Package::read_from(std::fs::File::open(&path).unwrap()).unwrap();
    let part = |n: &str| PartName::new(n).unwrap();
    let m = parse_model(p.part_data(&part("/3D/3dmodel.model")).unwrap()).unwrap();
    assert_eq!(m.objects.len(), 3);
    assert_eq!(m.build.len(), 3);
    assert_eq!(m.slice_stacks.len(), 3);
    assert_eq!(m.metadata[0].value, "title");
    assert_eq!(m.metadata[1].value, "description");
    for (i, o) in m.objects.iter().enumerate() {
        assert_eq!(o.id.get(), 2 * (i as u32 + 1));
        assert_eq!(o.slice_stack.unwrap().get(), 2 * i as u32 + 1);
        assert_eq!(o.mesh_resolution, MeshResolution::Fullres);
        let ObjectPayload::Mesh(mesh) = &o.payload else {
            panic!()
        };
        assert_eq!(mesh.vertices.len(), 8);
        assert_eq!(mesh.triangles.len(), 12);
        let n = format!("/2D/slices-{}.model", i + 1);
        let part_doc = parse_model(p.part_data(&part(&n)).unwrap()).unwrap();
        assert!(part_doc.objects.is_empty());
        assert!(part_doc.build.is_empty());
        let SliceStackContent::Slices(s) = &part_doc.slice_stacks[0].content else {
            panic!()
        };
        assert_eq!(
            s.iter().map(|s| s.z_top).collect::<Vec<_>>(),
            vec![0.25, 0.5, 0.75, 1.0]
        );
        assert!(s[2].polygons.is_empty());
        assert!(s[3].polygons.is_empty());
        for s in s {
            for p in &s.polygons {
                assert!(p.is_closed());
            }
        }
    }
    let (loaded, warnings) =
        sc_mesh_formats::threemf::read_threemf(&mut std::fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(loaded.reachable_leaf_count(), 3);
    assert!(warnings.contains(
        &sc_mesh_formats::messages::LoadThreeMfWarning::SliceDataIgnored { stack_count: 3 }
    ));
    let png = p.part_data(&part("/Metadata/thumbnail.png")).unwrap();
    let image = decode(png);
    assert_eq!((image.0, image.1, image.2), (256, 256, png::ColorType::Rgb));
    assert_eq!(&image.3[..3], &[242; 3]);
    let wrong = options(5);
    let bad = d.path().join("no-created.3mf");
    assert!(matches!(
        write_scene_slices(&scene, &leaves, &bad, &wrong),
        Err(WriterError::LayerGridMismatch { .. })
    ));
    assert!(!bad.exists());
    let reduced = d.path().join("one.3mf");
    write_scene_slices(&scene, &leaves[1..2], &reduced, &o).unwrap();
    let (loaded, _) =
        sc_mesh_formats::threemf::read_threemf(&mut std::fs::File::open(reduced).unwrap()).unwrap();
    assert_eq!(loaded.reachable_leaf_count(), 1);
}
#[test]
fn svg_holes_and_all_calibration_matrices() {
    let d = tempfile::tempdir().unwrap();
    let outer = ClosedRing::new(vec![
        Point2D::new(-5.0, -5.0),
        Point2D::new(5.0, -5.0),
        Point2D::new(5.0, 5.0),
        Point2D::new(-5.0, 5.0),
    ]);
    let hole = ClosedRing::new(vec![
        Point2D::new(-1.0, -1.0),
        Point2D::new(-1.0, 1.0),
        Point2D::new(1.0, 1.0),
        Point2D::new(1.0, -1.0),
    ]);
    let s = ContourStack::new(vec![
        ContourLayer::new(0.1, vec![Polygon::new(outer, vec![hole]).unwrap()]),
        ContourLayer::new(0.2, vec![]),
    ])
    .unwrap();
    for deg in [0_u16, 90, 180, 270] {
        for mx in [false, true] {
            for my in [false, true] {
                let profile = SliceProfile::new(
                    BuildVolume::new(20.0, 30.0, 40.0).unwrap(),
                    PixelResolution::new(200, 150).unwrap(),
                    LayerSchedule::uniform(0.1, 0.1, 2).unwrap(),
                    Mirror { x: mx, y: my },
                    AaLevel::X8,
                    PackingMode::None,
                )
                .with_calibration(
                    MachineCalibration::new(deg.try_into().unwrap(), [1.2, 0.8]).unwrap(),
                );
                let o = WriterOptions::from_profile(&profile, "v");
                let p = d.path().join("a.svg");
                write_slice(&s, &p, Format::SvgCombined, &o).unwrap();
                let text = std::fs::read_to_string(&p).unwrap();
                let sx = if mx { -1.2 } else { 1.2 };
                let sy = if my { -0.8 } else { 0.8 };
                let v = match deg {
                    0 => [sx, 0.0, 0.0, sy],
                    90 => [0.0, sx, -sy, 0.0],
                    180 => [-sx, 0.0, 0.0, -sy],
                    _ => [0.0, -sx, sy, 0.0],
                };
                assert!(text.contains(&format!("matrix({} {} {} {} 0 0)", v[0], v[1], v[2], v[3])));
                assert!(text.contains(if deg == 90 || deg == 270 {
                    "viewBox=\"-15 -10 30 20\""
                } else {
                    "viewBox=\"-10 -15 20 30\""
                }));
                assert!(
                    text.contains("M -5 -5 L 5 -5 L 5 5 L -5 5 Z M -1 -1 L -1 1 L 1 1 L 1 -1 Z")
                );
                assert!(text.contains("layer-0001"));
            }
        }
    }
}
#[test]
fn preview_mesh_overrides_projection_without_changing_layers() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("a.zip");
    let o = options(1).with_preview_mesh(&sc_mesh_fixtures::unit_cube_mesh());
    let s = stack(1);
    write_gcode_zip(&s, &p, &o, &job(1)).unwrap();
    let mut z = zip::ZipArchive::new(Cursor::new(std::fs::read(p).unwrap())).unwrap();
    let image = decode(&entry(&mut z, "preview.png"));
    assert_eq!((image.0, image.1, image.2), (256, 256, png::ColorType::Rgb));
    assert_eq!(image.3, o.preview.unwrap());
    assert_eq!(
        decode(&entry(&mut z, "1.png")).3,
        debug::png::pixels(&s.layers()[0], 0, &options(1))
    );
}

/// Run in release on the acceptance machine; this is a measurement, not a timing assertion.
#[test]
#[ignore]
fn export_measurements() {
    let d = tempfile::tempdir().unwrap();
    let workers = 4;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()
        .unwrap();
    for (name, size, regions) in [
        ("sparse", 0.25, vec![]),
        ("dense", 9.0, vec![]),
        (
            "calibration",
            5.0,
            vec![([-10.0, -10.0], [0.0, 10.0]), ([0.0, -10.0], [10.0, 10.0])],
        ),
    ] {
        let n = 128;
        let profile = SliceProfile::new(
            BuildVolume::new(20.0, 20.0, 100.0).unwrap(),
            PixelResolution::new(2048, 1536).unwrap(),
            LayerSchedule::uniform(0.05, 0.05, n).unwrap(),
            Mirror::default(),
            AaLevel::X4,
            PackingMode::None,
        );
        let o = WriterOptions::from_profile(&profile, "benchmark");
        let poly = polygon(&[[-size, -size], [size, -size], [size, size], [-size, size]]);
        let stack = ContourStack::new(
            (0..n)
                .map(|i| ContourLayer::new(i as f64 * 0.05 + 0.025, vec![poly.clone()]))
                .collect(),
        )
        .unwrap();
        let mut j = job(n as usize);
        j.regions = regions;
        let p = d.path().join(format!("{name}.zip"));
        let start = std::time::Instant::now();
        pool.install(|| write_gcode_zip(&stack, &p, &o, &j))
            .unwrap();
        println!(
            "raster,{name},{workers},{n},2048,1536,{:.6},{}",
            start.elapsed().as_secs_f64(),
            std::fs::metadata(p).unwrap().len()
        );
    }
    // Thousands of ordinary layers exercise the bounded batching path independently of resolution.
    let profile = SliceProfile::new(
        BuildVolume::new(20.0, 20.0, 100.0).unwrap(),
        PixelResolution::new(512, 512).unwrap(),
        LayerSchedule::uniform(0.05, 0.05, 2001).unwrap(),
        Mirror::default(),
        AaLevel::X4,
        PackingMode::None,
    );
    let o = WriterOptions::from_profile(&profile, "benchmark");
    let p = d.path().join("thousands.zip");
    let start = std::time::Instant::now();
    pool.install(|| write_gcode_zip(&stack(2001), &p, &o, &job(2001)))
        .unwrap();
    println!(
        "raster,thousands,{workers},2001,512,512,{:.6},{}",
        start.elapsed().as_secs_f64(),
        std::fs::metadata(p).unwrap().len()
    );
}
