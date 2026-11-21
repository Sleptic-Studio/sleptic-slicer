// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Automatic support worker and UI integration.
use crate::{
    MainWindow,
    preferences_bus::PreferenceHub,
    scene_document::{SceneDocument, Revision},
};
use sc_print_profiles::support_settings::SupportSettings;
use sc_scene::Scene;
use slint::ComponentHandle;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

type Outcome = Result<(Scene, usize, usize, String, String), String>;
pub(crate) struct AutoSupportRun {
    ui: slint::Weak<MainWindow>,
    document: Rc<SceneDocument>,
    bus: Rc<PreferenceHub>,
    timer: slint::Timer,
    base: RefCell<Option<(Scene, Revision)>>,
    lift_mm: Cell<f64>,
    subscription: RefCell<Option<crate::scene_document::Watch>>,
}

pub(crate) fn settings(ui: &MainWindow) -> Result<SupportSettings, String> {
    let number = |s: slint::SharedString| {
        s.parse::<f64>()
            .map_err(|_| crate::i18n::tr!("Enter a valid support setting.").to_string())
    };
    let result = SupportSettings {
        overhang_angle_deg: number(ui.get_support_overhang_angle_deg())?,
        spacing_mm: number(ui.get_support_spacing_mm())?,
        tip_contact_diameter_mm: number(ui.get_support_tip_contact_diameter_mm())?,
        tip_length_mm: number(ui.get_support_tip_length_mm())?,
        shaft_diameter_mm: number(ui.get_support_shaft_diameter_mm())?,
        base_diameter_mm: number(ui.get_support_base_diameter_mm())?,
        base_height_mm: number(ui.get_support_base_height_mm())?,
        raft_thickness_mm: number(ui.get_support_raft_thickness_mm())?,
        raft_margin_mm: number(ui.get_support_raft_margin_mm())?,
        lift_mm: number(ui.get_support_lift_mm())?,
        raft: ui.get_support_raft(),
        min_column_diameter_mm: number(ui.get_support_min_column_diameter_mm())?,
        max_column_diameter_mm: number(ui.get_support_max_column_diameter_mm())?,
        symmetric_supports: ui.get_support_symmetric(),
        tree_supports: ui.get_support_tree(),
        branch_angle_deg: number(ui.get_support_branch_angle_deg())?,
        lattice_raft: ui.get_support_lattice_raft(),
        skate_raft: ui.get_support_skate_raft(),
        contact_depth_mm: number(ui.get_support_contact_depth_mm())?,
        ..SupportSettings::default()
    };
    result.validate().map_err(|e| e.to_string())?;
    Ok(result)
}

