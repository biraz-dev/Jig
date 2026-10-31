//! Local STL instrument. Meshes and camera state stay in memory; only named
//! material settings are returned to the ordinary portable application state.
use eframe::{
    egui::{self, Color32, FontId, RichText, Stroke, Ui},
    egui_glow, glow,
};
use glow::HasContext as _;
use jig::{
    clay::{Basis, Settings},
    stl::{self, LengthUnit, Measurements, Mesh},
};
use std::{f32::consts::FRAC_PI_2, sync::Arc, time::Duration};
use wasm_bindgen::prelude::*;

const BG: Color32 = Color32::from_rgb(12, 19, 27);
const CREAM: Color32 = Color32::from_rgb(246, 218, 174);
const PINK: Color32 = Color32::from_rgb(234, 133, 173);
const MUTED: Color32 = Color32::from_rgb(144, 161, 202);
const RULE: Color32 = Color32::from_rgb(49, 65, 83);

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = jigStlOpen)]
    fn open_file();
    #[wasm_bindgen(js_name = jigStlTakeBytes)]
    fn take_bytes() -> Vec<u8>;
    #[wasm_bindgen(js_name = jigStlTakeName)]
    fn take_name() -> String;
    #[wasm_bindgen(js_name = jigStlTakeError)]
    fn take_error() -> String;
    #[wasm_bindgen(js_name = jigStlClear)]
    fn clear_file();
    #[wasm_bindgen(js_name = jigStlExport)]
    fn export_file(bytes: &[u8], name: &str);
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ShrinkMode {
    Off,
    Estimate,
    Compensate,
}
impl ShrinkMode {
    fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Estimate => "Estimate",
            Self::Compensate => "Compensate",
        }
    }
    fn measurement_label(self) -> &'static str {
        match self {
            Self::Off => "Model",
            Self::Estimate => "Shrunken estimate",
            Self::Compensate => "Compensated model",
        }
    }
}

/// Session-only scene state. Arc shares unchanged geometry across camera/control
/// edits; material preferences belong to the ordinary application-state journal.
#[derive(Clone, Debug)]
pub struct Snapshot {
    mesh: Option<Arc<Mesh>>,
    name: String,
    unit: LengthUnit,
    scale_percent: f64,
    mode: ShrinkMode,
    yaw: f32,
    pitch: f32,
    zoom: f32,
}

impl Snapshot {
    /// Identity and allocated geometry bytes, for the dashboard's shared budget.
    pub fn mesh_identity(&self) -> Option<(usize, usize)> {
        self.mesh.as_ref().map(mesh_identity)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Gesture {
    Orbit,
    Scale,
    Wheel,
}

struct PendingGesture {
    before: Snapshot,
    kind: Gesture,
    last_input: f64,
}

pub struct StlPanel {
    gl: Option<Arc<glow::Context>>,
    renderer: Option<Arc<egui::mutex::Mutex<Renderer>>>,
    mesh: Option<Arc<Mesh>>,
    name: String,
    error: Option<String>,
    graphics_error: Option<String>,
    unit: LengthUnit,
    scale_percent: f64,
    mode: ShrinkMode,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    material_open: bool,
    custom_open: bool,
    custom_name: String,
    custom_cone: String,
    custom_shrinkage: f64,
    custom_basis: Basis,
    undo_records: Vec<Snapshot>,
    pending_gesture: Option<PendingGesture>,
    focus_open: bool,
    /// Lets the dashboard retain its ordinary selected-applet keyboard behavior.
    pub focused: bool,
}

impl StlPanel {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            gl: cc.gl.clone(),
            renderer: None,
            mesh: None,
            name: String::new(),
            error: None,
            graphics_error: None,
            unit: LengthUnit::Millimeters,
            scale_percent: 100.0,
            mode: ShrinkMode::Off,
            yaw: 0.65,
            pitch: 0.55,
            zoom: 1.0,
            material_open: false,
            custom_open: false,
            custom_name: String::new(),
            custom_cone: "06".into(),
            custom_shrinkage: 6.0,
            custom_basis: Basis::WetToFired,
            undo_records: Vec::new(),
            pending_gesture: None,
            focus_open: false,
            focused: false,
        }
    }

    /// Poll the local file bridge. Failed replacement imports preserve the
    /// current mesh; a cancelled file chooser has no effect.
    pub fn update(&mut self) -> bool {
        let error = take_error();
        if !error.is_empty() {
            self.error = Some(error);
            return true;
        }
        let bytes = take_bytes();
        if bytes.is_empty() {
            return false;
        }
        let name = take_name();
        match stl::parse(&bytes) {
            Ok(mesh) => self.load(mesh, name),
            Err(error) => self.error = Some(error),
        }
        true
    }

    fn load(&mut self, mesh: Mesh, name: String) {
        let before = self.prepare_snapshot();
        self.error = None;
        self.graphics_error = None;
        self.name = name;
        self.scale_percent = 100.0;
        self.unit = LengthUnit::Millimeters;
        self.mode = ShrinkMode::Off;
        self.yaw = 0.65;
        self.pitch = 0.55;
        self.zoom = 1.0;
        self.mesh = Some(Arc::new(mesh));
        self.upload_current();
        self.push_snapshot(before);
    }

