// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Prepare benchmark: auto orient + auto supports over a model library, one
//! CSV row per model, for comparing settings and algorithm changes. Scored
//! in Sleptic's order: (1) the part is held (no islands left), (2) the fewest
//! contacts touching the part, none of them unreachable, (3) the least
//! support material for supports that stay strong.
//!
//! Run explicitly (release, it slices every model):
//!   SLEPTIC_BENCH_DIR=<models>            folder of models
//!   SLEPTIC_BENCH_FILES=a.stl|b.stl       optional subset (default: every .stl)
//!   SLEPTIC_BENCH_LIMIT=20                optional cap on the model count
//!   SLEPTIC_BENCH_OUT=bench.csv           rows are appended here (and printed)
//!   SLEPTIC_BENCH_VARIANT=name            label for this run's settings
//!   SLEPTIC_BENCH_SET=k=v,k=v             SupportSettings overrides, plus
//!                                         weights=s;h;q;f for the orient weights
//!   SLEPTIC_BENCH_SUPPORT_SET=k=v,...     overrides for the supports only, so
//!                                         support styles compare on one pose
//!   SLEPTIC_BENCH_PRINTER=<profile id>    printer (default sleptic-diy-msla)
//!   SLEPTIC_BENCH_EXPORT=<dir>            supported plate and model as STL
//!   SLEPTIC_BENCH_TUNED=1                 Auto prepare's tuned support settings (R106)
//!   SLEPTIC_BENCH_POINTS=1                list contacts, columns and the
//!                                         unsupported pose's islands (with
//!                                         the pixels around each)
//!   cargo test -p sc-slicer-cli --release --test tests_r64_bench -- --ignored --nocapture
use sc_mesh_core::IndexedMesh;
use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
use std::io::Write;

const HEADER: &str = "variant,model,islands,contacts,unreachable,columns_on_part,support_ml,raft_ml,max_slenderness,print_min,height_mm,copies,seconds,error";

fn volume_mm3(mesh: &IndexedMesh) -> f64 {
    mesh.faces
        .iter()
        .map(|f| {
            let [a, b, c] = f.vertices.map(|i| {
                let v = mesh.vertices[usize::from(i)].coords();
                glam::DVec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]))
            });
            a.dot(b.cross(c)) / 6.0
        })
        .sum::<f64>()
        .abs()
}

/// Height over narrowest width: how easily a column bends or snaps.
fn slenderness(mesh: &IndexedMesh) -> f64 {
    let Some(bb) = mesh.local_aabb() else {
        return 0.0;
    };
    let (lo, hi) = (bb.min(), bb.max());
    let width = f64::from((hi[0] - lo[0]).min(hi[1] - lo[1])).max(1e-3);
    f64::from(hi[2] - lo[2]) / width
}

fn apply(
    settings: &mut sc_print_profiles::support_settings::SupportSettings,
    weights: &mut [f64; 4],
    spec: &str,
) {
    for pair in spec.split(',').filter(|p| !p.trim().is_empty()) {
        let (key, value) = pair.split_once('=').expect("k=v");
        let (key, value) = (key.trim(), value.trim());
        let number = || value.parse::<f64>().expect(key);
        let flag = || matches!(value, "1" | "true" | "yes");
        match key {
            "weights" => {
                for (w, v) in weights.iter_mut().zip(value.split(';')) {
                    *w = v.parse().expect("weight");
                }
            }
            "overhang_angle_deg" => settings.overhang_angle_deg = number(),
            "spacing_mm" => settings.spacing_mm = number(),
            "tip_contact_diameter_mm" => settings.tip_contact_diameter_mm = number(),
            "tip_length_mm" => settings.tip_length_mm = number(),
            "shaft_diameter_mm" => settings.shaft_diameter_mm = number(),
            "base_diameter_mm" => settings.base_diameter_mm = number(),
            "min_column_diameter_mm" => settings.min_column_diameter_mm = number(),
            "max_column_diameter_mm" => settings.max_column_diameter_mm = number(),
            "lift_mm" => settings.lift_mm = number(),
            "raft_margin_mm" => settings.raft_margin_mm = number(),
            "raft" => settings.raft = flag(),
            "lattice_raft" => settings.lattice_raft = flag(),
            "skate_raft" => settings.skate_raft = flag(),
            "contact_depth_mm" => settings.contact_depth_mm = number(),
            "tree_supports" => settings.tree_supports = flag(),
            "branch_angle_deg" => settings.branch_angle_deg = number(),
            other => panic!("unknown setting {other}"),
        }
    }
}