impl AutoSupportRun {
    pub(crate) fn install(
        ui: &MainWindow,
        document: Rc<SceneDocument>,
        bus: Rc<PreferenceHub>,
    ) -> Rc<Self> {
        let run = Rc::new(Self {
            ui: ui.as_weak(),
            document,
            bus,
            timer: slint::Timer::default(),
            base: RefCell::new(None),
            lift_mm: Cell::new(0.0),
            subscription: RefCell::new(None),
        });
        let weak = ui.as_weak();
        ui.on_support_preset_selected(move |index| {
            if let Some(ui) = weak.upgrade() {
                let mut settings = SupportSettings::default();
                use sc_print_profiles::support_settings::SupportPreset;
                settings.apply_preset(match index {
                    0 => SupportPreset::Light,
                    2 => SupportPreset::Heavy,
                    _ => SupportPreset::Medium,
                });
                ui.set_support_tip_contact_diameter_mm(
                    settings.tip_contact_diameter_mm.to_string().into(),
                );
                ui.set_support_tip_length_mm(settings.tip_length_mm.to_string().into());
                ui.set_support_shaft_diameter_mm(settings.shaft_diameter_mm.to_string().into());
                ui.set_support_contact_depth_mm(settings.contact_depth_mm.to_string().into());
                ui.set_support_min_column_diameter_mm(
                    settings.min_column_diameter_mm.to_string().into(),
                );
                ui.set_support_max_column_diameter_mm(
                    settings.max_column_diameter_mm.to_string().into(),
                );
            }
        });
        let action = run.clone();
        ui.on_auto_supports_requested(move || action.start(false));
        let action = run.clone();
        ui.on_auto_supports_object_requested(move || action.start(true));
        let action = run.clone();
        ui.on_remove_supports_requested(move || action.remove_all());
        let action = run.clone();
        ui.on_remove_row_supports_requested(move |row| action.remove_for_row(row as usize));
        let action = run.clone();
        ui.on_support_contact_requested(move |x, y, z, id, remove| {
            action.contact([x, y, z], id, remove)
        });
        let action = run.clone();
        ui.on_support_paint_requested(move |x, y, z, id, avoid| action.paint([x, y, z], id, avoid));
        let weak = Rc::downgrade(&run);
        let subscription = run.document.watch(move |event| {
            let Some(run) = weak.upgrade() else {
                return;
            };
            let Some(ui) = run.ui.upgrade() else {
                return;
            };
            let count = run
                .document
                .read_scene(|s| {
                    support_ids(s.graph())
                        .iter()
                        .filter(|id| {
                            s.graph()
                                .node(**id)
                                .and_then(|n| n.meta.as_ref())
                                .and_then(|m| m.comment.as_deref())
                                .is_none_or(|c| !c.starts_with("sc-generated-support-raft"))
                        })
                        .count()
                })
                .unwrap_or(0);
            ui.set_supports_count(count as i32);
            ui.set_supports_stale(supports_out_of_date(&run.document));
            if matches!(
                event,
                crate::scene_document::DocumentEvent::Opened
                    | crate::scene_document::DocumentEvent::Emptied
            ) {
                run.base.borrow_mut().take();
                if count > 0 {
                    ui.set_supports_enabled(true);
                    let lift = run
                        .document
                        .read_scene(|s| {
                            support_ids(s.graph()).into_iter().find_map(|id| {
                                let text = s
                                    .graph()
                                    .node(id)?
                                    .meta
                                    .as_ref()?
                                    .comment
                                    .as_ref()?
                                    .split_once('\n')?
                                    .1;
                                let data: toml::Value = toml::from_str(text).ok()?;
                                data.get("lift_mm")?
                                    .as_float()
                                    .or_else(|| data.get("lift_mm")?.as_integer().map(|v| v as f64))
                                    // Plate-anchored models record 0; the
                                    // setting comes from a lifted one.
                                    .filter(|lift| *lift > 0.0)
                            })
                        })
                        .flatten()
                        .unwrap_or(5.0);
                    run.lift_mm.set(lift);
                    ui.set_support_lift_mm(lift.to_string().into());
                }
            }
        });
        *run.subscription.borrow_mut() = Some(subscription);
        run
    }
    fn paint(&self, position: [f32; 3], hit: i32, avoid: bool) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if ui.get_making_supports() || ui.get_slicing() {
            return;
        }
        let Ok(radius) = ui.get_support_brush_radius_mm().parse::<f32>() else {
            return;
        };
        if !radius.is_finite() || !(0.1..=100.0).contains(&radius) {
            return;
        }
        let edit = self
            .document
            .read_scene(|spatial| {
                let scene = spatial.graph();
                let id = spatial
                    .leaf_ids()
                    .into_iter()
                    .find(|id| id.get() as i32 == hit)?;
                let comment = scene
                    .node(id)?
                    .meta
                    .as_ref()
                    .and_then(|m| m.comment.as_deref())
                    .unwrap_or("");
                if sc_print_glue::supports::painted_regions(comment).len() >= 256 {
                    ui.set_supports_error(crate::i18n::tr!(
                        "Support brush limit reached (256). Undo strokes to repaint."
                    ));
                    return None;
                }
                painted_scene_edit(scene, id, position, radius, avoid)
            })
            .flatten();
        if let Some(edit) = edit
            && self.document.commit_edit(edit.clone()).is_ok()
        {
            self.document.record_edit(edit);
            self.base.borrow_mut().take();
        }
    }

    fn contact(&self, position: [f32; 3], id: i32, remove: bool) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if ui.get_making_supports() || ui.get_slicing() {
            return;
        }
        let Some(mut scene) = self.document.read_scene(|s| s.graph().clone()) else {
            return;
        };
        let revision = self.document.revision();
        let tracked = self
            .base
            .borrow()
            .as_ref()
            .filter(|(_, r)| r.same_document(revision))
            .map(|(s, r)| {
                if *r == revision {
                    s.clone()
                } else if support_ids(&scene).is_empty() {
                    scene.clone()
                } else {
                    unlifted_model(&scene, self.lift_mm.get())
                }
            });
        let original = tracked.clone().unwrap_or_else(|| {
            if support_ids(&scene).is_empty() {
                scene.clone()
            } else {
                unlifted_model(&scene, self.lift_mm.get())
            }
        });
        let is_support = |scene: &Scene, id| {
            scene
                .node(id)
                .and_then(|n| n.meta.as_ref())
                .and_then(|m| m.comment.as_deref())
                .is_some_and(|s| s.starts_with("sc-generated-support"))
        };
        let Some(hit) = self
            .document
            .read_scene(|s| {
                s.leaf_ids()
                    .into_iter()
                    .find(|node| node.get() as i32 == id)
            })
            .flatten()
        else {
            return;
        };
        let roots: Vec<_> = scene
            .roots()
            .iter()
            .copied()
            .filter(|n| !is_support(&scene, *n))
            .collect();
        let count = ui.get_supports_count();
        if remove {
            if !is_support(&scene, hit) {
                return;
            }
            if scene.remove_leaf(hit).is_err() {
                return;
            }
        } else {
            if is_support(&scene, hit) {
                return;
            }
            let settings = match settings(&ui) {
                Ok(s) => s,
                Err(e) => {
                    ui.set_supports_error(e.into());
                    return;
                }
            };
            let lift = count == 0 && tracked.is_none();
            match manual_contact_scene(&scene, position, &settings, lift) {
                Ok(generated) => {
                    scene = generated;
                    if lift {
                        self.lift_mm.set(settings.lift_mm);
                    }
                }
                Err(error) => {
                    ui.set_supports_error(error.into());
                    return;
                }
            }
        }
        match self.document.apply_support_scene(&scene, &roots) {
            Ok(()) => {
                *self.base.borrow_mut() = Some((original, self.document.revision()));
                ui.set_supports_error("".into());
            }
            Err(e) => ui.set_supports_error(apply_error_text(&e)),
        }
    }
    /// Remove every generated support and raft on the active plate and lower
    /// the models back onto the plate, as one undoable edit.
    /// Remove only the supports (and its raft) of the model owning object-list
    /// support row `row`, lowering that model; other models keep theirs.
    pub(crate) fn remove_for_row(self: &Rc<Self>, row: usize) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if ui.get_making_supports() || ui.get_slicing() {
            return;
        }
        let Some(owner) = self
            .document
            .read_scene(|s| crate::viewport::support_row_owner(s.graph(), row))
            .flatten()
        else {
            return;
        };
        let Some(mut source) = self
            .document
            .read_scene(|s| s.graph().plate_scene(self.document.active_plate()).ok())
            .flatten()
        else {
            return;
        };
        // Keep just this model and the supports it owns, then unlift it.
        let tag = format!("owner = {}", owner.get());
        let keep = |scene: &Scene, id: sc_scene::NodeId| {
            id == owner
                || scene
                    .node(id)
                    .and_then(|n| n.meta.as_ref())
                    .and_then(|m| m.comment.as_deref())
                    .is_some_and(|c| c.starts_with("sc-generated-support") && c.lines().any(|l| l == tag))
        };
        let others: Vec<_> = source
            .reachable_leaves(
                sc_scene::CsgPolicy::Descend,
                sc_scene::ReferencePolicy::Skip,
            )
            .unwrap_or_default()
            .into_iter()
            .filter(|&id| !keep(&source, id))
            .collect();
        for id in others.into_iter().rev() {
            let _ = source.remove_leaf(id);
        }
        if support_ids(&source).is_empty() {
            return;
        }
        let lift = match self.lift_mm.get() {
            l if l > 0.0 => l,
            _ => settings(&ui).map_or(0.0, |s| s.lift_mm),
        };
        let bare = unlifted_model(&source, lift);
        match self
            .document
            .apply_support_scene_scoped(&bare, &[owner], Some(owner))
        {
            Ok(()) => {
                self.base.borrow_mut().take();
                ui.set_supports_error("".into());
                ui.set_plate_action_message(crate::i18n::tr!("Supports removed").into());
            }
            Err(e) => ui.set_supports_error(apply_error_text(&e)),
        }
    }

    pub(crate) fn remove_all(self: &Rc<Self>) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if ui.get_making_supports() || ui.get_slicing() {
            return;
        }
        let Some(current) = self
            .document
            .read_scene(|s| s.graph().plate_scene(self.document.active_plate()).ok())
            .flatten()
        else {
            return;
        };
        if support_ids(&current).is_empty() {
            return;
        }
        let lift = match self.lift_mm.get() {
            l if l > 0.0 => l,
            _ => settings(&ui).map_or(0.0, |s| s.lift_mm),
        };
        let bare = unlifted_model(&current, lift);
        let roots = bare.roots().to_vec();
        match self.document.apply_support_scene_scoped(&bare, &roots, None) {
            Ok(()) => {
                self.base.borrow_mut().take();
                ui.set_supports_error("".into());
                ui.set_plate_action_message(crate::i18n::tr!("Supports removed").into());
            }
            Err(e) => ui.set_supports_error(apply_error_text(&e)),
        }
    }

    fn start(self: &Rc<Self>, selected_only: bool) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        if ui.get_making_supports() || ui.get_slicing() {
            return;
        }
        // Auto prepare's tuned settings, once; otherwise the user's (R106).
        let tuned = self.document.auto_support_settings.borrow_mut().take();
        let settings = match tuned.map_or_else(|| settings(&ui), Ok) {
            Ok(s) => s,
            Err(e) => {
                ui.set_supports_error(e.into());
                return;
            }
        };
        let revision = self.document.revision();
        let Some(current) = self
            .document
            .read_scene(|s| s.graph().plate_scene(self.document.active_plate()).ok())
            .flatten()
        else {
            return;
        };
        let mut source = self
            .base
            .borrow()
            .as_ref()
            .filter(|(_, r)| r.same_document(revision))
            .map(|(s, r)| {
                if *r == revision {
                    s.clone()
                } else if support_ids(&current).is_empty() {
                    current.clone()
                } else {
                    unlifted_model(&current, self.lift_mm.get())
                }
            })
            .unwrap_or_else(|| {
                if support_ids(&current).is_empty() {
                    current.clone()
                } else {
                    unlifted_model(&current, settings.lift_mm)
                }
            });
        source = source
            .plate_scene(self.document.active_plate())
            .unwrap_or(source);
        let selected = selected_only
            .then(|| self.document.main_chosen_node())
            .flatten();
        if selected_only && selected.is_none() {
            return;
        }
        if let Some(selected) = selected {
            let ids = source
                .reachable_leaves(
                    sc_scene::CsgPolicy::Descend,
                    sc_scene::ReferencePolicy::Skip,
                )
                .unwrap_or_default();
            for id in ids.into_iter().rev().filter(|&id| id != selected) {
                let _ = source.remove_leaf(id);
            }
        }
        let snapshot = self.bus.current_values();
        let printer = self
            .document
            .chosen_printer()
            .or(snapshot.default_printer);
        let resin = self.document.chosen_resin().or(snapshot.default_resin);
        let (Some(printer), Some(resin)) = (printer, resin) else {
            ui.set_supports_error(crate::i18n::tr!("Select a printer and resin first."));
            return;
        };
        let overrides = *self.document.process_overrides.borrow();
        let first_layer_height = self.document.chosen_first_layer();
        let aa = self.document.scene_aa.get().unwrap_or(snapshot.aa_level);
        let original = source.clone();
        let roots = source.roots().to_vec();
        let slot: Arc<Mutex<Option<Outcome>>> = Arc::new(Mutex::new(None));
        let worker = slot.clone();
        ui.set_making_supports(true);
        ui.set_supports_error("".into());
        std::thread::spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Outcome {
                let source = repaired_support_source(&source);
                let (printer, resin) =
                    crate::slice_projection::load_chosen_profiles(&printer, &resin)
                        .map_err(|e| e.to_string())?;
                // The profile is sized from the model itself: the support
                // pass extends its layer count to the lifted height. A whole
                // geometric support pass just to size it doubled the work
                // (Sleptic: auto supports were "often very slow").
                let profile = sc_print_glue::slice_profile_for_selection(
                    &source,
                    &printer,
                    &resin,
                    overrides.layer_height_mm,
                    first_layer_height,
                    aa,
                    sc_sl1cer::profile::PackingMode::None,
                )
                .map_err(|e| e.to_string())?;
                let profile = sc_print_glue::with_accuracy_for_selection(
                    profile, &printer, &resin, &overrides,
                );
                let volume = profile.build_volume();
                if let Some(text) = too_big_text(
                    &model_sizes(&source),
                    [volume.x_mm(), volume.y_mm(), volume.z_mm()],
                ) {
                    return Err(text);
                }
                let (scene, structure, remaining) =
                    sc_print_glue::supports::auto_supported_scene_analyzed_with_fallback(
                        &source, &settings, &profile, true,
                    )
                    .map_err(|e| e.to_string())?;
                let results =
                    sc_print_glue::supports::model_results(&scene, &structure, &settings);
                Ok((
                    scene,
                    // Pieces, as the object list and the rescan count them
                    // (R95: the panel said 17 after generating, 14 after an edit).
                    structure.columns.len(),
                    remaining.islands.len(),
                    support_result_text(&results),
                    support_notice_text(&results),
                ))
            }))
            .unwrap_or_else(|_| Err(crate::i18n::tr!("Support worker panicked").to_string()));
            *worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome);
        });
        let run = self.clone();
        self.timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(100),
            move || {
                let outcome = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
                let Some(outcome) = outcome else {
                    return;
                };
                run.timer.stop();
                let Some(ui) = run.ui.upgrade() else {
                    return;
                };
                ui.set_making_supports(false);
                if run.document.revision() != revision {
                    ui.set_supports_error(crate::i18n::tr!(
                        "The scene changed during support generation. Generate again."
                    ));
                    return;
                }
                match outcome {
                    Ok((scene, count, remaining, summary, notice)) => {
                        if let Err(e) = run
                            .document
                            .apply_support_scene_scoped(&scene, &roots, selected)
                        {
                            ui.set_supports_error(apply_error_text(&e));
                            return;
                        }
                        run.lift_mm.set(settings.lift_mm);
                        if selected.is_none() {
                            *run.base.borrow_mut() =
                                Some((original.clone(), run.document.revision()));
                        }
                        // Generated for this printer's pixels and layers (R108).
                        run.document
                            .supports_printer
                            .replace(run.document.chosen_printer());
                        if selected.is_none() {
                            ui.set_supports_count(count as i32);
                            ui.set_supports_stale(supports_out_of_date(&run.document));
                        }
                        ui.set_plate_action_message(summary.into());
                        show_support_notice(&ui, notice);
                        if remaining > 0 {
                            ui.set_supports_error(crate::i18n::tr!(
                                "Some islands remain; re-slice to inspect them."
                            ));
                        }
                    }
                    Err(e) => ui.set_supports_error(e.into()),
                }
            },
        );
    }
}