    fn upload_current(&mut self) {
        self.graphics_error = None;
        let Some(mesh) = self.mesh.as_ref().cloned() else {
            self.destroy();
            return;
        };
        if let Some(gl) = &self.gl {
            if self.renderer.is_none() {
                match Renderer::new(gl) {
                    Ok(renderer) => {
                        self.renderer = Some(Arc::new(egui::mutex::Mutex::new(renderer)));
                    }
                    Err(error) => self.graphics_error = Some(error),
                }
            }
            if let Some(renderer) = &self.renderer {
                if let Err(error) = renderer.lock().upload(gl, &mesh) {
                    self.graphics_error = Some(error);
                }
            }
        } else {
            self.graphics_error = Some("3D rendering needs a WebGL context.".into());
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            mesh: self.mesh.clone(),
            name: self.name.clone(),
            unit: self.unit,
            scale_percent: self.scale_percent,
            mode: self.mode,
            yaw: self.yaw,
            pitch: self.pitch,
            zoom: self.zoom,
        }
    }

    fn differs(&self, before: &Snapshot) -> bool {
        !same_mesh(&self.mesh, &before.mesh)
            || self.name != before.name
            || self.unit != before.unit
            || self.scale_percent.to_bits() != before.scale_percent.to_bits()
            || self.mode != before.mode
            || self.yaw.to_bits() != before.yaw.to_bits()
            || self.pitch.to_bits() != before.pitch.to_bits()
            || self.zoom.to_bits() != before.zoom.to_bits()
    }

    fn prepare_snapshot(&mut self) -> Snapshot {
        self.finish_gesture();
        self.snapshot()
    }

    fn push_snapshot(&mut self, before: Snapshot) {
        if self.differs(&before) {
            self.undo_records.push(before);
        }
    }

    fn begin_gesture(&mut self, kind: Gesture, before: Snapshot, now: f64) {
        if self
            .pending_gesture
            .as_ref()
            .is_some_and(|pending| pending.kind != kind)
        {
            self.finish_gesture();
        }
        if let Some(pending) = &mut self.pending_gesture {
            pending.last_input = now;
        } else {
            self.pending_gesture = Some(PendingGesture {
                before,
                kind,
                last_input: now,
            });
        }
    }

    fn finish_gesture(&mut self) {
        if let Some(pending) = self.pending_gesture.take() {
            self.push_snapshot(pending.before);
        }
    }

    /// Drain completed actions into the dashboard's single chronological journal.
    pub fn take_undo_records(&mut self) -> Vec<Snapshot> {
        std::mem::take(&mut self.undo_records)
    }

    pub fn has_pending_undo(&self) -> bool {
        !self.undo_records.is_empty()
            || self
                .pending_gesture
                .as_ref()
                .is_some_and(|pending| self.differs(&pending.before))
    }

    /// Give the applet shortcut a predictable keyboard entry point, including
    /// after maximizing changes the control's layout identity.
    pub fn request_open_focus(&mut self) {
        self.focus_open = true;
    }

    /// Finish a drag/edit before the dashboard records another applet's action
    /// or handles Undo. Restoring a snapshot itself never emits an action.
    pub fn finish_undo_gesture(&mut self) {
        self.finish_gesture();
    }

    pub fn mesh_identity(&self) -> Option<(usize, usize)> {
        self.mesh.as_ref().map(mesh_identity)
    }

    /// Restore one instrument action. Drain records first; any pending gesture
    /// or undelivered records are cancelled rather than recording Undo itself.
    pub fn restore(&mut self, before: Snapshot) {
        self.clear_undo();
        clear_file();
        let mesh_changed = !same_mesh(&self.mesh, &before.mesh);
        self.mesh = before.mesh;
        self.name = before.name;
        self.unit = before.unit;
        self.scale_percent = before.scale_percent;
        self.mode = before.mode;
        self.yaw = before.yaw;
        self.pitch = before.pitch;
        self.zoom = before.zoom;
        self.error = None;
        if mesh_changed {
            self.upload_current();
        }
    }

    /// Dataset replacement must not bring profiles from another dataset back.
    pub fn clear_undo(&mut self) {
        self.pending_gesture = None;
        self.undo_records.clear();
    }

    fn effective_scale(&self, settings: &Settings) -> f64 {
        let shrink = settings
            .selected_profile()
            .and_then(|profile| stl::shrinkage(profile.linear_shrinkage).ok());
        let factor = match (self.mode, shrink) {
            (ShrinkMode::Estimate, Some(shrink)) => shrink.remaining,
            (ShrinkMode::Compensate, Some(shrink)) => shrink.compensation,
            _ => 1.0,
        };
        self.scale_percent / 100.0 * factor
    }

    fn measurements(&self, settings: &Settings) -> Option<Result<Measurements, String>> {
        self.mesh
            .as_ref()
            .map(|mesh| mesh.measure(self.unit, self.effective_scale(settings)))
    }