#[test]
#[ignore = "local model library; slices every model"]
fn r64_prepare_benchmark() {
    let dir =
        std::path::PathBuf::from(std::env::var("SLEPTIC_BENCH_DIR").expect("SLEPTIC_BENCH_DIR"));
    let mut names: Vec<String> = match std::env::var("SLEPTIC_BENCH_FILES") {
        Ok(list) => list.split('|').map(str::to_owned).collect(),
        Err(_) => {
            let mut all: Vec<String> = std::fs::read_dir(&dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.to_ascii_lowercase().ends_with(".stl"))
                .collect();
            all.sort();
            all
        }
    };
    if let Some(limit) = std::env::var("SLEPTIC_BENCH_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        names.truncate(limit);
    }
    let variant = std::env::var("SLEPTIC_BENCH_VARIANT").unwrap_or_else(|_| "default".into());
    let envelope = sc_print_primitives::BuildVolume::new(192.0, 120.0, 200.0).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let printer = sc_print_profiles::load_printer_profile_from_path(&root.join(format!(
        "assets/profiles/printers/{}.toml",
        std::env::var("SLEPTIC_BENCH_PRINTER").unwrap_or_else(|_| "sleptic-diy-msla".into())
    )))
    .unwrap();
    let material = sc_print_profiles::load_material_profile_from_path(
        &root.join("assets/profiles/materials/standard-405.toml"),
    )
    .unwrap()
    .into();
    let mut context =
        sc_print_glue::orient::Context::for_profiles(&printer, &material, &Default::default())
            .unwrap();
    let mut weights = sc_print_glue::orient::Preset::Balanced.weights();
    if let Ok(spec) = std::env::var("SLEPTIC_BENCH_SET") {
        apply(&mut context.support, &mut weights, &spec);
    }
    let mut support = context.support;
    if let Ok(spec) = std::env::var("SLEPTIC_BENCH_SUPPORT_SET") {
        apply(&mut support, &mut [0.0; 4], &spec);
    }
    let mut out = std::env::var("SLEPTIC_BENCH_OUT").ok().map(|path| {
        let fresh = !std::path::Path::new(&path).exists();
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        if fresh {
            writeln!(file, "{HEADER}").unwrap();
        }
        file
    });
    println!("{HEADER}");
    for name in names {
        let start = std::time::Instant::now();
        let result = (|| -> Result<String, String> {
            let mut scene = sc_project_formats::load_scene(&dir.join(&name), None, |_| {})
                .map_err(|e| format!("load: {e}"))?
                .scene;
            scene.repair_meshes(sc_mesh_core::indexed_mesh::RepairPasses::default_repair());
            scene
                .drop_at([0.0, 0.0], 0.0)
                .map_err(|e| format!("{e:?}"))?;
            let id = *scene
                .reachable_leaves(
                    sc_scene::CsgPolicy::Descend,
                    sc_scene::ReferencePolicy::Skip,
                )
                .map_err(|e| format!("{e:?}"))?
                .first()
                .ok_or("no model")?;
            let mesh = sc_scene::bake_leaf_to_world(&scene, id).map_err(|e| format!("{e:?}"))?;
            let size = mesh.local_aabb().ok_or("empty")?;
            // A model too big for the plate is shrunk until its longest side
            // is 110 mm: it then fits every way round with room for a raft,
            // so orient is not forced to stand it up (a flat drone frame
            // scaled to the full 118 mm plate depth could only fit standing).
            let longest = (0..3)
                .map(|axis| f64::from(size.max()[axis] - size.min()[axis]))
                .fold(0.0, f64::max);
            let fits = f64::from(size.max()[0] - size.min()[0]) <= 190.0
                && f64::from(size.max()[1] - size.min()[1]) <= 118.0
                && f64::from(size.max()[2] - size.min()[2]) <= 195.0;
            let scale = if fits {
                1.0
            } else {
                (110.0 / longest).min(1.0)
            };
            // SLEPTIC_BENCH_SCALE=<factor> sets the scale outright.
            let scale = std::env::var("SLEPTIC_BENCH_SCALE")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(scale);
            if (scale - 1.0).abs() > 1e-9 {
                let before = scene.node(id).unwrap().local_transform;
                scene
                    .set_local_transform(
                        id,
                        Transform3D::Affine(AffineTransform3D::from_uniform_scale(scale))
                            .compose(&before),
                    )
                    .map_err(|e| format!("{e:?}"))?;
                scene
                    .drop_at([0.0, 0.0], 0.0)
                    .map_err(|e| format!("{e:?}"))?;
            }
            let oriented = std::time::Instant::now();
            let proposal = sc_print_glue::orient::search_with_context(
                &scene,
                id,
                envelope,
                context.support.overhang_angle_deg,
                weights,
                context,
            )
            .ok_or("orient found nothing")?;
            let orient_seconds = oriented.elapsed().as_secs_f64();
            // SLEPTIC_BENCH_EXPLAIN=1: orient's cost terms for the chosen pose
            // and the shortest one, to see why one won.
            if std::env::var("SLEPTIC_BENCH_EXPLAIN").is_ok() {
                for (label, m) in [("chosen", proposal.chosen), ("shortest", proposal.shortest)] {
                    let c = sc_print_glue::orient::user_costs(m, proposal.shortest_minutes);
                    let total: f64 = c.iter().zip(weights).map(|(c, w)| c * w).sum();
                    eprintln!(
                        "EXPLAIN {name} {label}: costs {c:.1?} weighted {total:.1} contacts {:.1} h {:.1} copies {} zero {} anchored {} min {:.0} peak {:.0} bed {:.0}",
                        m.contacts,
                        m.height,
                        m.copies,
                        m.zero_support,
                        m.anchored,
                        m.print_minutes,
                        m.peak_area,
                        m.bed_contact_area
                    );
                }
            }
            // SLEPTIC_BENCH_TILT=<x deg>;<y deg> tilts the loaded model instead
            // of orienting it, the way a resin print is usually angled.
            if let Ok(tilt) = std::env::var("SLEPTIC_BENCH_TILT") {
                let mut angles = tilt
                    .split(';')
                    .map(|v| v.parse::<f64>().expect("tilt").to_radians());
                let turn = AffineTransform3D::from_rotation_y(angles.clone().nth(1).unwrap_or(0.0))
                    .compose(&AffineTransform3D::from_rotation_x(
                        angles.next().unwrap_or(0.0),
                    ));
                let before = scene.node(id).unwrap().local_transform;
                scene
                    .set_local_transform(id, Transform3D::Affine(turn).compose(&before))
                    .map_err(|e| format!("{e:?}"))?;
                scene
                    .drop_at([0.0, 0.0], 0.0)
                    .map_err(|e| format!("{e:?}"))?;
            } else if std::env::var("SLEPTIC_BENCH_NO_ORIENT").is_err() {
                let before = scene.node(id).unwrap().local_transform;
                scene
                    .set_local_transform(id, proposal.delta.compose(&before))
                    .map_err(|e| format!("{e:?}"))?;
            }
            let profile = sc_print_glue::slice_profile_for_selection(
                &scene,
                &printer,
                &material,
                None,
                None,
                // Anti-aliasing off, as Sleptic's printer runs (an X4 benchmark
                // hid 31 extra contacts on a universal joint);
                // SLEPTIC_BENCH_AA=x4 for 4x.
                if std::env::var("SLEPTIC_BENCH_AA").is_ok_and(|v| v == "x4") {
                    sc_sl1cer::profile::AaLevel::X4
                } else {
                    sc_sl1cer::profile::AaLevel::Off
                },
                sc_sl1cer::profile::PackingMode::None,
            )
            .map_err(|e| format!("{e:?}"))?;
            // SLEPTIC_BENCH_POINTS=1 also lists the islands of the oriented,
            // unsupported part (layer height, area, plate position).
            if std::env::var("SLEPTIC_BENCH_POINTS").is_ok()
                && let Ok(sc_sl1cer::SliceOutcome::Completed(leaves)) =
                    sc_sl1cer::api::slice_scene_with_fallback(
                        &scene,
                        &profile,
                        &sc_sl1cer::CancelToken::new(),
                        |_| {},
                        true,
                    )
                && let Ok(stack) = sc_sl1cer::contour::merge_stacks(
                    leaves.into_iter().map(|l| l.into_stack()).collect(),
                )
            {
                let map = profile.pixel_map();
                for island in sc_print_job::find_islands(&stack, &profile) {
                    let z = profile
                        .layer_schedule()
                        .iter_slab_tops()
                        .nth(island.layer)
                        .unwrap_or(0.0);
                    let at = map.pixel_to_mm(island.centroid_px);
                    eprintln!(
                        "ISLAND {name}: layer {} z {z:.2} area {:.3} mm2 at {at:.2?}",
                        island.layer, island.area_mm2
                    );
                    // The lit pixels around it in its layer (#) and the one
                    // below (+ where only the layer below is lit).
                    let width = profile.pixel_resolution().x() as usize;
                    let lit = |layer: usize| {
                        let raster = sc_sl1cer::raster::accuracy::rasterize_accurate(
                            &stack.layers()[layer],
                            &map,
                            profile.aa_level(),
                            0.0,
                            profile.accuracy(),
                            layer,
                        );
                        let mut on = std::collections::HashSet::new();
                        let mut at = 0usize;
                        for run in raster.runs() {
                            if run.value > 127 {
                                for k in at..at + run.length as usize {
                                    on.insert((k % width, k / width));
                                }
                            }
                            at += run.length as usize;
                        }
                        on
                    };
                    let (here, below) = (lit(island.layer), lit(island.layer - 1));
                    let (cx, cy) = (island.contact_px.0 as i64, island.contact_px.1 as i64);
                    for y in cy - 12..=cy + 12 {
                        let row: String = (cx - 30..=cx + 30)
                            .map(|x| {
                                let p = (x.max(0) as usize, y.max(0) as usize);
                                match (here.contains(&p), below.contains(&p)) {
                                    (true, _) if (x, y) == (cx, cy) => '@',
                                    (true, true) => '#',
                                    (true, false) => 'o',
                                    (false, true) => '+',
                                    _ => '.',
                                }
                            })
                            .collect();
                        eprintln!("  {row}");
                    }
                }
            }
            // SLEPTIC_BENCH_TUNED=1: Auto prepare's tuned settings for the
            // part as it now sits (R106), as the app's Auto prepare uses them.
            let support = if std::env::var("SLEPTIC_BENCH_TUNED").is_ok() {
                let volume = profile.build_volume();
                let part = sc_scene::bake_leaf_to_world(&scene, id).map_err(|e| format!("{e:?}"))?;
                let choice = sc_print_glue::supports::choose_supports(
                    &[part],
                    volume.x_mm() * volume.y_mm(),
                    &support,
                );
                eprintln!("TUNED {name}: load {:.2} {:?}", choice.load, choice.settings);
                choice.settings
            } else {
                support
            };
            let supported = std::time::Instant::now();
            let (generated, structure, analysis) =
                sc_print_glue::supports::auto_supported_scene_analyzed_with_fallback(
                    &scene, &support, &profile, true,
                )
                .map_err(|e| format!("supports: {e:?}"))?;
            eprintln!(
                "TIME {name}: orient {orient_seconds:.1} s, supports {:.1} s",
                supported.elapsed().as_secs_f64()
            );
            let lifted =
                sc_scene::bake_leaf_to_world(&generated, id).map_err(|e| format!("{e:?}"))?;
            // SLEPTIC_BENCH_EXPORT=<dir>: the supported plate as one STL, to
            // look at in the app.
            if let Ok(dir) = std::env::var("SLEPTIC_BENCH_EXPORT") {
                let merged = generated
                    .merge_meshes()
                    .map_err(|e| format!("{e:?}"))?
                    .ok_or("empty plate")?;
                let path = std::path::Path::new(&dir).join(format!("{variant}_{name}"));
                let mut file = std::io::BufWriter::new(
                    std::fs::File::create(path).map_err(|e| e.to_string())?,
                );
                sc_mesh_formats::stl::write_stlb(&merged, &mut file)
                    .map_err(|e| format!("{e:?}"))?;
                // The supports alone, columns and raft as separate files.
                for (tag, meshes) in [
                    ("columns", structure.columns.clone()),
                    ("raft", structure.raft.iter().cloned().collect::<Vec<_>>()),
                ] {
                    let path = std::path::Path::new(&dir).join(format!("{variant}_{tag}_{name}"));
                    let mut file = std::io::BufWriter::new(
                        std::fs::File::create(path).map_err(|e| e.to_string())?,
                    );
                    let mut all = Vec::new();
                    for mesh in &meshes {
                        for f in &mesh.faces {
                            all.push(f.vertices.map(|i| mesh.vertices[usize::from(i)].coords()));
                        }
                    }
                    file.write_all(&[0u8; 80]).map_err(|e| e.to_string())?;
                    file.write_all(&(all.len() as u32).to_le_bytes())
                        .map_err(|e| e.to_string())?;
                    for t in all {
                        file.write_all(&[0u8; 12]).map_err(|e| e.to_string())?;
                        for v in t {
                            for c in v {
                                file.write_all(&c.to_le_bytes())
                                    .map_err(|e| e.to_string())?;
                            }
                        }
                        file.write_all(&[0u8; 2]).map_err(|e| e.to_string())?;
                    }
                }
                let path = std::path::Path::new(&dir).join(format!("{variant}_model_{name}"));
                let mut file = std::io::BufWriter::new(
                    std::fs::File::create(path).map_err(|e| e.to_string())?,
                );
                sc_mesh_formats::stl::write_stlb(&lifted, &mut file)
                    .map_err(|e| format!("{e:?}"))?;
            }
            // SLEPTIC_BENCH_POINTS=1: every contact (position, surface normal)
            // and each column's bounding box, to see where supports went.
            if std::env::var("SLEPTIC_BENCH_POINTS").is_ok() {
                for p in &structure.points {
                    eprintln!(
                        "POINT {name}: at {:.2?} normal {:.2?}",
                        p.position, p.normal
                    );
                }
                for (i, c) in structure.columns.iter().enumerate() {
                    if let Some(b) = c.local_aabb() {
                        eprintln!("COLUMN {name} {i}: {:.2?} .. {:.2?}", b.min(), b.max());
                    }
                }
            }
            let (_, unreachable) =
                sc_print_glue::supports::analyze_contact_access(&lifted, &structure.points, 0.06)
                    .map_err(|e| format!("{e:?}"))?;
            // Columns standing on a part (a second contact mark): their foot
            // is above the plate or raft top.
            let floor = structure.raft.as_ref().map_or(0.0, |raft| {
                raft.vertices.iter().map(|v| v[2]).fold(0.0f32, f32::max)
            });
            let drops = structure
                .columns
                .iter()
                .filter(|c| {
                    c.vertices
                        .iter()
                        .map(|v| v[2])
                        .fold(f32::INFINITY, f32::min)
                        > floor + 0.05
                })
                .count();
            let support: f64 = structure.columns.iter().map(volume_mm3).sum();
            let raft = structure.raft.as_ref().map_or(0.0, volume_mm3);
            let slender = structure
                .columns
                .iter()
                .map(slenderness)
                .fold(0.0, f64::max);
            let m = proposal.chosen;
            Ok(format!(
                "{},{},{},{},{:.3},{:.3},{:.1},{:.1},{:.1},{}",
                analysis.islands.len(),
                structure.points.len(),
                unreachable,
                drops,
                support / 1000.0,
                raft / 1000.0,
                slender,
                m.print_minutes,
                m.height,
                m.copies
            ))
        })();
        let seconds = start.elapsed().as_secs_f64();
        let row = match result {
            Ok(metrics) => format!(
                "{variant},{},{metrics},{seconds:.1},",
                name.replace(',', ";")
            ),
            Err(error) => format!(
                "{variant},{},,,,,,,,,,,{seconds:.1},{}",
                name.replace(',', ";"),
                error.replace(',', ";")
            ),
        };
        println!("{row}");
        if let Some(file) = out.as_mut() {
            writeln!(file, "{row}").unwrap();
        }
    }
}