/// The banner shown when a part needs no supports at all (Sleptic: Generate
/// supports and Auto prepare "awkwardly just doing nothing"; the status bar
/// line was easy to miss). Empty when every part got supports.
pub(crate) fn support_notice_text(
    results: &[sc_print_glue::supports::ModelSupportResult],
) -> String {
    let direct: Vec<&str> = results
        .iter()
        .filter(|r| r.direct)
        .map(|r| r.name.as_str())
        .collect();
    match (direct.len(), results.len()) {
        (0, _) => String::new(),
        (1, 1) => crate::i18n::tr!(
            "No supports needed: {} prints as it is. Every layer rests on the one below it.",
            direct[0]
        )
        .to_string(),
        (n, total) if n == total => crate::i18n::tr!(
            "No supports needed: all {} parts print as they are. Every layer rests on the one below it.",
            n
        )
        .to_string(),
        (_, _) => crate::i18n::tr!(
            "No supports needed for {}; the other parts got supports.",
            direct.join(", ")
        )
        .to_string(),
    }
}

/// Name and world size (mm) of every model in `scene`, generated supports
/// left out.
fn model_sizes(scene: &Scene) -> Vec<(String, [f64; 3])> {
    scene
        .reachable_leaves(
            sc_scene::CsgPolicy::Descend,
            sc_scene::ReferencePolicy::Skip,
        )
        .unwrap_or_default()
        .into_iter()
        .filter_map(|id| {
            let node = scene.node(id)?;
            let meta = node.meta.as_ref();
            if meta
                .and_then(|m| m.comment.as_deref())
                .is_some_and(|s| s.starts_with("sc-generated-support"))
            {
                return None;
            }
            let bounds = sc_scene::bake_leaf_to_world(scene, id).ok()?.local_aabb()?;
            let size = std::array::from_fn(|axis| {
                f64::from(bounds.max()[axis] - bounds.min()[axis])
            });
            let name = meta
                .and_then(|m| m.name.clone())
                .unwrap_or_else(|| format!("Model {}", id.get()));
            Some((name, size))
        })
        .collect()
}