    /// Returns true only when portable material preferences change.
    pub fn ui(&mut self, ui: &mut Ui, height: f32, settings: &mut Settings) -> bool {
        let top = ui.cursor().top();
        let now = ui.input(|input| input.time);
        if self.pending_gesture.as_ref().is_some_and(|pending| {
            pending.kind == Gesture::Wheel && now - pending.last_input >= 0.25
        }) {
            self.finish_gesture();
        }
        let mut changed = false;
        self.focused = false;
        if let Some(renderer) = &self.renderer {
            if let Some(error) = &renderer.lock().error {
                self.graphics_error = Some(error.clone());
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("STL").color(PINK));
            if self.focus_open {
                let id = ui.next_auto_id();
                ui.memory_mut(|memory| memory.request_focus(id));
                self.focus_open = false;
                self.focused = true;
                ui.ctx().request_repaint();
            }
            let open = ui.button("Open STL");
            self.focused |= open.has_focus();
            if open.clicked() {
                open_file();
            }
            let sample = ui.button("Sample cube");
            self.focused |= sample.has_focus();
            if sample.clicked() {
                clear_file();
                match sample_cube() {
                    Ok(mesh) => self.load(mesh, "Sample cube (10 mm).stl".into()),
                    Err(error) => self.error = Some(error),
                }
            }
            let clear = ui.add_enabled(self.mesh.is_some(), egui::Button::new("Clear"));
            self.focused |= clear.has_focus();
            if clear.clicked() {
                let before = self.prepare_snapshot();
                clear_file();
                self.destroy();
                self.mesh = None;
                self.name.clear();
                self.error = None;
                self.graphics_error = None;
                self.push_snapshot(before);
            }
            let copy = ui.add_enabled(self.mesh.is_some(), egui::Button::new("Copy report"));
            self.focused |= copy.has_focus();
            if copy.clicked() {
                ui.ctx().copy_text(self.report(settings));
            }
            let export = ui.add_enabled(self.mesh.is_some(), egui::Button::new("Export STL"));
            self.focused |= export.has_focus();
            if export.clicked() {
                if let Some(mesh) = &self.mesh {
                    match mesh.to_binary_stl(self.effective_scale(settings)) {
                        Ok(bytes) => {
                            let stem = self.name.strip_suffix(".stl").unwrap_or(&self.name);
                            export_file(&bytes, &format!("{stem}-jig.stl"));
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            }
        });
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).color(PINK).size(12.0));
        }
        if self.mesh.is_none() {
            ui.add_space(6.0);
            ui.label(RichText::new("Choose a local STL or inspect the sample cube.").color(MUTED));
            ui.label(
                RichText::new("ASCII / binary · up to 20 MiB · files stay on this device")
                    .color(MUTED)
                    .size(11.0),
            );
            return changed;
        }
        ui.label(
            RichText::new(format!(
                "{} · {} triangles",
                self.name,
                self.mesh.as_ref().map_or(0, |mesh| mesh.triangles.len())
            ))
            .color(MUTED)
            .size(11.0),
        );
        ui.horizontal_wrapped(|ui| {
            for (label, yaw, pitch) in [
                ("Iso", 0.65, 0.55),
                ("Front", 0.0, 0.0),
                ("Top", 0.0, FRAC_PI_2),
                ("Side", FRAC_PI_2, 0.0),
            ] {
                let response = ui.button(label);
                self.focused |= response.has_focus();
                if response.clicked() {
                    let before = self.prepare_snapshot();
                    self.yaw = yaw;
                    self.pitch = pitch;
                    self.push_snapshot(before);
                }
            }
            let fit = ui.button("Fit");
            self.focused |= fit.has_focus();
            if fit.clicked() {
                let before = self.prepare_snapshot();
                self.zoom = (1.0 / self.effective_scale(settings)) as f32;
                self.push_snapshot(before);
            }
            for (label, factor) in [("−", 0.8), ("+", 1.25)] {
                let response = ui.button(label);
                self.focused |= response.has_focus();
                if response.clicked() {
                    let before = self.prepare_snapshot();
                    self.zoom = (self.zoom * factor).clamp(0.0001, 10000.0);
                    self.push_snapshot(before);
                }
            }
        });
        let width = ui.available_width();
        let header_height = ui.cursor().top() - top;
        let graphics_reserve = if self.graphics_error.is_some() {
            24.0
        } else {
            0.0
        };
        let content_height = (height - header_height - graphics_reserve - 8.0).max(170.0);
        if width >= 620.0 {
            let viewer_width = (width * 0.58).max(310.0);
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(viewer_width, content_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| self.viewport(ui, content_height, settings),
                );
                ui.allocate_ui_with_layout(
                    egui::vec2((width - viewer_width - 8.0).max(180.0), content_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("stl-settings")
                            .max_height(content_height)
                            .show(ui, |ui| {
                                self.statistics(ui, settings);
                                changed |= self.controls(ui, settings);
                            });
                    },
                );
            });
        } else {
            // Volume remains above the model on narrow displays.
            self.statistics(ui, settings);
            self.viewport(ui, (height * 0.40).clamp(160.0, 260.0), settings);
            changed |= self.controls(ui, settings);
        }
        if let Some(error) = &self.graphics_error {
            ui.label(RichText::new(error).color(PINK).size(11.0));
        }
        changed
    }

    fn statistics(&self, ui: &mut Ui, settings: &Settings) {
        let Some(measurements) = self.measurements(settings) else {
            return;
        };
        match measurements {
            Ok(measurements) => {
                ui.label(
                    RichText::new(self.mode.measurement_label())
                        .color(MUTED)
                        .size(11.0),
                );
                if let Some(volume) = measurements.volume_mm3 {
                    ui.label(
                        RichText::new(format!("{:.2} cm³", volume / 1000.0))
                            .color(CREAM)
                            .size(24.0),
                    );
                    ui.label(
                        RichText::new(format!(
                            "Total volume · {:.2} mL · {:.2} in³",
                            volume / 1000.0,
                            volume / 25.4_f64.powi(3)
                        ))
                        .color(MUTED)
                        .size(11.0),
                    );
                    ui.label(
                        RichText::new("Closed mesh estimate · intersections not checked")
                            .color(MUTED)
                            .size(11.0),
                    );
                } else {
                    ui.label(
                        RichText::new("Total volume unavailable")
                            .color(PINK)
                            .size(14.0),
                    );
                }
                ui.label(
                    RichText::new(format!(
                        "Area {:.2} cm²",
                        measurements.surface_area_mm2 / 100.0
                    ))
                    .color(CREAM)
                    .size(12.0),
                );
                let [x, y, z] = measurements.dimensions_mm;
                ui.label(
                    RichText::new(format!("{x:.2} × {y:.2} × {z:.2} mm"))
                        .color(CREAM)
                        .size(12.0),
                );
                if self.mode != ShrinkMode::Off {
                    if let Some(Ok(base)) = self
                        .mesh
                        .as_ref()
                        .map(|mesh| mesh.measure(self.unit, self.scale_percent / 100.0))
                    {
                        if let Some(volume) = base.volume_mm3 {
                            ui.label(
                                RichText::new(format!("Base model {:.2} cm³", volume / 1000.0))
                                    .color(MUTED)
                                    .size(11.0),
                            );
                        }
                    }
                }
                if let Some(mesh) = &self.mesh {
                    if mesh.metrics.volume.is_none() {
                        ui.label(
                            RichText::new(format!(
                                "Open edges {} · non-manifold {} · winding {} · degenerate {}",
                                mesh.metrics.boundary_edges,
                                mesh.metrics.non_manifold_edges,
                                mesh.metrics.inconsistent_edges,
                                mesh.metrics.degenerate_triangles
                            ))
                            .color(PINK)
                            .size(11.0),
                        );
                        if mesh.metrics.mixed_component_winding {
                            ui.label(RichText::new("Mixed shell orientation / possible cavity; enclosed volume unverified").color(PINK).size(11.0));
                        }
                    }
                }
            }
            Err(error) => {
                ui.label(RichText::new(error).color(PINK).size(12.0));
            }
        }
        ui.add_space(5.0);
    }

    fn controls(&mut self, ui: &mut Ui, settings: &mut Settings) -> bool {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("STL units").color(MUTED).size(12.0));
            for unit in LengthUnit::ALL {
                let response = ui.selectable_label(self.unit == unit, unit.label());
                self.focused |= response.has_focus();
                if response.clicked() && self.unit != unit {
                    let before = self.prepare_snapshot();
                    self.unit = unit;
                    self.push_snapshot(before);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Scale");
            let before = self.snapshot();
            let response = ui
                .push_id("stl-scale", |ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.scale_percent)
                            .range(0.01..=10000.0)
                            .speed(1.0)
                            .suffix("%"),
                    )
                })
                .inner;
            self.focused |= response.has_focus();
            if response.changed() {
                let now = ui.input(|input| input.time);
                self.begin_gesture(Gesture::Scale, before, now);
            }
            let finish = response.drag_stopped()
                || response.lost_focus()
                || ui.input(|input| input.key_pressed(egui::Key::Enter));
            if finish
                && self
                    .pending_gesture
                    .as_ref()
                    .is_some_and(|pending| pending.kind == Gesture::Scale)
            {
                self.finish_gesture();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Shrink").color(MUTED).size(12.0));
            for mode in [
                ShrinkMode::Off,
                ShrinkMode::Estimate,
                ShrinkMode::Compensate,
            ] {
                let response = ui.selectable_label(self.mode == mode, mode.label());
                self.focused |= response.has_focus();
                if response.clicked() && self.mode != mode {
                    let before = self.prepare_snapshot();
                    self.mode = mode;
                    self.push_snapshot(before);
                }
            }
        });
        let selected = settings.selected_profile();
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            let button = ui.selectable_label(self.material_open, "Material");
            self.focused |= button.has_focus();
            if button.clicked() {
                self.material_open = !self.material_open;
            }
            if let Some(profile) = &selected {
                ui.label(
                    RichText::new(format!("{} · cone {}", profile.name, profile.cone))
                        .color(MUTED)
                        .size(11.0),
                );
            }
        });
        if let Some(profile) = &selected {
            ui.label(
                RichText::new(format!(
                    "{:.2}% linear · {}",
                    profile.linear_shrinkage,
                    profile.basis.label()
                ))
                .color(MUTED)
                .size(11.0),
            );
            if self.mode != ShrinkMode::Off {
                let formula = if self.mode == ShrinkMode::Estimate {
                    "Length × (1 − shrinkage)"
                } else {
                    "Length ÷ (1 − shrinkage)"
                };
                ui.label(RichText::new(formula).color(MUTED).size(11.0));
            }
            if let Some(source) = &profile.source {
                ui.hyperlink_to("Manufacturer data", source);
            }
        }
        if self.material_open {
            ui.indent("stl-material-choices", |ui| {
                for profile in settings.available_profiles() {
                    ui.horizontal_wrapped(|ui| {
                        let response = ui.selectable_label(
                            settings.selected == profile.id,
                            format!("{} · {:.2}%", profile.label(), profile.linear_shrinkage),
                        );
                        self.focused |= response.has_focus();
                        if response.clicked() && settings.selected != profile.id {
                            self.finish_gesture();
                            settings.selected = profile.id.clone();
                            changed = true;
                        }
                        if settings
                            .custom_profiles
                            .iter()
                            .any(|item| item.id == profile.id)
                            && ui.small_button("del").clicked()
                        {
                            self.finish_gesture();
                            match settings.remove_custom(&profile.id) {
                                Ok(()) => {
                                    changed = true;
                                }
                                Err(error) => self.error = Some(error),
                            }
                        }
                    });
                }
                let response = ui.selectable_label(self.custom_open, "Add test profile");
                self.focused |= response.has_focus();
                if response.clicked() {
                    self.custom_open = !self.custom_open;
                }
                if self.custom_open {
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.custom_name)
                            .hint_text("Clay body / test name")
                            .desired_width(ui.available_width()),
                    );
                    self.focused |= response.has_focus();
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Cone");
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.custom_cone).desired_width(50.0),
                        );
                        self.focused |= response.has_focus();
                        ui.label("Linear shrink");
                        let response = ui.add(
                            egui::DragValue::new(&mut self.custom_shrinkage)
                                .range(0.0..=99.99)
                                .speed(0.1)
                                .suffix("%"),
                        );
                        self.focused |= response.has_focus();
                    });
                    ui.horizontal_wrapped(|ui| {
                        for basis in Basis::ALL {
                            let response =
                                ui.selectable_label(self.custom_basis == basis, basis.label());
                            self.focused |= response.has_focus();
                            if response.clicked() {
                                self.custom_basis = basis;
                            }
                        }
                    });
                    let response = ui.button("Save test profile");
                    self.focused |= response.has_focus();
                    if response.clicked() {
                        self.finish_gesture();
                        match settings.add_custom(
                            &self.custom_name,
                            &self.custom_cone,
                            self.custom_shrinkage,
                            self.custom_basis,
                        ) {
                            Ok(_) => {
                                self.custom_open = false;
                                changed = true;
                                self.error = None;
                            }
                            Err(error) => self.error = Some(error),
                        }
                    }
                }
            });
        }
        changed
    }

    fn viewport(&mut self, ui: &mut Ui, height: f32, settings: &Settings) {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::click_and_drag(),
        );
        self.focused |= response.has_focus();
        let now = ui.input(|input| input.time);
        if response.dragged() {
            let motion = response.drag_motion();
            if motion != egui::Vec2::ZERO {
                let before = self.snapshot();
                self.begin_gesture(Gesture::Orbit, before, now);
                self.yaw += motion.x * 0.012;
                self.pitch = (self.pitch + motion.y * 0.012).clamp(-3.0, 3.0);
            }
        }
        if response.drag_stopped()
            && self
                .pending_gesture
                .as_ref()
                .is_some_and(|pending| pending.kind == Gesture::Orbit)
        {
            self.finish_gesture();
        }
        if response.hovered() {
            let scroll = ui.input_mut(|input| {
                let scroll = input.raw_scroll_delta.y;
                input.raw_scroll_delta.y = 0.0;
                input.smooth_scroll_delta.y = 0.0;
                scroll
            });
            let pinch = ui.input(|input| input.zoom_delta());
            let zoom = (self.zoom * (scroll * 0.002).exp() * pinch).clamp(0.0001, 10000.0);
            if zoom.to_bits() != self.zoom.to_bits() {
                let before = self.snapshot();
                self.begin_gesture(Gesture::Wheel, before, now);
                self.zoom = zoom;
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            }
        }
        response.on_hover_text("Drag to rotate · wheel or +/− to zoom · Fit reframes the model");
        ui.painter().rect_filled(rect, 0.0, BG);
        if self.graphics_error.is_none() {
            if let Some(renderer) = &self.renderer {
                let renderer = Arc::clone(renderer);
                let view = View {
                    yaw: self.yaw,
                    pitch: self.pitch,
                    scale: (self.zoom as f64 * self.effective_scale(settings)) as f32,
                };
                let context = ui.ctx().clone();
                let callback = egui_glow::CallbackFn::new(move |info, painter| {
                    let mut renderer = renderer.lock();
                    renderer.paint(&info, painter, view);
                    if renderer.error.is_some() {
                        context.request_repaint();
                    }
                });
                ui.painter().add(egui::PaintCallback {
                    rect,
                    callback: Arc::new(callback),
                });
            }
        }
        self.gnomon(ui, rect);
        if self.focused {
            ui.painter()
                .hline(rect.x_range(), rect.bottom(), Stroke::new(1.0_f32, MUTED));
        } else {
            ui.painter()
                .hline(rect.x_range(), rect.bottom(), Stroke::new(0.5_f32, RULE));
        }
    }

    fn gnomon(&self, ui: &Ui, rect: egui::Rect) {
        let center = egui::pos2(rect.left() + 43.0, rect.bottom() - 43.0);
        ui.painter()
            .circle_filled(center, 36.0, BG.gamma_multiply(0.92));
        let (right, up, depth) = camera_axes(self.yaw, self.pitch);
        let mut axes = [
            (0, "X", PINK),
            (1, "Y", Color32::from_rgb(148, 188, 160)),
            (2, "Z", Color32::from_rgb(109, 185, 238)),
        ];
        axes.sort_by(|a, b| depth[b.0].total_cmp(&depth[a.0]));
        for (axis, label, color) in axes {
            let direction = egui::vec2(right[axis], -up[axis]);
            if direction.length_sq() < 0.02 {
                ui.painter()
                    .circle_stroke(center, 4.0, Stroke::new(1.0_f32, color));
                ui.painter().text(
                    center + egui::vec2(-11.0, 10.0),
                    egui::Align2::CENTER_CENTER,
                    label,
                    FontId::monospace(11.0),
                    color,
                );
                continue;
            }
            let end = center + direction * 25.0;
            ui.painter()
                .line_segment([center, end], Stroke::new(1.5_f32, color));
            ui.painter().text(
                center + direction * 31.0,
                egui::Align2::CENTER_CENTER,
                label,
                FontId::monospace(11.0),
                color,
            );
        }
        ui.painter().circle_filled(center, 2.0, CREAM);
    }

    fn report(&self, settings: &Settings) -> String {
        let mut report = format!("Jig STL report\nFile: {}\nSource units: {} (STL has no units)\nModel scale: {:.2}%\nMode: {}\n", self.name, self.unit.label(), self.scale_percent, self.mode.label());
        if let Some(profile) = settings.selected_profile() {
            report.push_str(&format!(
                "Clay: {}\nCone: {}\nLinear shrinkage: {:.6}%\nBasis: {}\n",
                profile.name,
                profile.cone,
                profile.linear_shrinkage,
                profile.basis.label()
            ));
            if let Some(source) = profile.source {
                report.push_str(&format!("Source: {source}\n"));
            }
        }
        if let Some(Ok(measurements)) = self.measurements(settings) {
            let [x, y, z] = measurements.dimensions_mm;
            report.push_str(&format!(
                "Dimensions: {x:.6} × {y:.6} × {z:.6} mm\nSurface area: {:.6} mm² / {:.6} cm²\n",
                measurements.surface_area_mm2,
                measurements.surface_area_mm2 / 100.0
            ));
            if let Some(volume) = measurements.volume_mm3 {
                report.push_str(&format!(
                    "Total volume: {volume:.6} mm³ / {:.6} cm³ (mL) / {:.6} in³\n",
                    volume / 1000.0,
                    volume / 25.4_f64.powi(3)
                ));
            } else {
                report.push_str("Total volume: unavailable (mesh must be closed, nondegenerate and coherently oriented)\n");
            }
            if self.mode != ShrinkMode::Off {
                if let Some(Ok(base)) = self
                    .mesh
                    .as_ref()
                    .map(|mesh| mesh.measure(self.unit, self.scale_percent / 100.0))
                {
                    if let Some(volume) = base.volume_mm3 {
                        report.push_str(&format!("Base model volume: {volume:.6} mm³\n"));
                    }
                }
            }
        }
        report.push_str("Volume assumes nonintersecting, nonoverlapping shells; self-intersections are not detected.\nShrinkage is an isotropic linear estimate, not a prediction of warping or kiln behavior.\nExported STL keeps the selected source units; STL contains no unit metadata.\n");
        report
    }

    pub fn destroy(&mut self) {
        if let (Some(gl), Some(renderer)) = (&self.gl, &self.renderer) {
            renderer.lock().destroy(gl);
        }
        self.renderer = None;
    }
}

