// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
use super::*;

fn volume(mesh: &IndexedMesh) -> f64 {
    mesh.faces
        .iter()
        .map(|f| {
            let [a, b, c] = f
                .vertices
                .map(|i| DVec3::from_array(mesh.vertices[usize::from(i)].coords().map(f64::from)));
            a.dot(b.cross(c)) / 6.0
        })
        .sum::<f64>()
        .abs()
}

#[test]
fn overlapping_cube_booleans_have_exact_volumes_and_closed_edges() {
    let a = cuboid([0.; 3], [2.; 3]).unwrap();
    let b = cuboid([1.; 3], [3.; 3]).unwrap();
    for (op, expected) in [
        (BooleanOp::Union, 15.),
        (BooleanOp::Subtract, 7.),
        (BooleanOp::Intersect, 1.),
    ] {
        let result = boolean(&a, &b, op).unwrap();
        assert!(
            (volume(&result) - expected).abs() < 1e-5,
            "{op:?}: {}",
            volume(&result)
        );
        println!("R15 Boolean {op:?}: volume={:.6} mm3", volume(&result));
        validate(&result).unwrap();
    }
}

#[test]
fn coplanar_identical_touching_and_disjoint_cubes_are_safe() {
    let a = cuboid([0.; 3], [2.; 3]).unwrap();
    for (lo, hi, expected) in [
        ([0.; 3], [2.; 3], 8.),
        ([2., 0., 0.], [4., 2., 2.], 16.),
        ([3.; 3], [5.; 3], 16.),
    ] {
        let b = cuboid(lo, hi).unwrap();
        assert!((volume(&boolean(&a, &b, BooleanOp::Union).unwrap()) - expected).abs() < 1e-5);
    }
    assert!(
        boolean(&a, &a, BooleanOp::Subtract)
            .unwrap()
            .faces
            .is_empty()
    );
}

#[test]
fn cut_volume_is_conserved_and_caps_are_optional() {
    let a = cuboid([0.; 3], [2.; 3]).unwrap();
    let (lower, upper) = cut(&a, 0.7, true).unwrap();
    println!(
        "R15 cut original={:.6} lower={:.6} upper={:.6} mm3",
        volume(&a),
        volume(&lower),
        volume(&upper)
    );
    assert!((volume(&lower) + volume(&upper) - volume(&a)).abs() < 1e-5);
    assert!((volume(&lower) - 2.8).abs() < 1e-5);
    validate(&lower).unwrap();
    validate(&upper).unwrap();
    let (lower, upper) = cut(&a, 0.7, false).unwrap();
    assert!(validate(&lower).is_err());
    assert!(validate(&upper).is_err());
}

#[test]
fn disconnected_shells_split_into_compact_meshes() {
    let a = cuboid([0.; 3], [2.; 3]).unwrap();
    let b = cuboid([3.; 3], [4.; 3]).unwrap();
    let mesh = boolean(&a, &b, BooleanOp::Union).unwrap();
    let parts = split_parts(&mesh).unwrap();
    assert_eq!(parts.len(), 2);
    assert!((parts.iter().map(volume).sum::<f64>() - 9.).abs() < 1e-5);
}

#[test]
fn open_or_nonmanifold_boolean_inputs_are_refused() {
    let a = cuboid([0.; 3], [2.; 3]).unwrap();
    let mut broken = a.clone();
    broken.faces.pop();
    assert_eq!(
        boolean(&a, &broken, BooleanOp::Union).unwrap_err(),
        ModelToolError::InvalidMesh
    );
    broken = a.clone();
    broken.faces.push(broken.faces[0]);
    assert_eq!(
        boolean(&a, &broken, BooleanOp::Union).unwrap_err(),
        ModelToolError::InvalidMesh
    );
    assert_eq!(
        boolean(&a, &sc_mesh_fixtures::bowtie_tetrahedra(), BooleanOp::Union).unwrap_err(),
        ModelToolError::InvalidMesh
    );
}

#[test]
fn cut_preserves_a_through_tunnel_and_does_not_cap_its_void() {
    let mesh = sc_mesh_fixtures::cuboid_with_z_tunnel();
    let (lower, upper) = cut(&mesh, 0.8, true).unwrap();
    assert!((volume(&lower) + volume(&upper) - volume(&mesh)).abs() < 1e-5);
    assert!((volume(&lower) - volume(&mesh) * 0.4).abs() < 1e-5);
    validate(&lower).unwrap();
    validate(&upper).unwrap();
}

