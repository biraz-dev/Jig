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
    stl_camera::{self, Axis, Perspective, Projection},
    stl_comparison::{silhouette_mm, ComparisonLayout, HUMAN_HEIGHT_MM},
};
use std::{cell::RefCell, f32::consts::FRAC_PI_2, sync::Arc, time::Duration};
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
    axis_scale: [f64; 3],
    linked: bool,
    export_unit: LengthUnit,
    projection: Projection,
    human_comparison: bool,
    grey_matcap: bool,
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
    AxisPercent(usize),
    TargetSize(usize),
}

#[derive(PartialEq)]
struct MeasurementKey {
    mesh: usize,
    unit: LengthUnit,
    axes: [u64; 3],
}

struct MeasurementCache {
    key: MeasurementKey,
    value: Result<Measurements, String>,
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
    axis_scale: [f64; 3],
    linked: bool,
    export_unit: LengthUnit,
    projection: Projection,
    human_comparison: bool,
    grey_matcap: bool,
    mode: ShrinkMode,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    viewport_rect: Option<egui::Rect>,
    size_open: bool,
    focus_size: bool,
    measurement_cache: RefCell<Vec<MeasurementCache>>,
    orbit_allowed: bool,
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
            axis_scale: [1.0; 3],
            linked: true,
            export_unit: LengthUnit::Millimeters,
            projection: Projection::Orthographic,
            human_comparison: false,
            grey_matcap: false,
            mode: ShrinkMode::Off,
            yaw: 0.65,
            pitch: 0.55,
            zoom: 1.0,
            viewport_rect: None,
            size_open: false,
            focus_size: false,
            measurement_cache: RefCell::new(Vec::new()),
            orbit_allowed: true,
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
        self.axis_scale = [1.0; 3];
        self.linked = true;
        self.export_unit = LengthUnit::Millimeters;
        self.projection = Projection::Orthographic;
        self.human_comparison = false;
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
        self.measurement_cache.get_mut().clear();
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
            axis_scale: self.axis_scale,
            linked: self.linked,
            export_unit: self.export_unit,
            projection: self.projection,
            human_comparison: self.human_comparison,
            grey_matcap: self.grey_matcap,
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
            || self.axis_scale.map(f64::to_bits) != before.axis_scale.map(f64::to_bits)
            || self.linked != before.linked
            || self.export_unit != before.export_unit
            || self.projection != before.projection
            || self.human_comparison != before.human_comparison
            || self.grey_matcap != before.grey_matcap
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
        self.axis_scale = before.axis_scale;
        self.linked = before.linked;
        self.export_unit = before.export_unit;
        self.projection = before.projection;
        self.human_comparison = before.human_comparison;
        self.grey_matcap = before.grey_matcap;
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

    fn shrink_factor(&self, settings: &Settings) -> f64 {
        let shrink = settings
            .selected_profile()
            .and_then(|profile| stl::shrinkage(profile.linear_shrinkage).ok());
        match (self.mode, shrink) {
            (ShrinkMode::Estimate, Some(shrink)) => shrink.remaining,
            (ShrinkMode::Compensate, Some(shrink)) => shrink.compensation,
            _ => 1.0,
        }
    }

    fn effective_axes(&self, settings: &Settings) -> [f64; 3] {
        let uniform = self.scale_percent / 100.0 * self.shrink_factor(settings);
        self.axis_scale.map(|axis| axis * uniform)
    }

    fn base_axes(&self) -> [f64; 3] {
        self.axis_scale
            .map(|axis| axis * self.scale_percent / 100.0)
    }

    fn measure_axes(&self, axes: [f64; 3]) -> Option<Result<Measurements, String>> {
        let mesh = self.mesh.as_ref()?;
        let key = MeasurementKey {
            mesh: Arc::as_ptr(mesh) as usize,
            unit: self.unit,
            axes: axes.map(f64::to_bits),
        };
        if let Some(cached) = self
            .measurement_cache
            .borrow()
            .iter()
            .find(|item| item.key == key)
        {
            return Some(cached.value.clone());
        }
        let value = mesh.measure_axes(self.unit, axes);
        let mut cache = self.measurement_cache.borrow_mut();
        if cache.len() == 2 {
            cache.remove(0);
        }
        cache.push(MeasurementCache {
            key,
            value: value.clone(),
        });
        Some(value)
    }

    fn measurements(&self, settings: &Settings) -> Option<Result<Measurements, String>> {
        self.measure_axes(self.effective_axes(settings))
    }