fn same_mesh(a: &Option<Arc<Mesh>>, b: &Option<Arc<Mesh>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

fn mesh_identity(mesh: &Arc<Mesh>) -> (usize, usize) {
    (
        Arc::as_ptr(mesh) as usize,
        std::mem::size_of::<Mesh>()
            + mesh.triangles.capacity() * std::mem::size_of::<stl::Triangle>(),
    )
}

fn sample_cube() -> Result<Mesh, String> {
    let vertices = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
        [0.0, 0.0, 10.0],
        [10.0, 0.0, 10.0],
        [10.0, 10.0, 10.0],
        [0.0, 10.0, 10.0],
    ];
    let mut triangles = Vec::new();
    for [a, b, c, d] in [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [3, 7, 6, 2],
        [0, 4, 7, 3],
        [1, 2, 6, 5],
    ] {
        triangles.push([vertices[a], vertices[b], vertices[c]]);
        triangles.push([vertices[a], vertices[c], vertices[d]]);
    }
    Mesh::from_triangles(triangles)
}

fn camera_axes(yaw: f32, pitch: f32) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    (
        [cy, -sy, 0.0],
        [sp * sy, sp * cy, cp],
        [cp * sy, cp * cy, -sp],
    )
}

#[derive(Clone, Copy, PartialEq)]
struct View {
    yaw: f32,
    pitch: f32,
    scale: f32,
}