#[test]
fn emboss_adds_and_engrave_removes_closed_material() {
    let mesh = cuboid([0.; 3], [20., 10., 3.]).unwrap();
    for engrave in [false, true] {
        let result = label(&mesh, "Sleptic", [2., 2., 3.], [0., 0., 1.], 3.5, 0.5, engrave).unwrap();
        validate(&result).unwrap();
        let delta = volume(&result) - volume(&mesh);
        println!(
            "R15 label engrave={engrave} original={:.6} result={:.6} delta={delta:.6} mm3",
            volume(&mesh),
            volume(&result)
        );
        assert!(if engrave { delta < -0.1 } else { delta > 0.1 }, "{delta}");
    }
}

#[test]
fn font_covers_printable_ascii_with_distinct_lowercase_and_refuses_other_text() {
    for c in 33..=126 {
        assert!(!crate::label_font::glyph(c).1.is_empty(), "{c}");
    }
    assert_ne!(
        crate::label_font::glyph(b'a'),
        crate::label_font::glyph(b'A')
    );
    let mesh = cuboid([0.; 3], [2.; 3]).unwrap();
    for text in ["", "\n", "\u{e9}"] {
        assert_eq!(
            label(&mesh, text, [0.; 3], [0., 0., 1.], 1., 0.1, false).unwrap_err(),
            ModelToolError::InvalidText
        );
    }
}

#[test]
fn emboss_refuses_detached_letters_instead_of_adding_loose_solids() {
    let mesh = cuboid([0.; 3], [20., 10., 3.]).unwrap();
    assert_eq!(
        label(&mesh, "T", [100., 100., 3.], [0., 0., 1.], 3.5, 0.5, false).unwrap_err(),
        ModelToolError::InvalidResult
    );
}

#[test]
fn outline_font_has_counter_and_proportional_advances() {
    let (advance, rings) = crate::label_font::glyph(b'O');
    assert_eq!(rings.len(), 2, "O must retain its inner counter");
    assert!(rings.iter().all(|ring| ring.len() > 12));
    assert!(advance > 0.0);
    assert!(crate::label_font::glyph(b'W').0 > crate::label_font::glyph(b'i').0);
    let width = |text: &str| {
        text.bytes()
            .map(|c| crate::label_font::glyph(c).0)
            .sum::<f64>()
    };
    assert!(width("Sleptic Sleptic") > width("Sleptic"));
}

#[test]
fn embossed_o_keeps_its_counter_open_above_the_base() {
    let mesh = cuboid([0.; 3], [20., 10., 3.]).unwrap();
    let result = label(&mesh, "O", [5., 5., 3.], [0., 0., 1.], 2., 0.2, false).unwrap();
    // The inner contour's mean lies inside the convex counter of Noto Sans O.
    let (_, rings) = crate::label_font::glyph(b'O');
    let area = |ring: &&Vec<[f64; 2]>| {
        ring.iter()
            .zip(ring.iter().cycle().skip(1))
            .take(ring.len())
            .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
            .sum::<f64>()
            .abs()
    };
    let inner = rings
        .iter()
        .min_by(|a, b| area(a).total_cmp(&area(b)))
        .unwrap();
    let point =
        [0, 1].map(|i| 5. + 2. * inner.iter().map(|p| p[i]).sum::<f64>() / inner.len() as f64);
    let cross = |a: [f64; 2], b: [f64; 2], p: [f64; 2]| {
        (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
    };
    let covers_counter = result.faces.iter().any(|face| {
        let vertices = face
            .vertices
            .map(|i| result.vertices[usize::from(i)].coords().map(f64::from));
        if !vertices.iter().all(|p| p[2] > 3.1) {
            return false;
        }
        let xy = vertices.map(|p| [p[0], p[1]]);
        let signs = [0, 1, 2].map(|i| cross(xy[i], xy[(i + 1) % 3], point));
        signs.iter().all(|v| *v >= -1e-10) || signs.iter().all(|v| *v <= 1e-10)
    });
    assert!(!covers_counter, "No raised cap may cover the counter in O");
}

#[test]
fn every_printable_outline_can_be_embossed_and_engraved_as_closed_material() {
    let mesh = cuboid([0.; 3], [20., 10., 3.]).unwrap();
    for c in 33u8..=126 {
        let text = char::from(c).to_string();
        for engrave in [false, true] {
            let result = label(&mesh, &text, [5., 5., 3.], [0., 0., 1.], 2.0, 0.2, engrave)
                .unwrap_or_else(|e| panic!("ASCII {c} ({text}), engrave={engrave}: {e}"));
            validate(&result).unwrap();
            assert!(if engrave {
                volume(&result) < volume(&mesh)
            } else {
                volume(&result) > volume(&mesh)
            });
        }
    }
}