/// The message for a model too big for the printer whichever way it is
/// turned, or `None` when every model could fit. Supports for such a model
/// stand in mid-air off the plate: a 515 mm model on a 224 mm plate took
/// 21 seconds to get 1,908 of them (R86). A model that only fits turned
/// another way (a flat frame wider than the plate is deep) is not refused:
/// Auto prepare stands it up.
/// Whether part `id` fits the printer some way round (see [`too_big_text`]).
pub(crate) fn part_fits_some_way(
    scene: &Scene,
    id: sc_scene::NodeId,
    volume: sc_print_primitives::BuildVolume,
) -> bool {
    let Some(bounds) = sc_scene::bake_leaf_to_world(scene, id)
        .ok()
        .and_then(|mesh| mesh.local_aabb())
    else {
        return true;
    };
    let size = std::array::from_fn(|axis| f64::from(bounds.max()[axis] - bounds.min()[axis]));
    too_big_text(
        &[(String::new(), size)],
        [volume.x_mm(), volume.y_mm(), volume.z_mm()],
    )
    .is_none()
}

pub(crate) fn too_big_text(models: &[(String, [f64; 3])], volume: [f64; 3]) -> Option<String> {
    let sorted = |mut v: [f64; 3]| {
        v.sort_by(|a, b| b.total_cmp(a));
        v
    };
    let room = sorted(volume);
    // Longest side against the longest diagonal, and so on down: generous,
    // so only a model with no way to fit is refused.
    let limits = [
        (room[0] * room[0] + room[1] * room[1] + room[2] * room[2]).sqrt(),
        room[0].hypot(room[1]),
        room[0],
    ];
    let (name, size) = models.iter().find(|(_, size)| {
        sorted(*size)
            .iter()
            .zip(limits)
            .any(|(side, limit)| *side > limit + 1e-6)
    })?;
    Some(
        crate::i18n::tr!(
            "{} is too big for this printer: {} x {} x {} mm on a {} x {} x {} mm build volume. Scale it down, split it, or pick a bigger printer.",
            name,
            size[0].round(),
            size[1].round(),
            size[2].round(),
            volume[0].round(),
            volume[1].round(),
            volume[2].round()
        )
        .to_string(),
    )
}