    fn fit_model(&mut self, settings: &Settings) -> Result<(), String> {
        if self.human_comparison {
            self.zoom = 1.0;
            return Ok(());
        }
        let mesh = self
            .mesh
            .as_ref()
            .ok_or("Open a model before fitting the view")?;
        let dimensions = mesh.bounds.dimensions();
        let extent = dimensions.into_iter().fold(0.0_f64, f64::max);
        let axes = self.effective_axes(settings);
        let transformed_extent = dimensions
            .into_iter()
            .zip(axes)
            .map(|(dimension, factor)| dimension * factor)
            .fold(0.0_f64, f64::max);
        if !extent.is_finite()
            || extent <= 0.0
            || !transformed_extent.is_finite()
            || transformed_extent <= 0.0
        {
            return Err("The model has no finite nonzero extent to fit.".into());
        }
        let zoom = (extent / transformed_extent) as f32;
        if !zoom.is_finite() || zoom <= 0.0 {
            return Err("The model size exceeds the camera's numeric range.".into());
        }
        self.zoom = zoom;
        Ok(())
    }

    fn export_axes(&self, settings: &Settings) -> [f64; 3] {
        let units = self.unit.millimeters_per_unit() / self.export_unit.millimeters_per_unit();
        self.effective_axes(settings).map(|axis| axis * units)
    }