/// Two small GL programs render the full mesh into a depth-buffered texture,
/// then composite it into egui's existing framebuffer. No model decimation or
/// painter-order approximation is needed, including for concave models.
struct Renderer {
    mesh_program: glow::Program,
    composite_program: glow::Program,
    vertex_array: glow::VertexArray,
    buffer: glow::Buffer,
    framebuffer: glow::Framebuffer,
    color: glow::Texture,
    depth: glow::Renderbuffer,
    vertices: i32,
    size: [i32; 2],
    last_view: Option<View>,
    destroyed: bool,
    error: Option<String>,
}

impl Renderer {
    fn new(gl: &glow::Context) -> Result<Self, String> {
        let version = egui_glow::ShaderVersion::get(gl);
        if !version.is_new_shader_interface() {
            return Err("3D rendering needs WebGL2. Measurements remain available.".into());
        }
        // All GL work runs on eframe's render thread, using its current context.
        unsafe {
            let mut programs = Vec::new();
            let mut arrays = Vec::new();
            let mut buffers = Vec::new();
            let mut framebuffers = Vec::new();
            let mut textures = Vec::new();
            let mut renderbuffers = Vec::new();
            let result = (|| {
                let mesh_program = program(
                    gl,
                    version.version_declaration(),
                    MESH_VERTEX,
                    MESH_FRAGMENT,
                )?;
                programs.push(mesh_program);
                let composite_program = program(
                    gl,
                    version.version_declaration(),
                    QUAD_VERTEX,
                    QUAD_FRAGMENT,
                )?;
                programs.push(composite_program);
                let vertex_array = gl.create_vertex_array()?;
                arrays.push(vertex_array);
                let buffer = gl.create_buffer()?;
                buffers.push(buffer);
                let framebuffer = gl.create_framebuffer()?;
                framebuffers.push(framebuffer);
                let color = gl.create_texture()?;
                textures.push(color);
                let depth = gl.create_renderbuffer()?;
                renderbuffers.push(depth);
                Ok(Self {
                    mesh_program,
                    composite_program,
                    vertex_array,
                    buffer,
                    framebuffer,
                    color,
                    depth,
                    vertices: 0,
                    size: [0, 0],
                    last_view: None,
                    destroyed: false,
                    error: None,
                })
            })();
            if result.is_err() {
                for handle in programs {
                    gl.delete_program(handle);
                }
                for handle in arrays {
                    gl.delete_vertex_array(handle);
                }
                for handle in buffers {
                    gl.delete_buffer(handle);
                }
                for handle in framebuffers {
                    gl.delete_framebuffer(handle);
                }
                for handle in textures {
                    gl.delete_texture(handle);
                }
                for handle in renderbuffers {
                    gl.delete_renderbuffer(handle);
                }
            }
            result
        }
    }