/// Show `text` in the support notice banner for a while (empty hides it).
fn show_support_notice(ui: &MainWindow, text: String) {
    ui.set_support_notice_text(text.as_str().into());
    if text.is_empty() {
        return;
    }
    let weak = ui.as_weak();
    slint::Timer::single_shot(Duration::from_secs(10), move || {
        if let Some(ui) = weak.upgrade()
            && ui.get_support_notice_text() == text.as_str()
        {
            ui.set_support_notice_text("".into());
        }
    });
}

pub(crate) fn support_result_text(
    results: &[sc_print_glue::supports::ModelSupportResult],
) -> String {
    // More than two parts: one line for the plate (R109: nine Benchys read
    // "3DBenchy: 4 supports; 3DBenchy: 4 supports; ..." past the window).
    if results.len() > 2 {
        let supports: usize = results.iter().map(|r| r.columns + r.small).sum();
        let direct = results.iter().filter(|r| r.direct).count();
        let text = crate::i18n::tr!("{} parts: {} supports", results.len(), supports).to_string();
        return if direct > 0 {
            crate::i18n::tr!("{} ({} print on the plate as they are)", text, direct).to_string()
        } else {
            text
        };
    }
    results
        .iter()
        .map(|r| {
            if r.direct {
                crate::i18n::tr!(
                    "{}: prints directly on the plate, no supports needed",
                    r.name.as_str()
                )
                .to_string()
            } else if r.small > 0 {
                // One word for one thing (R95): supports are the pieces on the
                // plate, as in the panel and the object list.
                crate::i18n::tr!(
                    "{}: {} supports ({} short)",
                    r.name.as_str(),
                    r.columns + r.small,
                    r.small
                )
                .to_string()
            } else {
                crate::i18n::tr!("{}: 1 support", "{}: {n} supports", r.columns, r.name.as_str())
                    .to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Each supported model's lift, read from its supports' `lift_mm` (models
/// kept on the plate record 0); `fallback` for legacy comments without one.
fn owner_lifts(scene: &Scene, fallback: f64) -> std::collections::BTreeMap<u32, f64> {
    let mut lifts = std::collections::BTreeMap::new();
    for id in support_ids(scene) {
        let Some(comment) = scene
            .node(id)
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.comment.as_deref())
        else {
            continue;
        };
        let field = |key: &str| {
            comment
                .lines()
                .find_map(|line| line.strip_prefix(key).map(str::trim))
        };
        let Some(owner) = field("owner = ").and_then(|v| v.parse::<u32>().ok()) else {
            continue;
        };
        let lift = field("lift_mm = ")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(fallback);
        lifts.entry(owner).or_insert(lift);
    }
    lifts
}

fn unlifted_model(scene: &Scene, lift_mm: f64) -> Scene {
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
    let lifts = owner_lifts(scene, lift_mm);
    let supported_owners: std::collections::BTreeSet<_> = lifts.keys().copied().collect();
    let mut model = scene.clone();
    let unlift = Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, -lift_mm]));
    for id in support_ids(scene).into_iter().rev() {
        let _ = model.remove_leaf(id);
    }
    for id in model.roots().to_vec() {
        let node = model.node(id).unwrap();
        if node
            .meta
            .as_ref()
            .and_then(|m| m.comment.as_deref())
            .is_some_and(|s| s.starts_with("sc-generated-support"))
        {
            let _ = model.remove_leaf(id);
        } else if supported_owners.is_empty() {
            // Legacy support comments had no owner. Preserve direct roots while
            // retaining the old grouped-support regeneration behaviour.
            let direct = sc_scene::bake_leaf_to_world(&model, id)
                .is_ok_and(|mesh| sc_print_glue::orient::prints_without_supports(&mesh, 45.0));
            if !direct {
                let _ = model.set_local_transform(id, unlift.compose(&node.local_transform));
            }
        }
    }
    if !supported_owners.is_empty() {
        for id in model
            .reachable_leaves(
                sc_scene::CsgPolicy::Descend,
                sc_scene::ReferencePolicy::Skip,
            )
            .unwrap_or_default()
        {
            if !supported_owners.contains(&id.get()) {
                continue;
            }
            let lift = lifts.get(&id.get()).copied().unwrap_or(lift_mm);
            if lift == 0.0 {
                continue;
            }
            let unlift_owner =
                Transform3D::Affine(AffineTransform3D::from_translation([0.0, 0.0, -lift]));
            let current = model.node(id).unwrap().local_transform;
            if let Ok(world) = model.world_transform_of(id)
                && let Ok(inverse) = current.inverse()
                && let Ok(parent_inverse) = world.compose(&inverse).inverse()
            {
                let _ = model.set_local_transform(
                    id,
                    parent_inverse.compose(&unlift_owner).compose(&world),
                );
            }
        }
    }
    model
}

pub(crate) fn support_ids(scene: &Scene) -> Vec<sc_scene::NodeId> {
    let mut ids = Vec::new();
    let _ = scene.for_each_expanded_leaf::<sc_scene::SceneError, _>(
        sc_scene::CsgPolicy::Descend,
        |id, _, _| {
            if scene
                .node(id)
                .and_then(|n| n.meta.as_ref())
                .and_then(|m| m.comment.as_deref())
                .is_some_and(|s| s.starts_with("sc-generated-support"))
            {
                ids.push(id);
            }
            Ok(())
        },
    );
    ids
}

/// Build a manual contact on a scene copy; failures leave the source intact.
fn manual_contact_scene(
    source: &Scene,
    mut position: [f32; 3],
    settings: &SupportSettings,
    lift_model: bool,
) -> Result<Scene, String> {
    use sc_mesh_core::transform::{AffineTransform3D, Transform3D};
    use sc_scene::{LeafPayload, node::SceneMetadata};
    let mut scene = source.clone();
    let mut model = repaired_support_source(source);
    let ids = support_ids(source);
    for id in ids.iter().rev() {
        model.remove_leaf(*id).map_err(|e| e.to_string())?;
    }
    if lift_model {
        let lift = Transform3D::Affine(AffineTransform3D::from_translation([
            0.0,
            0.0,
            settings.lift_mm,
        ]));
        for id in model.roots().to_vec() {
            let transform = model.node(id).unwrap().local_transform;
            let transform = lift.compose(&transform);
            scene
                .set_local_transform(id, transform)
                .map_err(|e| e.to_string())?;
            model
                .set_local_transform(id, transform)
                .map_err(|e| e.to_string())?;
        }
        position[2] += settings.lift_mm as f32;
    }
    let mesh = model
        .merge_meshes()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| crate::i18n::tr!("The scene has no geometry.").to_string())?;
    let mut structure = sc_print_glue::supports::build_supports(
        &mesh,
        &[sc_print_glue::supports::SupportPoint {
            position,
            normal: [0.0, 0.0, -1.0],
        }],
        settings,
    )
    .map_err(|e| e.to_string())?;
    if structure.points.is_empty() {
        return Err(crate::i18n::tr!("The support is blocked by model geometry.").to_string());
    }
    let owner = sc_print_glue::supports::support_owners(&model, &structure.points)
        .first()
        .copied()
        .flatten()
        .map_or(0, |id| id.get());
    // Rebuild one raft for this owner around all of its existing contacts.
    let owned: Vec<_> = support_ids(&scene)
        .into_iter()
        .filter(|&id| {
            crate::scene_document::support_owner(&scene, id).is_some_and(|id| id.get() == owner)
        })
        .collect();
    let mut contacts = structure.points.clone();
    for id in owned {
        let Some(comment) = scene
            .node(id)
            .and_then(|n| n.meta.as_ref())
            .and_then(|m| m.comment.as_deref())
        else {
            continue;
        };
        if comment.starts_with("sc-generated-support-raft") {
            scene.remove_leaf(id).map_err(|e| e.to_string())?;
        } else if let Some(point) = comment
            .split_once('\n')
            .and_then(|(_, text)| toml::from_str::<toml::Value>(text).ok())
            .and_then(|v| {
                let p = v.get("position")?.as_array()?;
                if p.len() != 3 {
                    return None;
                }
                let mut position = [0.0; 3];
                for i in 0..3 {
                    position[i] = p[i]
                        .as_float()
                        .or_else(|| p[i].as_integer().map(|v| v as f64))?
                        as f32;
                }
                let transform = scene.world_transform_of(id).ok()?;
                let position = match transform {
                    Transform3D::Identity => position,
                    Transform3D::Affine(a) => a
                        .apply_to_vertex(sc_mesh_core::Vertex::new(position))
                        .coords(),
                };
                Some(sc_print_glue::supports::SupportPoint {
                    position,
                    normal: [0.0, 0.0, -1.0],
                })
            })
        {
            contacts.push(point);
        }
    }
    if contacts.len() > 1 {
        structure.raft = sc_print_glue::supports::build_supports(&mesh, &contacts, settings)
            .map_err(|e| e.to_string())?
            .raft;
    }
    for (index, mesh) in structure
        .columns
        .into_iter()
        .chain(structure.raft)
        .enumerate()
    {
        let comment = if index == 0 {
            format!(
                "sc-generated-support\nposition = {:?}\nnormal = [0.0, 0.0, -1.0]\nlift_mm = {}\nowner = {}\n",
                position, settings.lift_mm, owner
            )
        } else {
            format!("sc-generated-support-raft\nowner = {owner}\n")
        };
        let leaf = scene
            .add_leaf(
                Some(SceneMetadata {
                    name: Some(format!("Support {}", source.nodes_len() + index + 1)),
                    comment: Some(comment),
                }),
                Transform3D::Identity,
                LeafPayload::Mesh(Arc::new(mesh)),
            )
            .map_err(|e| e.to_string())?;
        scene.add_root(leaf).map_err(|e| e.to_string())?;
    }
    Ok(scene)
}

