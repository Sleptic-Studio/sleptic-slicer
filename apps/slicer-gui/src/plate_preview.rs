// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0

//! Presentation cached alongside each plate slice.
#[derive(Clone, Default)]
pub(crate) struct PlatePreview {
    estimated_weight_text: slint::SharedString,
    exposure_summary: slint::SharedString,
    estimated_print_time_text: slint::SharedString,
    estimated_resin_volume_text: slint::SharedString,
    estimated_resin_cost_text: slint::SharedString,
    slice_recovery_message: slint::SharedString,
    print_risks: slint::ModelRc<crate::RiskRow>,
    risk_error_count: i32,
    layer_areas: slint::ModelRc<f32>,
    island_layers: slint::ModelRc<i32>,
    selected_layer_index: f32,
    slice_layer_count: i32,
}
impl PlatePreview {
    pub(crate) fn capture(ui: &crate::MainWindow) -> Self {
        Self {
            estimated_weight_text: ui.get_estimated_weight_text(),
            exposure_summary: ui.get_exposure_summary(),
            estimated_print_time_text: ui.get_estimated_print_time_text(),
            estimated_resin_volume_text: ui.get_estimated_resin_volume_text(),
            estimated_resin_cost_text: ui.get_estimated_resin_cost_text(),
            slice_recovery_message: ui.get_slice_recovery_message(),
            print_risks: ui.get_print_risks(),
            risk_error_count: ui.get_risk_error_count(),
            layer_areas: ui.get_layer_areas(),
            island_layers: ui.get_island_layers(),
            selected_layer_index: ui.get_layer_shown(),
            slice_layer_count: ui.get_layer_total(),
        }
    }
    pub(crate) fn apply(&self, ui: &crate::MainWindow) {
        ui.set_estimated_weight_text(self.estimated_weight_text.clone());
        ui.set_exposure_summary(self.exposure_summary.clone());
        ui.set_estimated_print_time_text(self.estimated_print_time_text.clone());
        ui.set_estimated_resin_volume_text(self.estimated_resin_volume_text.clone());
        ui.set_estimated_resin_cost_text(self.estimated_resin_cost_text.clone());
        ui.set_slice_recovery_message(self.slice_recovery_message.clone());
        ui.set_print_risks(self.print_risks.clone());
        ui.set_risk_error_count(self.risk_error_count);
        ui.set_layer_areas(self.layer_areas.clone());
        ui.set_island_layers(self.island_layers.clone());
        ui.set_layer_shown(self.selected_layer_index);
        ui.set_layer_total(self.slice_layer_count);
        ui.set_selected_risk(-1);
    }
}