    fn upload(&mut self, gl: &glow::Context, mesh: &Mesh) -> Result<(), String> {
        let center: [f64; 3] =
            std::array::from_fn(|axis| mesh.bounds.min[axis] * 0.5 + mesh.bounds.max[axis] * 0.5);
        let extent = mesh.bounds.dimensions().into_iter().fold(0.0_f64, f64::max);
        if extent <= 0.0 || !extent.is_finite() {
            return Err("Model has no usable spatial extent for a 3D view.".into());
        }
        let mut bytes = Vec::with_capacity(mesh.triangles.len() * 3 * 6 * 4);
        for triangle in &mesh.triangles {
            let points: [[f32; 3]; 3] = triangle.map(|point| {
                std::array::from_fn(|axis| ((point[axis] - center[axis]) / extent) as f32)
            });
            let a: [f32; 3] = std::array::from_fn(|axis| points[1][axis] - points[0][axis]);
            let b: [f32; 3] = std::array::from_fn(|axis| points[2][axis] - points[0][axis]);
            let mut normal = [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ];
            let length = normal.iter().map(|value| value * value).sum::<f32>().sqrt();
            if length > 0.0 {
                normal = normal.map(|value| value / length);
            } else {
                normal = [0.0, 0.0, 1.0];
            }
            for point in points {
                for value in point.into_iter().chain(normal) {
                    bytes.extend_from_slice(&value.to_ne_bytes());
                }
            }
        }
        // The uploaded format is six tightly packed f32s per vertex.
        unsafe {
            gl.bind_vertex_array(Some(self.vertex_array));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.buffer));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &bytes, glow::STATIC_DRAW);
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 24, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, 24, 12);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            if gl.get_error() != glow::NO_ERROR {
                return Err(
                    "Could not allocate the 3D model buffer. Measurements remain available.".into(),
                );
            }
        }
        self.vertices = (mesh.triangles.len() * 3) as i32;
        self.last_view = None;
        self.error = None;
        Ok(())
    }

    fn paint(&mut self, info: &egui::PaintCallbackInfo, painter: &egui_glow::Painter, view: View) {
        if self.destroyed || self.vertices == 0 || self.error.is_some() {
            return;
        }
        let gl = painter.gl();
        let viewport = info.viewport_in_pixels();
        let clip = info.clip_rect_in_pixels();
        let reduction = (2048.0 / viewport.width_px.max(viewport.height_px).max(1) as f32).min(1.0);
        let width = (viewport.width_px as f32 * reduction).round().max(1.0) as i32;
        let height = (viewport.height_px as f32 * reduction).round().max(1.0) as i32;
        // A private depth attachment avoids depending on egui's canvas depth
        // settings. The texture is redrawn only after geometry/view/size changes.
        unsafe {
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);
            if self.size != [width, height] {
                gl.bind_texture(glow::TEXTURE_2D, Some(self.color));
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_S,
                    glow::CLAMP_TO_EDGE as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_T,
                    glow::CLAMP_TO_EDGE as i32,
                );
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA as i32,
                    width,
                    height,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(None),
                );
                gl.bind_renderbuffer(glow::RENDERBUFFER, Some(self.depth));
                gl.renderbuffer_storage(glow::RENDERBUFFER, glow::DEPTH_COMPONENT16, width, height);
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.framebuffer));
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(self.color),
                    0,
                );
                gl.framebuffer_renderbuffer(
                    glow::FRAMEBUFFER,
                    glow::DEPTH_ATTACHMENT,
                    glow::RENDERBUFFER,
                    Some(self.depth),
                );
                if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                    self.error = Some(
                        "The 3D framebuffer could not be allocated. Measurements remain available."
                            .into(),
                    );
                    gl.bind_framebuffer(glow::FRAMEBUFFER, painter.intermediate_fbo());
                    gl.bind_renderbuffer(glow::RENDERBUFFER, None);
                    gl.enable(glow::SCISSOR_TEST);
                    gl.scissor(
                        clip.left_px,
                        clip.from_bottom_px,
                        clip.width_px,
                        clip.height_px,
                    );
                    return;
                }
                self.size = [width, height];
                self.last_view = None;
            }
            if self.last_view != Some(view) {
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.framebuffer));
                gl.viewport(0, 0, width, height);
                gl.enable(glow::DEPTH_TEST);
                gl.depth_mask(true);
                gl.depth_func(glow::LESS);
                gl.disable(glow::CULL_FACE);
                gl.clear_color(12.0 / 255.0, 19.0 / 255.0, 27.0 / 255.0, 1.0);
                gl.clear_depth_f32(1.0);
                gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
                gl.use_program(Some(self.mesh_program));
                gl.uniform_2_f32(
                    gl.get_uniform_location(self.mesh_program, "u_angles")
                        .as_ref(),
                    view.yaw,
                    view.pitch,
                );
                gl.uniform_2_f32(
                    gl.get_uniform_location(self.mesh_program, "u_fit").as_ref(),
                    1.1 * (viewport.height_px as f32 / viewport.width_px.max(1) as f32).min(1.0)
                        * view.scale,
                    1.1 * (viewport.width_px as f32 / viewport.height_px.max(1) as f32).min(1.0)
                        * view.scale,
                );
                gl.bind_vertex_array(Some(self.vertex_array));
                gl.draw_arrays(glow::TRIANGLES, 0, self.vertices);
                self.last_view = Some(view);
            }
            gl.bind_framebuffer(glow::FRAMEBUFFER, painter.intermediate_fbo());
            gl.viewport(
                viewport.left_px,
                viewport.from_bottom_px,
                viewport.width_px,
                viewport.height_px,
            );
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(
                clip.left_px,
                clip.from_bottom_px,
                clip.width_px,
                clip.height_px,
            );
            gl.disable(glow::DEPTH_TEST);
            gl.depth_mask(false);
            gl.use_program(Some(self.composite_program));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.color));
            gl.uniform_1_i32(
                gl.get_uniform_location(self.composite_program, "u_image")
                    .as_ref(),
                0,
            );
            gl.bind_vertex_array(Some(self.vertex_array));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_vertex_array(None);
            gl.bind_renderbuffer(glow::RENDERBUFFER, None);
            // egui_glow restores its remaining program/buffer/blend state after
            // every callback; the framebuffer and depth state are restored here.
        }
    }

    fn destroy(&mut self, gl: &glow::Context) {
        if self.destroyed {
            return;
        }
        unsafe {
            gl.delete_program(self.mesh_program);
            gl.delete_program(self.composite_program);
            gl.delete_vertex_array(self.vertex_array);
            gl.delete_buffer(self.buffer);
            gl.delete_framebuffer(self.framebuffer);
            gl.delete_texture(self.color);
            gl.delete_renderbuffer(self.depth);
        }
        self.destroyed = true;
    }
}