/// User-facing text for a failed support placement. Shown in the Supports
/// panel, so it must read as a message, not as Rust debug output (it used to
/// print the raw `EditNotApplied` value).
fn apply_error_text(error: &crate::scene_document::EditRefused) -> slint::SharedString {
    match error {
        crate::scene_document::EditRefused::Empty => {
            crate::i18n::tr!("Open a model first.")
        }
        crate::scene_document::EditRefused::Refused(err) => {
            crate::i18n::tr!("Could not apply the supports: {}", err)
        }
    }
}

/// Create one undoable dab without changing geometry or stable ownership IDs.
fn painted_scene_edit(
    scene: &Scene,
    id: sc_scene::NodeId,
    position: [f32; 3],
    radius: f32,
    avoid: bool,
) -> Option<sc_scene_edit::SceneEdit> {
    use sc_scene_edit::SceneEdit;
    if !radius.is_finite() || !(0.1..=100.0).contains(&radius) {
        return None;
    }
    if support_ids(scene).contains(&id) {
        return None;
    }
    let node = scene.node(id)?;
    let sc_scene::NodeKind::Leaf(payload) = &node.kind else {
        return None;
    };
    let transform = scene.world_transform_of(id).ok()?;
    let inverse = transform.inverse().ok()?;
    let p = inverse
        .apply_to_vertex(sc_mesh_core::Vertex::new(position))
        .coords();
    // Store a local radius so uniformly scaled models keep the
    // requested world-space brush size when painted.
    let edge = inverse
        .apply_to_vertex(sc_mesh_core::Vertex::new([
            position[0] + radius,
            position[1],
            position[2],
        ]))
        .coords();
    let radius = p
        .iter()
        .zip(edge)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f32>()
        .sqrt();
    let mut meta = node.meta.clone().unwrap_or_default();
    let comment = meta.comment.get_or_insert_with(String::new);
    let regions = sc_print_glue::supports::painted_regions(comment);
    if regions.len() >= 256 {
        return None;
    }
    if regions.last().is_some_and(|r| {
        r.keep != avoid
            && r.centre
                .iter()
                .zip(p)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f32>()
                < (radius * 0.25).powi(2)
    }) {
        return None;
    }
    comment.push_str(&format!(
        "\nsc-support-paint {} {} {} {} {}\n",
        p[0],
        p[1],
        p[2],
        radius,
        if avoid { 0 } else { 1 }
    ));
    let attachment = scene.attachment_of(id)?;
    Some(SceneEdit::Compound(vec![
        SceneEdit::RemoveLeaf(sc_scene_edit::LeafRecord {
            node_id: id,
            attachment,
            payload: payload.clone(),
            local_transform: node.local_transform,
            meta: node.meta.clone(),
        }),
        SceneEdit::AddLeaf(sc_scene_edit::LeafRecord {
            node_id: id,
            attachment,
            payload: payload.clone(),
            local_transform: node.local_transform,
            meta: Some(meta),
        }),
    ]))
}