    pub fn request_size_focus(&mut self) {
        self.size_open = true;
        self.focus_size = true;
        self.focus_open = false;
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
        self.viewport_rect = None;
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
                    match mesh.to_binary_stl_axes(self.export_axes(settings)) {
                        Ok(bytes) => {
                            let stem = self
                                .name
                                .rsplit_once('.')
                                .filter(|(_, extension)| extension.eq_ignore_ascii_case("stl"))
                                .map_or(self.name.as_str(), |(stem, _)| stem);
                            export_file(
                                &bytes,
                                &format!("{stem}-jig-{}.stl", self.export_unit.label()),
                            );
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
                    self.human_comparison = false;
                    if label == "Iso" {
                        self.projection = Projection::Orthographic;
                    }
                    self.push_snapshot(before);
                }
            }
            let fit = ui.button("Fit");
            self.focused |= fit.has_focus();
            if fit.clicked() {
                let before = self.prepare_snapshot();
                match self.fit_model(settings) {
                    Ok(()) => self.error = None,
                    Err(error) => self.error = Some(error),
                }
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
            let perspective = ui
                .selectable_label(self.projection == Projection::Perspective, "Perspective")
                .on_hover_text(
                    "Toggle a 45° perspective camera. Off uses orthographic projection.",
                );
            self.focused |= perspective.has_focus();
            if perspective.clicked() {
                let before = self.prepare_snapshot();
                self.human_comparison = false;
                self.projection = if self.projection == Projection::Orthographic {
                    Projection::Perspective
                } else {
                    Projection::Orthographic
                };
                self.push_snapshot(before);
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
        self.human_toggle(ui);
        self.matcap_toggle(ui);
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
                if let Some(volume) = measurements.volume_label() {
                    ui.label(RichText::new(volume).color(CREAM).size(24.0));
                    ui.label(
                        RichText::new(format!(
                            "Total volume · {}",
                            measurements.volume_alternatives_label().unwrap_or_default()
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
                    RichText::new(format!("Area {}", measurements.area_label()))
                        .color(CREAM)
                        .size(12.0),
                );
                ui.label(
                    RichText::new(measurements.dimensions_label())
                        .color(CREAM)
                        .size(12.0),
                );
                if self.mode != ShrinkMode::Off {
                    if let Some(Ok(base)) = self.measure_axes(self.base_axes()) {
                        if let Some(volume) = base.volume_label() {
                            ui.label(
                                RichText::new(format!("Base model {volume}"))
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
            ui.label("Uniform scale");
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
        self.size_controls(ui, settings);
        changed
    }

    fn finish_scale_edit(
        &mut self,
        ui: &Ui,
        response: &egui::Response,
        before: Snapshot,
        kind: Gesture,
    ) {
        if response.changed() && self.differs(&before) {
            self.begin_gesture(kind, before, ui.input(|input| input.time));
        }
        let finish = response.drag_stopped()
            || response.lost_focus()
            || ui.input(|input| input.key_pressed(egui::Key::Enter));
        if finish
            && self
                .pending_gesture
                .as_ref()
                .is_some_and(|pending| pending.kind == kind)
        {
            self.finish_gesture();
        }
    }

    fn apply_axis_ratio(
        &mut self,
        axis: usize,
        ratio: f64,
        settings: &Settings,
    ) -> Result<(), String> {
        if !ratio.is_finite() || ratio <= 0.0 {
            return Err("Scale and target dimensions must be positive finite numbers.".into());
        }
        let mut axes = self.axis_scale;
        for (index, value) in axes.iter_mut().enumerate() {
            if self.linked || index == axis {
                *value *= ratio;
            }
            let percent = *value * self.scale_percent;
            if !percent.is_finite() || !(0.000001..=1.0e10).contains(&percent) {
                return Err(
                    "Requested size requires an axis scale outside 0.000001–10000000000%.".into(),
                );
            }
        }
        let effective =
            axes.map(|value| value * self.scale_percent / 100.0 * self.shrink_factor(settings));
        if let Some(mesh) = &self.mesh {
            mesh.measure_axes(self.unit, effective)?;
        }
        self.axis_scale = axes;
        self.error = None;
        Ok(())
    }

    fn size_controls(&mut self, ui: &mut Ui, settings: &Settings) {
        ui.label(
            RichText::new(format!(
                "Export: {} · import STL as {}",
                self.export_unit.label(),
                self.export_unit.label()
            ))
            .color(MUTED)
            .size(11.0),
        );
        let size = ui.selectable_label(self.size_open, "Size / export");
        self.focused |= size.has_focus();
        if size.clicked() {
            self.size_open = !self.size_open;
        }
        if !self.size_open {
            return;
        }
        ui.indent("stl-size-export", |ui| {
            let focus_size = self.focus_size;
            if focus_size {
                let id = ui.next_auto_id();
                ui.memory_mut(|memory| memory.request_focus(id));
                self.focus_size = false;
                self.focused = true;
                ui.ctx().request_repaint();
            }
            let before = self.snapshot();
            let link = ui.checkbox(&mut self.linked, "Linked axes").on_hover_text("Linked edits preserve the object's current proportions. Uncheck to resize X, Y and Z independently.");
            self.focused |= link.has_focus();
            if focus_size {
                link.scroll_to_me(Some(egui::Align::Min));
            }
            if link.changed() {
                self.finish_gesture();
                self.push_snapshot(before);
            }
            ui.label(RichText::new("Total scale before shrinkage").color(MUTED).size(11.0));
            for axis in Axis::ALL {
                let index = axis.index();
                ui.horizontal(|ui| {
                    ui.label(axis.label());
                    let original = self.scale_percent * self.axis_scale[index];
                    let mut percent = original;
                    let before = self.snapshot();
                    let response = ui.push_id(("axis-percent", index), |ui| {
                        ui.add(egui::DragValue::new(&mut percent).range(0.000001..=1.0e10).clamp_existing_to_range(false).speed(1.0).suffix("%"))
                    }).inner;
                    self.focused |= response.has_focus();
                    if response.changed() {
                        if let Err(error) = self.apply_axis_ratio(index, percent / original, settings) {
                            self.error = Some(error);
                        }
                    }
                    self.finish_scale_edit(ui, &response, before, Gesture::AxisPercent(index));
                });
            }
            ui.label(RichText::new("Export dimensions (mm, including shrinkage mode)").color(MUTED).size(11.0));
            for axis in Axis::ALL {
                let index = axis.index();
                ui.horizontal(|ui| {
                    ui.label(axis.label());
                    let current = self.measurements(settings).and_then(Result::ok).map(|measure| measure.dimensions_mm[index]).unwrap_or(0.0);
                    let mut target = current;
                    let before = self.snapshot();
                    let response = ui.push_id(("target-dimension", index), |ui| {
                        ui.add_enabled(current > 0.0, egui::DragValue::new(&mut target).range(0.000001..=1.0e12).clamp_existing_to_range(false).speed(0.1).suffix(" mm"))
                    }).inner;
                    self.focused |= response.has_focus();
                    if current == 0.0 {
                        response.clone().on_hover_text("This axis has zero extent and cannot be resized to a nonzero dimension.");
                    }
                    if response.changed() {
                        if let Err(error) = self.apply_axis_ratio(index, target / current, settings) {
                            self.error = Some(error);
                        }
                    }
                    self.finish_scale_edit(ui, &response, before, Gesture::TargetSize(index));
                });
            }
            let reset = ui.button("Reset scaling");
            self.focused |= reset.has_focus();
            if reset.clicked() {
                let before = self.prepare_snapshot();
                self.scale_percent = 100.0;
                self.axis_scale = [1.0; 3];
                self.error = None;
                self.push_snapshot(before);
            }
            ui.horizontal_wrapped(|ui| {
                ui.label("Export units");
                for unit in LengthUnit::ALL {
                    let response = ui.selectable_label(self.export_unit == unit, unit.label());
                    self.focused |= response.has_focus();
                    if response.clicked() && self.export_unit != unit {
                        let before = self.prepare_snapshot();
                        self.export_unit = unit;
                        self.push_snapshot(before);
                    }
                }
            });
            ui.label(RichText::new("STL has no unit metadata. The filename and copied report state the export units; use them when importing the file.").color(MUTED).size(11.0));
        });
    }

    fn viewport(&mut self, ui: &mut Ui, height: f32, settings: &Settings) {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::click_and_drag(),
        );
        self.focused |= response.has_focus();
        self.viewport_rect = Some(rect);
        let comparison = self.comparison_layout(ui, rect, settings);
        let now = ui.input(|input| input.time);
        if response.drag_started() {
            self.orbit_allowed =
                ui.input(|input| input.pointer.press_origin())
                    .is_none_or(|position| {
                        !gnomon_rect(ui, rect).contains(position)
                            && !viewport_toggle_rect(ui, rect, false).contains(position)
                            && !viewport_toggle_rect(ui, rect, true).contains(position)
                    });
        }
        if response.dragged() && self.orbit_allowed && !self.human_comparison {
            let motion = response.drag_motion();
            if motion != egui::Vec2::ZERO {
                let before = self.snapshot();
                self.begin_gesture(Gesture::Orbit, before, now);
                self.yaw += motion.x * 0.012;
                self.pitch = (self.pitch + motion.y * 0.012).clamp(-3.0, 3.0);
            }
        }
        if response.drag_stopped() {
            self.orbit_allowed = true;
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
        response.on_hover_text(if self.human_comparison {
            "Front comparison · wheel or +/− to zoom · turn Human off to rotate"
        } else {
            "Drag to rotate · wheel or +/− to zoom · Fit reframes the model"
        });
        ui.painter().rect_filled(rect, 0.0, BG);
        if self.graphics_error.is_none() {
            if let Some(renderer) = &self.renderer {
                let renderer = Arc::clone(renderer);
                let effective = self.effective_axes(settings);
                let largest = effective.into_iter().fold(0.0_f64, f64::max);
                let view = View {
                    yaw: self.yaw,
                    pitch: self.pitch,
                    scale: (self.zoom as f64 * largest) as f32,
                    axes: effective.map(|axis| (axis / largest) as f32),
                    projection: self.projection,
                    grey_matcap: self.grey_matcap,
                    comparison: comparison.map(|(layout, content)| {
                        let extent = self
                            .mesh
                            .as_ref()
                            .map(|mesh| {
                                mesh.bounds.dimensions().into_iter().fold(0.0_f64, f64::max)
                            })
                            .unwrap_or(1.0);
                        let factor = self.unit.millimeters_per_unit() * largest;
                        let content_scale = [
                            content.width() / rect.width(),
                            content.height() / rect.height(),
                        ];
                        let content_offset = [
                            2.0 * (content.center().x - rect.center().x) / rect.width(),
                            -2.0 * (content.center().y - rect.center().y) / rect.height(),
                        ];
                        let coefficients = layout.mm_to_ndc();
                        let object_offset = layout.object_offset_ndc();
                        let scale = std::array::from_fn(|axis| {
                            (coefficients[axis]
                                * extent
                                * factor
                                * f64::from(self.zoom)
                                * f64::from(content_scale[axis])) as f32
                        });
                        let offset = std::array::from_fn(|axis| {
                            (object_offset[axis]
                                * f64::from(self.zoom)
                                * f64::from(content_scale[axis])) as f32
                                + content_offset[axis]
                        });
                        let clip = [
                            (content.left() - rect.left()) / rect.width(),
                            (rect.bottom() - content.bottom()) / rect.height(),
                            content_scale[0],
                            content_scale[1],
                        ];
                        ComparisonView {
                            scale,
                            offset,
                            clip,
                        }
                    }),
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
        if let Some((layout, content)) = comparison {
            self.draw_human(ui, rect, content, layout);
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

    pub fn toggle_human_comparison(&mut self) {
        if self.mesh.is_none() {
            return;
        }
        let before = self.prepare_snapshot();
        self.human_comparison = !self.human_comparison;
        if self.human_comparison {
            self.projection = Projection::Orthographic;
            self.yaw = 0.0;
            self.pitch = 0.0;
            self.zoom = 1.0;
        }
        self.push_snapshot(before);
    }

    fn human_toggle(&mut self, ui: &mut Ui) {
        let Some(viewport) = self.viewport_rect else {
            return;
        };
        let rect = viewport_toggle_rect(ui, viewport, false);
        if rect == egui::Rect::NOTHING {
            return;
        }
        // Register after ordinary settings, even though this button is painted
        // over the viewport, so established keyboard navigation stays stable.
        let toggle = ui.put(rect, egui::Button::new("Human 5′9″").selected(self.human_comparison)).on_hover_text("Compare with a 69 in / 1752.6 mm human reference. Front X/Z view; the reference is never exported. Ctrl-Alt-H toggles.");
        self.focused |= toggle.has_focus();
        if toggle.clicked() {
            self.toggle_human_comparison();
            ui.ctx().request_repaint();
        }
    }

    /// Appearance is a local camera setting, never a change to mesh geometry.
    pub fn toggle_grey_matcap(&mut self) {
        if self.mesh.is_none() {
            return;
        }
        let before = self.prepare_snapshot();
        self.grey_matcap = !self.grey_matcap;
        self.push_snapshot(before);
    }

    fn matcap_toggle(&mut self, ui: &mut Ui) {
        let Some(viewport) = self.viewport_rect else {
            return;
        };
        let rect = viewport_toggle_rect(ui, viewport, true);
        if rect == egui::Rect::NOTHING {
            return;
        }
        // Append to keyboard order without shifting existing viewer controls.
        let toggle = ui
            .put(
                rect,
                egui::Button::new("Grey matcap").selected(self.grey_matcap),
            )
            .on_hover_text(
                "Neutral grey studio material for reading surface details. View only; never exported. Ctrl-Alt-G toggles.",
            );
        self.focused |= toggle.has_focus();
        if toggle.clicked() {
            self.toggle_grey_matcap();
            ui.ctx().request_repaint();
        }
    }

    fn comparison_layout(
        &self,
        ui: &Ui,
        rect: egui::Rect,
        settings: &Settings,
    ) -> Option<(ComparisonLayout, egui::Rect)> {
        if !self.human_comparison {
            return None;
        }
        let visible = rect.intersect(ui.clip_rect());
        let content = egui::Rect::from_min_max(
            egui::pos2(visible.left(), visible.top() + 48.0),
            visible.max,
        );
        if content.width() <= 0.0 || content.height() <= 0.0 {
            return None;
        }
        let measurements = self.measurements(settings)?.ok()?;
        let layout = ComparisonLayout::new(
            measurements.dimensions_mm,
            f64::from(content.width() / content.height()),
        )
        .ok()?;
        Some((layout, content))
    }

    fn draw_human(
        &self,
        ui: &Ui,
        viewport: egui::Rect,
        content: egui::Rect,
        layout: ComparisonLayout,
    ) {
        let painter = ui.painter().with_clip_rect(content);
        let point = |world: [f64; 2]| {
            let ndc = layout.ndc(world);
            egui::pos2(
                content.center().x + (ndc[0] * f64::from(self.zoom)) as f32 * content.width() * 0.5,
                content.center().y
                    - (ndc[1] * f64::from(self.zoom)) as f32 * content.height() * 0.5,
            )
        };
        let baseline = [
            point([0.0, layout.baseline_z_mm]),
            point([layout.content_size_mm[0], layout.baseline_z_mm]),
        ];
        painter.line_segment(baseline, Stroke::new(1.0_f32, RULE));
        for polygon in silhouette_mm() {
            let points = polygon
                .into_iter()
                .map(|[x, z]| point([x + layout.person_center_x_mm, z + layout.baseline_z_mm]))
                .collect();
            painter.add(egui::Shape::convex_polygon(
                points,
                Color32::from_rgb(148, 188, 160),
                Stroke::NONE,
            ));
        }
        let visible = viewport.intersect(ui.clip_rect());
        ui.painter().with_clip_rect(visible).text(
            egui::pos2(visible.center().x, visible.top() + 34.0),
            egui::Align2::CENTER_TOP,
            "Front X/Z · 69 in / 1752.6 mm reference",
            FontId::monospace(11.0),
            MUTED,
        );
    }

    fn gnomon(&mut self, ui: &mut Ui, viewport: egui::Rect) {
        let rect = gnomon_rect(ui, viewport);
        if rect.width() < 86.0 || rect.height() < 86.0 {
            return;
        }
        let center = rect.center();
        ui.painter()
            .circle_filled(center, 36.0, BG.gamma_multiply(0.92));
        let (right, up, depth) = stl_camera::axes(self.yaw, self.pitch);
        let mut axes = [
            (Axis::X, PINK),
            (Axis::Y, Color32::from_rgb(148, 188, 160)),
            (Axis::Z, Color32::from_rgb(109, 185, 238)),
        ];
        axes.sort_by(|a, b| depth[b.0.index()].total_cmp(&depth[a.0.index()]));
        for (axis, color) in axes {
            let index = axis.index();
            let direction = egui::vec2(right[index], -up[index]);
            let label_center = if direction.length_sq() < 0.02 {
                ui.painter()
                    .circle_stroke(center, 4.0, Stroke::new(1.0_f32, color));
                center + egui::vec2(-11.0, 10.0)
            } else {
                let end = center + direction * 25.0;
                ui.painter()
                    .line_segment([center, end], Stroke::new(1.5_f32, color));
                center + direction * 31.0
            };
            ui.painter().text(
                label_center,
                egui::Align2::CENTER_CENTER,
                axis.label(),
                FontId::monospace(11.0),
                color,
            );
            // Mouse/touch axis labels share the exact model basis. Preset
            // buttons remain the keyboard path, preserving existing tab order.
            let target =
                egui::Rect::from_center_size(label_center, egui::vec2(20.0, 20.0)).intersect(rect);
            let sense = egui::Sense::click().difference(egui::Sense::focusable_noninteractive());
            let click = ui
                .interact(target, ui.id().with(("stl-axis-snap", index)), sense)
                .on_hover_text(format!(
                    "Look from +{} · right-click for −{}",
                    axis.label(),
                    axis.label()
                ));
            if click.clicked() || click.secondary_clicked() {
                let before = self.prepare_snapshot();
                (self.yaw, self.pitch) = axis.snap_angles(!click.secondary_clicked());
                self.human_comparison = false;
                self.focused = true;
                self.push_snapshot(before);
                ui.ctx().request_repaint();
            }
        }
        ui.painter().circle_filled(center, 2.0, CREAM);
    }

    fn report(&self, settings: &Settings) -> String {
        let mut report = format!("Jig STL report\nFile: {}\nSource units: {} (STL has no units)\nModel scale: {:.2}%\nMode: {}\n", self.name, self.unit.label(), self.scale_percent, self.mode.label());
        let axes = self.base_axes().map(|scale| scale * 100.0);
        report.push_str(&format!("Axis scale: X {:.6}% · Y {:.6}% · Z {:.6}%\nScale edits: {}\nExport units: {} (import exported STL as {})\nProjection: {}{}\nCamera yaw: {:.6} rad\nCamera pitch: {:.6} rad\n", axes[0], axes[1], axes[2], if self.linked { "Linked" } else { "Independent" }, self.export_unit.label(), self.export_unit.label(), self.projection.label(), if self.projection == Projection::Perspective { " (45°)" } else { "" }, self.yaw, self.pitch));
        report.push_str(&format!(
            "Comparison: {}\nReference height: {:.6} mm (69 in)\n",
            if self.human_comparison { "On" } else { "Off" },
            HUMAN_HEIGHT_MM
        ));
        report.push_str(&format!(
            "Viewport material: {}\n",
            if self.grey_matcap {
                "Grey matcap"
            } else {
                "Warm clay"
            }
        ));
        if self.human_comparison {
            if let Some(Ok(measurements)) = self.measurements(settings) {
                report.push_str(&format!("Object / human height: {:.6}\nComparison view: Front X/Z, Z up, feet at model minimum Z. Reference is not exported.\n", measurements.dimensions_mm[2] / HUMAN_HEIGHT_MM));
            }
        }
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
                if let Some(Ok(base)) = self.measure_axes(self.base_axes()) {
                    if let Some(volume) = base.volume_mm3 {
                        report.push_str(&format!("Base model volume: {volume:.6} mm³\n"));
                    }
                }
            }
        }
        report.push_str("Volume assumes nonintersecting, nonoverlapping shells; self-intersections are not detected.\nShrinkage is an isotropic linear estimate, not a prediction of warping or kiln behavior.\nExported STL uses the export units stated above; import using those units. STL contains no unit metadata.\n");
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

fn viewport_toggle_rect(ui: &Ui, viewport: egui::Rect, left: bool) -> egui::Rect {
    let visible = viewport.intersect(ui.clip_rect());
    // Keep both controls separate on phones; keyboard shortcuts also work when
    // an exceptionally narrow/clipped viewport cannot display both buttons.
    let minimum_width = if left { 248.0 } else { 140.0 };
    if visible.width() < minimum_width || visible.height() < 40.0 {
        return egui::Rect::NOTHING;
    }
    let x = if left {
        visible.left() + 8.0
    } else {
        visible.right() - 120.0
    };
    egui::Rect::from_min_size(egui::pos2(x, visible.top() + 8.0), egui::vec2(112.0, 22.0))
}

fn gnomon_rect(ui: &Ui, viewport: egui::Rect) -> egui::Rect {
    let visible = viewport.intersect(ui.clip_rect());
    if visible.width() < 86.0 || visible.height() < 86.0 {
        return egui::Rect::NOTHING;
    }
    egui::Rect::from_min_size(
        egui::pos2(visible.left(), visible.bottom() - 86.0),
        egui::vec2(86.0, 86.0),
    )
}

#[derive(Clone, Copy, PartialEq)]
struct ComparisonView {
    scale: [f32; 2],
    offset: [f32; 2],
    clip: [f32; 4],
}

#[derive(Clone, Copy, PartialEq)]
struct View {
    yaw: f32,
    pitch: f32,
    scale: f32,
    axes: [f32; 3],
    projection: Projection,
    comparison: Option<ComparisonView>,
    grey_matcap: bool,
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
    half_extents: [f32; 3],
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
                    half_extents: [0.5; 3],
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
        self.half_extents = mesh
            .bounds
            .dimensions()
            .map(|dimension| (dimension / extent * 0.5) as f32);
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
                gl.uniform_3_f32(
                    gl.get_uniform_location(self.mesh_program, "u_axes")
                        .as_ref(),
                    view.axes[0],
                    view.axes[1],
                    view.axes[2],
                );
                let perspective = if view.projection == Projection::Perspective {
                    let aspect = viewport.width_px.max(1) as f32 / viewport.height_px.max(1) as f32;
                    let radius = (self.half_extents[0] * view.axes[0])
                        .hypot(self.half_extents[1] * view.axes[1])
                        .hypot(self.half_extents[2] * view.axes[2]);
                    let base_radius = self.half_extents[0]
                        .hypot(self.half_extents[1])
                        .hypot(self.half_extents[2]);
                    Perspective::fit_distance(aspect, base_radius)
                        .and_then(|fit| {
                            // Beyond this distance the model is already subpixel;
                            // retain enough f32 depth precision for a legal frustum.
                            let distance =
                                (fit / view.scale).clamp(radius * 1.05, radius * 100000.0);
                            Perspective::new(aspect, radius, distance)
                        })
                        .ok()
                } else {
                    None
                };
                gl.uniform_1_i32(
                    gl.get_uniform_location(self.mesh_program, "u_matcap")
                        .as_ref(),
                    i32::from(view.grey_matcap),
                );
                gl.uniform_1_i32(
                    gl.get_uniform_location(self.mesh_program, "u_comparison")
                        .as_ref(),
                    i32::from(view.comparison.is_some()),
                );
                if let Some(comparison) = view.comparison {
                    gl.uniform_2_f32(
                        gl.get_uniform_location(self.mesh_program, "u_comparison_scale")
                            .as_ref(),
                        comparison.scale[0],
                        comparison.scale[1],
                    );
                    gl.uniform_2_f32(
                        gl.get_uniform_location(self.mesh_program, "u_comparison_offset")
                            .as_ref(),
                        comparison.offset[0],
                        comparison.offset[1],
                    );
                }
                gl.uniform_1_i32(
                    gl.get_uniform_location(self.mesh_program, "u_perspective")
                        .as_ref(),
                    i32::from(perspective.is_some()),
                );
                if let Some(camera) = perspective {
                    gl.uniform_4_f32(
                        gl.get_uniform_location(self.mesh_program, "u_camera")
                            .as_ref(),
                        camera.distance,
                        camera.focal_x,
                        camera.focal_y,
                        camera.depth_scale,
                    );
                    gl.uniform_1_f32(
                        gl.get_uniform_location(self.mesh_program, "u_depth_offset")
                            .as_ref(),
                        camera.depth_offset,
                    );
                }
                gl.uniform_2_f32(
                    gl.get_uniform_location(self.mesh_program, "u_fit").as_ref(),
                    1.1 * (viewport.height_px as f32 / viewport.width_px.max(1) as f32).min(1.0)
                        * view.scale,
                    1.1 * (viewport.width_px as f32 / viewport.height_px.max(1) as f32).min(1.0)
                        * view.scale,
                );
                if let Some(comparison) = view.comparison {
                    let [left, bottom, clip_width, clip_height] = comparison.clip;
                    let x = (left * width as f32).floor() as i32;
                    let y = (bottom * height as f32).floor() as i32;
                    let right = ((left + clip_width) * width as f32).ceil() as i32;
                    let top = ((bottom + clip_height) * height as f32).ceil() as i32;
                    gl.enable(glow::SCISSOR_TEST);
                    gl.scissor(x, y, (right - x).max(0), (top - y).max(0));
                }
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
uniform vec3 u_axes;
uniform bool u_perspective;
uniform vec4 u_camera;
uniform float u_depth_offset;
uniform bool u_comparison;
uniform vec2 u_comparison_scale;
uniform vec2 u_comparison_offset;
out vec3 v_normal;
void main() {
    float sy = sin(u_angles.x), cy = cos(u_angles.x);
    float sp = sin(u_angles.y), cp = cos(u_angles.y);
    vec3 right = vec3(cy, -sy, 0.0);
    vec3 up = vec3(sp * sy, sp * cy, cp);
    vec3 depth = vec3(cp * sy, cp * cy, -sp);
    vec3 position = a_position * u_axes;
    vec3 normal = normalize(a_normal / u_axes);
    if (u_comparison) {
        gl_Position = vec4(dot(right, position) * u_comparison_scale.x + u_comparison_offset.x,
                           dot(up, position) * u_comparison_scale.y + u_comparison_offset.y,
                           dot(depth, position) * 0.5, 1.0);
    } else if (u_perspective) {
        float distance = u_camera.x + dot(depth, position);
        gl_Position = vec4(dot(right, position) * u_camera.y,
                           dot(up, position) * u_camera.z,
                           distance * u_camera.w + u_depth_offset, distance);
    } else {
        gl_Position = vec4(dot(right, position) * u_fit.x,
                           dot(up, position) * u_fit.y,
                           dot(depth, position) * 0.5, 1.0);
    }
    v_normal = vec3(dot(right, normal), dot(up, normal), dot(depth, normal));
}
"#;
const MESH_FRAGMENT: &str = r#"
precision mediump float;
in vec3 v_normal;
uniform bool u_matcap;
out vec4 out_color;
// A procedural grey sphere matcap: the studio illumination is fixed in view
// space and sampled by the surface normal. Broad soft lights keep facets and
// small changes in curvature readable without a glossy, pinpoint reflection.
vec3 grey_ball(vec2 uv) {
    vec2 xy = uv * 2.0 - 1.0;
    vec3 n = vec3(xy, sqrt(max(0.0, 1.0 - dot(xy, xy))));
    float key = max(dot(n, normalize(vec3(-0.45, 0.65, 0.8))), 0.0);
    float fill = max(dot(n, normalize(vec3(0.75, -0.15, 0.65))), 0.0);
    float softbox = pow(max(dot(n, normalize(vec3(-0.35, 0.55, 1.0))), 0.0), 8.0);
    float rim = pow(1.0 - n.z, 2.0);
    float grey = 0.25 + 0.36 * key + 0.12 * fill + 0.13 * softbox
               - 0.10 * rim - 0.04 * max(-n.y, 0.0);
    return vec3(clamp(grey, 0.14, 0.88));
}
void main() {
    if (u_matcap) {
        // Camera depth points away from the eye. Flip back-facing normals for
        // the existing two-sided viewport, without altering mesh/export data.
        vec3 normal = normalize(vec3(v_normal.xy, -v_normal.z));
        if (normal.z < 0.0) normal = -normal;
        out_color = vec4(grey_ball(normal.xy * 0.5 + 0.5), 1.0);
    } else {
        float light = 0.30 + 0.70 * abs(dot(normalize(v_normal), normalize(vec3(-0.4, 0.6, 1.0))));
        out_color = vec4(vec3(0.965, 0.855, 0.682) * light, 1.0);
    }
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