fn program(
    gl: &glow::Context,
    version: &str,
    vertex: &str,
    fragment: &str,
) -> Result<glow::Program, String> {
    // Shader text is fixed application code; errors are reported inline rather
    // than panicking and taking the rest of the workbench down.
    unsafe {
        let program = gl.create_program()?;
        let mut shaders = Vec::new();
        for (kind, source) in [
            (glow::VERTEX_SHADER, vertex),
            (glow::FRAGMENT_SHADER, fragment),
        ] {
            let shader = match gl.create_shader(kind) {
                Ok(shader) => shader,
                Err(error) => {
                    for shader in shaders {
                        gl.delete_shader(shader);
                    }
                    gl.delete_program(program);
                    return Err(error);
                }
            };
            gl.shader_source(shader, &format!("{version}\n{source}"));
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                let error = gl.get_shader_info_log(shader);
                gl.delete_shader(shader);
                for shader in shaders {
                    gl.delete_shader(shader);
                }
                gl.delete_program(program);
                return Err(format!("Could not prepare the 3D view: {error}"));
            }
            gl.attach_shader(program, shader);
            shaders.push(shader);
        }
        gl.bind_attrib_location(program, 0, "a_position");
        gl.bind_attrib_location(program, 1, "a_normal");
        gl.link_program(program);
        for shader in shaders {
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
        }
        if !gl.get_program_link_status(program) {
            let error = gl.get_program_info_log(program);
            gl.delete_program(program);
            return Err(format!("Could not prepare the 3D view: {error}"));
        }
        Ok(program)
    }
}

