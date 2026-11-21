// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use assert_cmd::Command;
use predicates::str::contains;
use sc_print_profiles::recipes::{Recipe, Step};
use std::path::Path;

#[test]
fn cli_recipe_file_and_saved_name_scale_the_exported_scene() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let temp = tempfile::tempdir().unwrap();
    let cube = temp.path().join("cube.stl");
    let mut text = String::from("solid cube
");
    let mesh = sc_mesh_fixtures::unit_cube_mesh();
    for face in &mesh.faces {
        text += "facet normal 0 0 0
outer loop
";
        for i in face.vertices {
            let [x, y, z] = mesh.vertices[usize::from(i)].coords();
            text += &format!("vertex {x} {y} {z}
");
        }
        text += "endloop
endfacet
";
    }
    std::fs::write(&cube, text + "endsolid cube
").unwrap();
    let recipe = Recipe {
        version: 1,
        name: "CLI fixture".into(),
        steps: vec![Step::Scale { percent: 200.0 }, Step::Slice],
    };
    let file = sc_print_profiles::recipes::save_new(temp.path(), &recipe).unwrap();
    for (index, reference) in [file.to_string_lossy().into_owned(), recipe.name]
        .iter()
        .enumerate()
    {
        let target = temp.path().join(format!("out-{index}.3mf"));
        Command::cargo_bin("sc-slicer-cli")
            .unwrap()
            .arg("slice")
            .arg("--source")
            .arg(&cube)
            .arg("--target")
            .arg(&target)
            .arg("--target-format")
            .arg("3mf")
            .arg("--printer-profile")
            .arg("slicer-cli-e2e-test")
            .arg("--material-profile")
            .arg("slicer-cli-e2e-test-resin")
            .arg("--recipe")
            .arg(reference)
            .env("SC_SLICER_ASSETS_DIR", root.join("tests/fixtures/profiles"))
            .env("SLEPTIC_RECIPES_DIR", temp.path())
            .assert()
            .success()
            .stdout(contains("Recipe complete; all steps succeeded"));
        let scene = sc_project_formats::load_scene(&target, None, |_| {})
            .unwrap()
            .scene;
        let bounds = scene.aabb().unwrap().unwrap();
        assert!((bounds.max()[2] - bounds.min()[2] - 2.0).abs() < 1e-6);
    }
}