#[cfg(test)]
#[path = "tests_auto_support_run.rs"]
mod tests;

fn repaired_support_source(source: &Scene) -> Scene {
    let mut scene = source.clone();
    scene.repair_meshes(sc_mesh_core::indexed_mesh::RepairPasses::default_repair());
    scene
}

/// Whether the plate's generated supports need regenerating: a model they
/// hold moved or changed, or the printer changed since they were made (R108:
/// supports made for an M3 Premium left an island at layer 463 of a Benchy
/// sliced for an M7 Pro, whose pixels are finer, and nothing said so).
pub(crate) fn supports_out_of_date(document: &SceneDocument) -> bool {
    let (any, geometry) = document
        .read_scene(|s| {
            let ids = support_ids(s.graph());
            (
                !ids.is_empty(),
                ids.into_iter()
                    .any(|id| crate::scene_document::support_is_stale(s.graph(), id)),
            )
        })
        .unwrap_or((false, false));
    geometry
        || printer_changed(
            any,
            document.supports_printer.borrow().as_ref(),
            document.chosen_printer().as_ref(),
        )
}

/// Supports exist, were made for a known printer, and another is selected.
pub(crate) fn printer_changed<P: PartialEq>(supports: bool, made_for: Option<&P>, now: Option<&P>) -> bool {
    supports && made_for.is_some() && made_for != now
}

#[cfg(test)]
mod supports_printer_tests {
    /// R108: supports made for one printer are out of date on another; with
    /// no supports, or not knowing what they were made for, nothing is said.
    #[test]
    fn supports_are_out_of_date_after_a_printer_change() {
        let (m3, m7) = ("m3-premium", "m7-pro");
        assert!(super::printer_changed(true, Some(&m3), Some(&m7)));
        assert!(!super::printer_changed(true, Some(&m3), Some(&m3)));
        assert!(!super::printer_changed(false, Some(&m3), Some(&m7)));
        assert!(!super::printer_changed(true, None, Some(&m7)));
        let run = include_str!("auto_support_run.rs");
        assert!(run.contains(".supports_printer\n                            .replace(run.document.chosen_printer())"));
    }
}