const MESH_VERTEX: &str = r#"
precision highp float;
in vec3 a_position;
in vec3 a_normal;
uniform vec2 u_angles;
uniform vec2 u_fit;
out vec3 v_normal;
void main() {
    float sy = sin(u_angles.x), cy = cos(u_angles.x);
    float sp = sin(u_angles.y), cp = cos(u_angles.y);
    vec3 right = vec3(cy, -sy, 0.0);
    vec3 up = vec3(sp * sy, sp * cy, cp);
    vec3 depth = vec3(cp * sy, cp * cy, -sp);
    gl_Position = vec4(dot(right, a_position) * u_fit.x,
                       dot(up, a_position) * u_fit.y,
                       dot(depth, a_position) * 0.5, 1.0);
    v_normal = vec3(dot(right, a_normal), dot(up, a_normal), dot(depth, a_normal));
}
"#;
const MESH_FRAGMENT: &str = r#"
precision mediump float;
in vec3 v_normal;
out vec4 out_color;
void main() {
    float light = 0.30 + 0.70 * abs(dot(normalize(v_normal), normalize(vec3(-0.4, 0.6, 1.0))));
    out_color = vec4(vec3(0.965, 0.855, 0.682) * light, 1.0);
}
"#;
const QUAD_VERTEX: &str = r#"
precision highp float;
out vec2 v_uv;
void main() {
    vec2 p = vec2((gl_VertexID == 1) ? 3.0 : -1.0,
                  (gl_VertexID == 2) ? 3.0 : -1.0);
    v_uv = (p + 1.0) * 0.5;
    gl_Position = vec4(p, 0.0, 1.0);
}
"#;
const QUAD_FRAGMENT: &str = r#"
precision mediump float;
in vec2 v_uv;
uniform sampler2D u_image;
out vec4 out_color;
void main() {
    out_color = texture(u_image, v_uv);
}
"#;
