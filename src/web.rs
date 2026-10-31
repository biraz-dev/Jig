//! egui adapter for the same local utility model used by the CLI and terminal.
use crate::{
    browser_storage,
    stl_ui::{Snapshot as StlSnapshot, StlPanel},
    timeline_ui::{TimelineAction, TimelinePanel},
    weather_ui::WeatherPanel,
};
use chrono::{Datelike, Local, NaiveDate, TimeZone, Utc};
use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke, TextEdit, Ui};
use jig::{
    calendar,
    model::{State, TaskColor},
};
use std::time::Duration;

const BG: Color32 = Color32::from_rgb(12, 19, 27);
const CREAM: Color32 = Color32::from_rgb(246, 218, 174);
const PINK: Color32 = Color32::from_rgb(234, 133, 173);
const BLUE: Color32 = Color32::from_rgb(144, 161, 202);
const ACCENT: Color32 = Color32::from_rgb(109, 185, 238);
const GREEN: Color32 = Color32::from_rgb(164, 223, 144);
const RULE: Color32 = Color32::from_rgb(49, 65, 83);
const UNDO_STEPS: usize = 20;
const UNDO_BYTES: usize = 64 * 1024 * 1024;

enum UndoEntry {
    Data { state: Box<State>, bytes: usize },
    Viewer(StlSnapshot),
}

pub(crate) fn task_color(color: Option<TaskColor>, complete: bool) -> Color32 {
    let color = match color {
        Some(TaskColor::Rose) => PINK,
        Some(TaskColor::Amber) => Color32::from_rgb(218, 185, 126),
        Some(TaskColor::Sage) => Color32::from_rgb(148, 188, 160),
        Some(TaskColor::Blue) => ACCENT,
        Some(TaskColor::Lilac) => Color32::from_rgb(179, 158, 207),
        None => BLUE,
    };
    if complete {
        color.gamma_multiply(0.6)
    } else {
        color
    }
}

fn task_mark(ui: &Ui, center: egui::Pos2, color: Option<TaskColor>, complete: bool, radius: f32) {
    let color = task_color(color, complete);
    if complete {
        ui.painter()
            .circle_stroke(center, radius, Stroke::new(1.0_f32, color));
    } else {
        ui.painter().circle_filled(center, radius, color);
    }
}

#[derive(Clone, Copy)]
enum Confirm {
    Empty,
    Clear,
}

struct ChecklistRow {
    task: Option<u64>,
    deadline: Option<u64>,
    title: String,
    complete: bool,
    color: Option<TaskColor>,
    position: Option<usize>,
}

pub struct BrowserApp {
    state: State,
    weather: WeatherPanel,
    stl: StlPanel,
    undo_history: Vec<UndoEntry>,
    undo_baseline: State,
    undo_baseline_json: Vec<u8>,
    demo: bool,
    dirty: bool,
    storage_error: bool,
    status: String,
    confirm: Option<Confirm>,
    data_open: bool,
    pending_import: Option<State>,
    focus: usize,
    expanded: bool,
    focus_calculator: bool,
    scroll_selected: bool,
    calculation: String,
    task_text: String,
    task_edit: Option<u64>,
    task_color_edit: Option<u64>,
    confirm_clear_completed: bool,
    task_date_edit: Option<u64>,
    task_date: String,
    task_start: String,
    task_date_timeline: bool,
    task_date_legacy: Option<u64>,
    task_date_error: Option<String>,
    feature_after_date: bool,
    focus_task_date: bool,
    year: i32,
    month: u32,
    selected_date: NaiveDate,
    timeline_view: bool,
    timeline: TimelinePanel,
}

impl BrowserApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = BG;
        style.visuals.window_fill = BG;
        style.visuals.override_text_color = Some(CREAM);
        style.visuals.selection.bg_fill = Color32::from_rgb(40, 72, 101);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(21, 31, 43);
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(0.5_f32, RULE);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(38, 54, 73);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(43, 66, 90);
        style.spacing.item_spacing = egui::vec2(8.0, 7.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::monospace(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::monospace(13.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, FontId::monospace(14.0));
        cc.egui_ctx.set_style(style);
        let (mut state, demo, status, storage_error) = match browser_storage::load() {
            Ok(Some(state)) => (
                state,
                false,
                "Restored your browser-local data.".into(),
                false,
            ),
            Ok(None) => (
                jig::sample::state(Utc::now()),
                true,
                "Sample changes are temporary. Start empty or import a file for your own data."
                    .into(),
                false,
            ),
            Err(error) => (
                jig::sample::state(Utc::now()),
                true,
                format!("Stored data could not be opened and has been preserved: {error}"),
                true,
            ),
        };
        let today = Local::now().date_naive();
        let weather = WeatherPanel::new(state.preferences.weather.clone());
        state.preferences.weather = weather.config().cloned();
        let undo_baseline = state.clone();
        let undo_baseline_json = serde_json::to_vec(&state).unwrap_or_default();
        Self {
            state,
            weather,
            stl: StlPanel::new(cc),
            undo_history: Vec::new(),
            undo_baseline,
            undo_baseline_json,
            demo,
            dirty: false,
            storage_error,
            status,
            confirm: None,
            data_open: false,
            pending_import: None,
            focus: 0,
            expanded: false,
            focus_calculator: false,
            scroll_selected: false,
            calculation: String::new(),
            task_text: String::new(),
            task_edit: None,
            task_color_edit: None,
            confirm_clear_completed: false,
            task_date_edit: None,
            task_date: String::new(),
            task_start: String::new(),
            task_date_timeline: false,
            task_date_legacy: None,
            task_date_error: None,
            feature_after_date: false,
            focus_task_date: false,
            year: today.year(),
            month: today.month(),
            selected_date: today,
            timeline_view: false,
            timeline: TimelinePanel::new(today),
        }
    }

    fn changed(&mut self) {
        // Finish an earlier viewer gesture before recording this data action.
        self.stl.finish_undo_gesture();
        self.collect_viewer_undo();
        if let Ok(json) = serde_json::to_vec(&self.state) {
            if json != self.undo_baseline_json {
                let previous = std::mem::replace(&mut self.undo_baseline, self.state.clone());
                let bytes = self.undo_baseline_json.len();
                self.undo_baseline_json = json;
                self.push_undo(UndoEntry::Data {
                    state: Box::new(previous),
                    bytes,
                });
            }
        }
        self.save_current();
    }

    fn save_current(&mut self) {
        if self.demo {
            self.status =
                "Sample edit applied; demo data is temporary. Export JSON to keep a copy.".into();
        } else {
            self.dirty = true;
            if self.storage_error {
                self.status = "Stored data could not be decoded and has been preserved. Export your current work, then explicitly import a valid file or confirm Start empty before saving.".into();
                return;
            }
            match browser_storage::save(&self.state) {
                Ok(()) => {
                    self.dirty = false;
                    self.storage_error = false;
                    self.status =
                        "Saved in this browser. Export JSON for a portable backup.".into();
                }
                Err(error) => {
                    self.status = format!(
                        "Not saved: {error}. Your changes remain in memory; retry or export JSON."
                    )
                }
            }
        }
    }

    fn push_undo(&mut self, entry: UndoEntry) {
        self.undo_history.push(entry);
        while self.undo_history.len() > UNDO_STEPS || self.undo_bytes() > UNDO_BYTES {
            if self.undo_history.is_empty() {
                break;
            }
            self.undo_history.remove(0);
        }
    }

    fn undo_bytes(&self) -> usize {
        let mut bytes = 0_usize;
        let mut meshes = std::collections::HashSet::new();
        if let Some((identity, size)) = self.stl.mesh_identity() {
            meshes.insert(identity);
            bytes = bytes.saturating_add(size);
        }
        for entry in &self.undo_history {
            match entry {
                UndoEntry::Data { bytes: size, .. } => bytes = bytes.saturating_add(*size),
                UndoEntry::Viewer(snapshot) => {
                    if let Some((identity, size)) = snapshot.mesh_identity() {
                        if meshes.insert(identity) {
                            bytes = bytes.saturating_add(size);
                        }
                    }
                }
            }
        }
        bytes
    }

    fn collect_viewer_undo(&mut self) -> bool {
        let records = self.stl.take_undo_records();
        let changed = !records.is_empty();
        for snapshot in records {
            self.push_undo(UndoEntry::Viewer(snapshot));
        }
        changed
    }

    fn reset_undo(&mut self) {
        self.timeline = TimelinePanel::new(Local::now().date_naive());
        self.undo_history.clear();
        self.stl.clear_undo();
        self.undo_baseline = self.state.clone();
        self.undo_baseline_json = serde_json::to_vec(&self.state).unwrap_or_default();
    }

    fn undo_last(&mut self, ctx: &egui::Context) {
        self.stl.finish_undo_gesture();
        self.collect_viewer_undo();
        let Some(entry) = self.undo_history.pop() else {
            return;
        };
        match entry {
            UndoEntry::Data { state, .. } => {
                self.state = *state;
                if self.weather.config() != self.state.preferences.weather.as_ref() {
                    self.weather
                        .replace_config(self.state.preferences.weather.clone());
                }
                self.reset_editors();
                // Reuse save guards: a stale tab or corrupt raw file stays preserved.
                self.save_current();
            }
            UndoEntry::Viewer(snapshot) => self.stl.restore(snapshot),
        }
        self.undo_baseline = self.state.clone();
        self.undo_baseline_json = serde_json::to_vec(&self.state).unwrap_or_default();
        ctx.request_repaint();
    }

    fn apply(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => self.changed(),
            Err(error) => self.status = error,
        }
    }

    fn reset_editors(&mut self) {
        self.task_edit = None;
        self.task_color_edit = None;
        self.task_text.clear();
        self.confirm_clear_completed = false;
        self.task_date_edit = None;
        self.task_date.clear();
        self.task_start.clear();
        self.task_date_timeline = false;
        self.task_date_legacy = None;
        self.task_date_error = None;
        self.feature_after_date = false;
        self.focus_task_date = false;
        self.calculation.clear();
    }

    fn restore_weather(&mut self) {
        if let Some(config) = self.state.preferences.weather.clone() {
            self.weather.replace_config(Some(config));
        }
        self.state.preferences.weather = self.weather.config().cloned();
    }

    fn replace_personal(&mut self, state: State) {
        self.state = state;
        self.restore_weather();
        self.demo = false;
        // This path is reached only after explicit replacement confirmation.
        self.storage_error = false;
        self.reset_editors();
        self.reset_undo();
        self.save_current();
    }

    fn open_local(&mut self) {
        if self.dirty {
            self.status =
                "Save your changes before opening another dataset. If saving is unavailable, export JSON, then reload to reopen the saved copy."
                    .into();
        } else {
            match browser_storage::load() {
                Ok(state) => {
                    self.state = state.unwrap_or_default();
                    self.restore_weather();
                    self.demo = false;
                    self.storage_error = false;
                    self.reset_editors();
                    self.reset_undo();
                    self.status = "Opened your browser-local data.".into();
                }
                Err(error) => {
                    self.storage_error = true;
                    self.status = format!("Stored data is preserved: {error}");
                }
            }
        }
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        let wide = ui.available_width() >= 900.0;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("JIG")
                    .font(FontId::monospace(29.0))
                    .color(CREAM)
                    .strong(),
            );
            if wide {
                ui.label(RichText::new("You are a small Small Giant Psionicist.").color(BLUE));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.selectable_label(self.data_open, "Data").clicked() {
                    self.data_open = !self.data_open;
                }
                if ui
                    .add_enabled(
                        !self.undo_history.is_empty() || self.stl.has_pending_undo(),
                        egui::Button::new("Undo"),
                    )
                    .on_hover_text("Undo the most recent workbench or STL change (Ctrl-Z)")
                    .clicked()
                {
                    self.undo_last(ui.ctx());
                }
                ui.label(RichText::new("• local").color(GREEN).small());
                ui.label(
                    RichText::new(if self.demo {
                        "sample data"
                    } else {
                        "your data"
                    })
                    .color(if self.demo { PINK } else { BLUE })
                    .small(),
                );
                let date_format = if wide { "%a %d %b %Y" } else { "%d %b %Y" };
                ui.label(
                    RichText::new(Local::now().format(date_format).to_string())
                        .size(if wide { 14.0 } else { 11.0 })
                        .color(BLUE),
                );
            });
        });
        if !wide {
            ui.label(
                RichText::new("You are a small Small Giant Psionicist.")
                    .color(BLUE)
                    .small(),
            );
        }
        if self.data_open {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui.button("Start empty").clicked() {
                    self.confirm = Some(Confirm::Empty);
                }
                if ui.button("Open local data").clicked() {
                    self.open_local();
                }
                if ui.button("Sample mode").clicked() {
                    if self.dirty {
                        self.status = "Save your changes before switching to sample mode. If saving is unavailable, export JSON, then reload before switching.".into();
                    } else {
                        self.state = jig::sample::state(Utc::now());
                        self.restore_weather();
                        self.demo = true;
                        self.reset_editors();
                        self.reset_undo();
                        self.status = "Sample mode. Your saved personal data has not changed.".into();
                    }
                }
                if ui.button("Import JSON").clicked() {
                    if let Err(error) = browser_storage::request_import() {
                        self.status = error;
                    }
                }
                if ui.button("Export JSON").clicked() {
                    self.status = match browser_storage::download(&self.state) {
                        Ok(()) => "JSON download prepared on this device.".into(),
                        Err(error) => error,
                    };
                }
                if ui.button("Clear saved data").clicked() {
                    self.confirm = Some(Confirm::Clear);
                }
            });
        }
        if self.dirty && ui.button("Retry save").clicked() {
            self.changed();
        }
        if let Some(confirm) = self.confirm {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(match confirm { Confirm::Empty => "Replace this browser's saved data with an empty workbench? Export first if needed.", Confirm::Clear => "Delete this browser's saved Jig data? Export a backup first if needed." }).color(PINK));
                if ui.button("Confirm").clicked() {
                    self.confirm = None;
                    match confirm {
                        Confirm::Empty => self.replace_personal(State::default()),
                        Confirm::Clear => match browser_storage::clear() {
                            Ok(()) => { self.weather.disable(); self.state = jig::sample::state(Utc::now()); self.demo = true; self.dirty = false; self.storage_error = false; self.reset_editors(); self.reset_undo(); self.status = "Browser-local data cleared. Displaying temporary sample data.".into(); }
                            Err(error) => self.status = error,
                        },
                    }
                }
                if ui.button("Cancel").clicked() { self.confirm = None; }
            });
        }
        if let Some(import) = &self.pending_import {
            let summary = format!("Import {} tasks, {} deadlines and {} calculations? This replaces the browser's saved dataset.", import.tasks.len(), import.deadlines.len(), import.history.len());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(summary).color(PINK));
                if ui.button("Confirm import").clicked() {
                    if let Some(import) = self.pending_import.take() {
                        self.replace_personal(import);
                    }
                }
                if ui.button("Cancel import").clicked() {
                    self.pending_import = None;
                }
            });
        }
    }

    fn tool(&mut self, ui: &mut Ui, index: usize, height: f32) {
        let selected = self.focus == index;
        let frame = egui::Frame::default()
            .fill(if selected {
                Color32::from_rgb(15, 24, 35)
            } else {
                BG
            })
            .inner_margin(egui::Margin::same(9))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height((height - 18.0).max(45.0));
                match index {
                    0 => self.deadlines(ui),
                    1 => {
                        egui::ScrollArea::vertical()
                            .id_salt("checklist")
                            .max_height((height - 18.0).max(180.0))
                            .show(ui, |ui| self.tasks(ui));
                    }
                    2 => self.calendar(ui),
                    3 => self.workbench(ui, (height - 18.0).max(120.0)),
                    _ => {
                        let mut settings = self.state.preferences.clay.clone().unwrap_or_default();
                        if self.stl.ui(ui, (height - 18.0).max(180.0), &mut settings) {
                            self.state.preferences.clay = Some(settings);
                            self.changed();
                        }
                        if self.stl.focused {
                            self.focus = 4;
                        }
                        if self.collect_viewer_undo() {
                            ui.ctx().request_repaint();
                        }
                    }
                }
            });
        if self.scroll_selected && self.focus == index {
            // An expanded viewer can be slightly taller than the visible area.
            // Align its top so selecting it never scrolls the file controls away.
            let align = if self.expanded && (index == 2 || index == 4) {
                egui::Align::Min
            } else {
                egui::Align::Center
            };
            frame.response.scroll_to_me(Some(align));
            self.scroll_selected = false;
        }
        let visible = ui.clip_rect();
        if ui.input(|input| {
            input.pointer.any_pressed()
                && input.pointer.interact_pos().is_some_and(|position| {
                    frame.response.rect.contains(position) && visible.contains(position)
                })
        }) && self.focus != index
        {
            self.focus = index;
            ui.ctx().request_repaint();
        }
        let color = if self.focus == index { PINK } else { RULE };
        if selected != (self.focus == index) {
            ui.ctx().request_repaint();
        }
        ui.painter().hline(
            frame.response.rect.x_range(),
            frame.response.rect.top(),
            Stroke::new(1.0_f32, color),
        );
    }

    fn workbench(&mut self, ui: &mut Ui, height: f32) {
        let width = ui.available_width();
        if width >= 820.0 {
            let left = (width - 18.0) * 0.62;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(left, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| self.workbench_input(ui),
                );
                ui.add_space(10.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(width - left - 18.0, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("math-history")
                            .max_height(height)
                            .show(ui, |ui| self.workbench_history(ui));
                    },
                );
            });
        } else {
            let top = ui.cursor().top();
            self.workbench_input(ui);
            ui.separator();
            let used = ui.cursor().top() - top;
            egui::ScrollArea::vertical()
                .id_salt("math-history")
                .max_height((height - used).max(70.0))
                .show(ui, |ui| self.workbench_history(ui));
        }
    }

    fn workbench_input(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let id = egui::Id::new("calculator-input");
            if self.focus_calculator {
                ui.memory_mut(|memory| memory.request_focus(id));
            }
            let response = ui.add(
                TextEdit::singleline(&mut self.calculation)
                    .id(id)
                    .return_key(None)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("125 * 1.15   or   72 in mm")
                    .desired_width((ui.available_width() - 65.0).max(80.0)),
            );
            if response.has_focus() {
                self.focus = 3;
            }
            if self.focus_calculator {
                response.scroll_to_me(None);
                self.focus_calculator = !response.has_focus();
                ui.ctx().request_repaint();
            }
            // Handle Enter while this field owns focus, including first-frame
            // focus plus typing; TextEdit's lost_focus only compares prior frames.
            let enter = response.has_focus()
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            if ui.button("Enter").clicked() || enter {
                match self.state.calculate(&self.calculation) {
                    Ok(_) => {
                        self.calculation.clear();
                        self.changed();
                    }
                    Err(error) => self.status = error,
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if let Some(last) = self.state.history.last() {
                ui.label(
                    RichText::new(format!(
                        "= {}",
                        last.formatted(self.state.preferences.decimals)
                    ))
                    .font(FontId::monospace(28.0))
                    .color(GREEN),
                );
                if ui
                    .small_button("Copy")
                    .on_hover_text("Copy the full-precision result")
                    .clicked()
                {
                    ui.ctx().copy_text(last.reusable_input());
                }
            }
            ui.label("Decimals");
            let response =
                ui.add(egui::DragValue::new(&mut self.state.preferences.decimals).range(0..=10));
            if response.has_focus() {
                self.focus = 3;
            }
            if response.changed() {
                self.changed();
            }
        });
    }

    fn workbench_history(&mut self, ui: &mut Ui) {
        let mut reused_history = false;
        for history in self.state.history.iter().rev() {
            ui.horizontal_wrapped(|ui| {
                if ui.link(&history.expression).clicked() {
                    self.calculation = history.expression.clone();
                    reused_history = true;
                    self.focus_calculator = true;
                    ui.ctx().request_repaint();
                }
                if ui
                    .button(format!(
                        "= {}",
                        history.formatted(self.state.preferences.decimals)
                    ))
                    .clicked()
                {
                    self.calculation = history.reusable_input();
                    reused_history = true;
                    self.focus_calculator = true;
                    ui.ctx().request_repaint();
                }
            });
        }
        if reused_history {
            let id = egui::Id::new("calculator-input");
            if let Some(mut state) = TextEdit::load_state(ui.ctx(), id) {
                // Start typing after the reused value rather than at an old cursor.
                state.cursor.set_char_range(None);
                TextEdit::store_state(ui.ctx(), id, state);
            }
        }
    }

    fn row_task(&mut self, row: &ChecklistRow) -> Result<u64, String> {
        match row.task {
            Some(id) => Ok(id),
            None => self
                .state
                .ensure_task_for_deadline(row.deadline.ok_or("Missing task")?),
        }
    }

    fn begin_task_date(&mut self, id: u64, feature_after: bool) {
        self.task_start = self
            .state
            .tasks
            .iter()
            .find(|task| task.id == id)
            .and_then(|task| task.start)
            .map_or_else(String::new, |start| {
                start.with_timezone(&Local).format("%Y-%m-%d").to_string()
            });
        self.task_date_timeline = false;
        self.task_date_legacy = None;
        self.task_date_error = None;
        self.task_date = self.state.task_deadline(id).map_or_else(
            || self.selected_date.to_string(),
            |deadline| {
                deadline
                    .at
                    .with_timezone(&Local)
                    .format("%Y-%m-%d")
                    .to_string()
            },
        );
        self.task_date_edit = Some(id);
        self.feature_after_date = feature_after;
        self.focus_task_date = true;
    }

    fn tasks(&mut self, ui: &mut Ui) {
        let mut rows: Vec<_> = self
            .state
            .tasks
            .iter()
            .enumerate()
            .map(|(index, task)| ChecklistRow {
                task: Some(task.id),
                deadline: task.deadline,
                title: task.text.clone(),
                complete: task.complete,
                color: task.color,
                position: Some(index),
            })
            .collect();
        // Older files keep their independent deadlines until a row is acted on.
        for deadline in &self.state.deadlines {
            if !self
                .state
                .tasks
                .iter()
                .any(|task| task.deadline == Some(deadline.id))
            {
                rows.push(ChecklistRow {
                    task: None,
                    deadline: Some(deadline.id),
                    title: deadline.title.clone(),
                    complete: false,
                    color: None,
                    position: None,
                });
            }
        }
        for row in rows {
            ui.push_id(("checklist-row", row.task, row.deadline), |ui| {
                let mut color_control = None;
                ui.horizontal_wrapped(|ui| {
                    let featured = row.deadline.is_some() && row.deadline == self.state.featured;
                    if ui
                        .add_enabled(
                            !row.complete,
                            egui::Button::new(
                                RichText::new(if featured { "★" } else { "☆" })
                                    .color(if featured { PINK } else { BLUE }),
                            )
                            .small(),
                        )
                        .on_hover_text("Star a dated task for the countdown")
                        .clicked()
                    {
                        if featured {
                            self.state.featured = None;
                            self.changed();
                        } else {
                            match self.row_task(&row) {
                                Ok(id) if self.state.task_deadline(id).is_some() => {
                                    let result = self.state.feature_task(id);
                                    if result.is_ok() {
                                        self.reveal_task_date(id);
                                    }
                                    self.apply(result);
                                }
                                Ok(id) => self.begin_task_date(id, true),
                                Err(error) => self.status = error,
                            }
                        }
                    }
                    let mut complete = row.complete;
                    if ui.checkbox(&mut complete, "").changed() {
                        let result = self.row_task(&row).and_then(|id| {
                            self.state.toggle_task(id)?;
                            if complete && self.task_date_edit == Some(id) {
                                self.feature_after_date = false;
                            }
                            Ok(())
                        });
                        self.apply(result);
                    }
                    if let Some(deadline) = row.deadline {
                        let response = ui.add(
                            egui::Button::new(" ")
                                .selected(self.task_color_edit == Some(deadline))
                                .min_size(egui::vec2(30.0, 30.0)),
                        );
                        color_control = Some(response.id);
                        let label =
                            format!("Color: {}", row.color.map_or("None", TaskColor::label));
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                ui.is_enabled(),
                                &label,
                            )
                        });
                        if response.has_focus() {
                            self.focus = 1;
                        }
                        task_mark(ui, response.rect.center(), row.color, row.complete, 4.0);
                        if response.on_hover_text(label).clicked() {
                            self.task_color_edit = if self.task_color_edit == Some(deadline) {
                                None
                            } else {
                                Some(deadline)
                            };
                        }
                    }
                    let text =
                        RichText::new(&row.title).color(if row.complete { BLUE } else { CREAM });
                    ui.label(if row.complete {
                        text.strikethrough()
                    } else {
                        text
                    });
                    let date = row
                        .deadline
                        .and_then(|id| {
                            self.state
                                .deadlines
                                .iter()
                                .find(|deadline| deadline.id == id)
                        })
                        .map(|deadline| {
                            deadline
                                .at
                                .with_timezone(&Local)
                                .format("%d %b · %H:%M")
                                .to_string()
                        });
                    if ui.small_button(date.as_deref().unwrap_or("date")).clicked() {
                        match self.row_task(&row) {
                            Ok(id) => {
                                self.begin_task_date(id, false);
                                if row.task.is_none() {
                                    self.changed();
                                }
                            }
                            Err(error) => self.status = error,
                        }
                    }
                    if ui.small_button("edit").clicked() {
                        match self.row_task(&row) {
                            Ok(id) => {
                                self.task_edit = Some(id);
                                self.task_text = row.title.clone();
                                if row.task.is_none() {
                                    self.changed();
                                }
                            }
                            Err(error) => self.status = error,
                        }
                    }
                    if ui
                        .add_enabled(
                            row.position.is_some_and(|position| position > 0),
                            egui::Button::new("↑").small(),
                        )
                        .clicked()
                    {
                        if let (Some(id), Some(position)) = (row.task, row.position) {
                            let result = self.state.move_task(id, position);
                            self.apply(result);
                        }
                    }
                    if ui
                        .add_enabled(
                            row.position
                                .is_some_and(|position| position + 1 < self.state.tasks.len()),
                            egui::Button::new("↓").small(),
                        )
                        .clicked()
                    {
                        if let (Some(id), Some(position)) = (row.task, row.position) {
                            let result = self.state.move_task(id, position + 2);
                            self.apply(result);
                        }
                    }
                    if ui.small_button("del").clicked() {
                        let result = match row.task {
                            Some(id) => self.state.remove_task(id),
                            None => self.state.remove_deadline(row.deadline.unwrap_or(0)),
                        };
                        self.apply(result);
                        if row.task.is_some() && self.task_edit == row.task {
                            self.task_edit = None;
                            self.task_text.clear();
                        }
                        if row.task.is_some() && self.task_date_edit == row.task {
                            self.task_date_edit = None;
                        }
                        if self.task_color_edit == row.deadline {
                            self.task_color_edit = None;
                        }
                    }
                });
                if row.deadline.is_some() && self.task_color_edit == row.deadline {
                    ui.horizontal_wrapped(|ui| {
                        for color in std::iter::once(None).chain(TaskColor::ALL.map(Some)) {
                            let text = RichText::new(color.map_or("None", TaskColor::label))
                                .color(task_color(color, false));
                            if ui
                                .add(
                                    egui::Button::new(text)
                                        .selected(row.color == color)
                                        .min_size(egui::vec2(0.0, 30.0)),
                                )
                                .clicked()
                            {
                                let result = self
                                    .row_task(&row)
                                    .and_then(|id| self.state.set_task_color(id, color));
                                if result.is_ok() {
                                    self.task_color_edit = None;
                                    if let Some(id) = color_control {
                                        ui.memory_mut(|memory| memory.request_focus(id));
                                    }
                                    ui.ctx().request_repaint();
                                }
                                self.apply(result);
                            }
                        }
                    });
                }
                let row_id = row.task.or_else(|| {
                    self.state
                        .tasks
                        .iter()
                        .find(|task| task.deadline == row.deadline)
                        .map(|task| task.id)
                });
                if let Some(id) =
                    row_id.filter(|id| self.task_date_edit == Some(*id) && !self.task_date_timeline)
                {
                    self.task_date_editor(ui, id);
                }
            });
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let id = egui::Id::new("task-input");
            let response = ui.add(
                TextEdit::singleline(&mut self.task_text)
                    .id(id)
                    .return_key(None)
                    .hint_text("Add new task")
                    .desired_width((ui.available_width() - 55.0).max(80.0)),
            );
            if response.has_focus() {
                self.focus = 1;
            }
            let enter = response.has_focus()
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            if ui
                .button(if self.task_edit.is_some() {
                    "Save"
                } else {
                    "+"
                })
                .clicked()
                || enter
            {
                let result = if let Some(id) = self.task_edit {
                    self.state.edit_task(id, &self.task_text)
                } else {
                    self.state.add_task(&self.task_text).map(|_| ())
                };
                if result.is_ok() {
                    self.task_text.clear();
                    self.task_edit = None;
                }
                self.apply(result);
            }
        });
        if self.task_edit.is_some() && ui.small_button("Cancel edit").clicked() {
            self.task_edit = None;
            self.task_text.clear();
        }
        let completed = self.state.tasks.iter().filter(|task| task.complete).count();
        if completed > 0 {
            ui.add_space(4.0);
            if self.confirm_clear_completed {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("Delete {completed} completed tasks?"));
                    if ui.small_button("Confirm").clicked() {
                        self.state.remove_completed_tasks();
                        if self
                            .task_edit
                            .is_some_and(|id| !self.state.tasks.iter().any(|task| task.id == id))
                        {
                            self.task_edit = None;
                            self.task_text.clear();
                        }
                        if self
                            .task_date_edit
                            .is_some_and(|id| !self.state.tasks.iter().any(|task| task.id == id))
                        {
                            self.task_date_edit = None;
                        }
                        if self.task_color_edit.is_some_and(|id| {
                            !self
                                .state
                                .deadlines
                                .iter()
                                .any(|deadline| deadline.id == id)
                        }) {
                            self.task_color_edit = None;
                        }
                        self.confirm_clear_completed = false;
                        self.changed();
                    }
                    if ui.small_button("Cancel").clicked() {
                        self.confirm_clear_completed = false;
                    }
                });
            } else if ui.small_button("Delete all completed tasks").clicked() {
                self.confirm_clear_completed = true;
            }
        } else {
            self.confirm_clear_completed = false;
        }
    }

    fn close_task_dates(&mut self) {
        self.task_date_edit = None;
        self.task_date_legacy = None;
        self.task_date_timeline = false;
        self.task_date_error = None;
        self.feature_after_date = false;
        self.focus_task_date = false;
    }

    fn save_task_dates(&mut self, id: u64) -> Result<u64, String> {
        let input = self.task_date.trim();
        let due = if input.len() == 10 {
            let date = NaiveDate::parse_from_str(input, "%Y-%m-%d")
                .map_err(|_| "Due date must be YYYY-MM-DD")?;
            let existing = match self.task_date_legacy {
                Some(deadline) => self.state.deadlines.iter().find(|item| item.id == deadline),
                None => self.state.task_deadline(id),
            };
            if let Some(existing) =
                existing.filter(|deadline| deadline.at.with_timezone(&Local).date_naive() == date)
            {
                // A date-only edit must preserve an existing countdown's exact
                // instant, including offset changes and repeated DST hours.
                existing.at.to_rfc3339()
            } else {
                let time = existing.map_or_else(
                    || chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                    |deadline| deadline.at.with_timezone(&Local).time(),
                );
                Local
                    .from_local_datetime(&date.and_time(time))
                    .earliest()
                    .ok_or("That deadline time does not exist locally; enter a timestamp with a timezone")?
                    .with_timezone(&Utc)
                    .to_rfc3339()
            }
        } else {
            input.to_owned()
        };
        let start = if self.task_start.trim().is_empty() {
            None
        } else {
            let date = NaiveDate::parse_from_str(self.task_start.trim(), "%Y-%m-%d")
                .map_err(|_| "Start date must be YYYY-MM-DD")?;
            let existing = self
                .state
                .tasks
                .iter()
                .find(|task| task.id == id)
                .and_then(|task| task.start)
                .filter(|start| start.with_timezone(&Local).date_naive() == date);
            if let Some(existing) = existing {
                Some(existing.to_rfc3339())
            } else {
                let midnight = date.and_hms_opt(0, 0, 0).ok_or("Invalid start date")?;
                let start = Local
                    .from_local_datetime(&midnight)
                    .earliest()
                    .ok_or("This start date does not exist in your local timezone")?;
                Some(start.with_timezone(&Utc).to_rfc3339())
            }
        };
        // Resolve a legacy deadline only when saving. Viewing or cancelling an
        // editor must not migrate a record or consume an Undo step.
        let mut next = self.state.clone();
        let task = match self.task_date_legacy {
            Some(deadline) => next.ensure_task_for_deadline(deadline)?,
            None => id,
        };
        next.set_task_schedule(task, start.as_deref(), Some(&due))?;
        if self.feature_after_date {
            next.feature_task(task)?;
        }
        self.state = next;
        Ok(task)
    }

    fn task_date_editor(&mut self, ui: &mut Ui, id: u64) {
        ui.indent(("task-date-editor", id), |ui| {
            let owner = if self.task_date_timeline { 2 } else { 1 };
            let field = egui::Id::new(("task-date-input", id));
            ui.label(RichText::new("Due").color(BLUE).size(11.0));
            if self.focus_task_date {
                ui.memory_mut(|memory| memory.request_focus(field));
            }
            let response = ui.add(
                TextEdit::singleline(&mut self.task_date)
                    .id(field)
                    .return_key(None)
                    .hint_text("YYYY-MM-DD")
                    .desired_width(ui.available_width()),
            ).on_hover_text("Enter a date, or an RFC3339 timestamp with a timezone. Date edits retain the existing deadline time; new deadlines use 09:00 local time.");
            if response.has_focus() {
                self.focus = owner;
            }
            if self.focus_task_date {
                response.scroll_to_me(None);
                self.focus_task_date = !response.has_focus();
                ui.ctx().request_repaint();
            }
            let mut enter = response.has_focus()
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            ui.label(RichText::new("Start · optional").color(BLUE).size(11.0));
            let start = ui.add(
                TextEdit::singleline(&mut self.task_start)
                    .id(egui::Id::new(("task-start-input", id)))
                    .return_key(None)
                    .hint_text("YYYY-MM-DD")
                    .desired_width(ui.available_width()),
            );
            if start.has_focus() {
                self.focus = owner;
                if start.gained_focus() {
                    start.scroll_to_me(None);
                    ui.ctx().request_repaint();
                }
                enter |= ui.input_mut(|input| {
                    input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                });
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(if self.feature_after_date {
                        "Set date & star"
                    } else {
                        "Save date"
                    })
                    .clicked()
                    || enter
                {
                    let result = self.save_task_dates(id);
                    match result {
                        Ok(task) => {
                            if self.feature_after_date {
                                self.reveal_task_date(task);
                            }
                            self.close_task_dates();
                            self.focus = owner;
                            self.changed();
                        }
                        Err(error) => {
                            self.status = error.clone();
                            self.task_date_error = Some(error);
                        }
                    }
                }
                let has_date = self.task_date_legacy.is_some()
                    || self.state.task_deadline(id).is_some();
                if has_date && ui.small_button("Remove date").clicked() {
                    let result = match self.task_date_legacy {
                        Some(deadline) => self.state.remove_deadline(deadline),
                        None => self.state.set_task_deadline(id, None),
                    };
                    if result.is_ok() {
                        self.close_task_dates();
                    }
                    self.apply(result);
                }
                if ui.small_button("Cancel").clicked() {
                    self.close_task_dates();
                }
            });
            if let Some(error) = &self.task_date_error {
                ui.label(RichText::new(error).color(PINK).size(11.0));
            }
        });
    }

    fn deadlines(&mut self, ui: &mut Ui) {
        let featured = self.state.deadlines.iter().find(|deadline| {
            Some(deadline.id) == self.state.featured && self.state.is_deadline_active(deadline.id)
        });
        let Some(deadline) = featured else {
            ui.label(RichText::new("No starred deadline").color(BLUE));
            return;
        };
        let title = deadline.title.clone();
        let date = deadline
            .at
            .with_timezone(&Local)
            .format("%a %d %b · %H:%M %:z")
            .to_string();
        let seconds = (deadline.at - Utc::now()).num_seconds();
        let n = seconds.unsigned_abs();
        let values = [n / 86400, n / 3600 % 24, n / 60 % 60, n % 60];
        let wide = ui.available_width() >= 700.0;
        let title_width = (ui.available_width() * 0.34).min(400.0);
        let draw_title = |ui: &mut Ui| {
            ui.label(RichText::new(&title).color(CREAM).strong().size(18.0));
            ui.label(RichText::new(&date).color(BLUE));
            if seconds < 0 {
                ui.label(RichText::new("OVERDUE").color(PINK));
            }
        };
        let draw_countdown = |ui: &mut Ui| {
            let size = (ui.available_width() / 10.5).clamp(24.0, 54.0);
            ui.columns(4, |columns| {
                for (index, column) in columns.iter_mut().enumerate() {
                    column.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(format!("{:02}", values[index]))
                                .font(FontId::new(size, FontFamily::Monospace))
                                .color(CREAM),
                        );
                        ui.label(
                            RichText::new(["DAYS", "HOURS", "MINUTES", "SECONDS"][index])
                                .small()
                                .color(ACCENT),
                        );
                    });
                }
            });
        };
        let unstar = if wide {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(title_width, 78.0),
                    egui::Layout::top_down(egui::Align::Min),
                    draw_title,
                );
                ui.allocate_ui_with_layout(
                    egui::vec2((ui.available_width() - 65.0).max(260.0), 78.0),
                    egui::Layout::top_down(egui::Align::Min),
                    draw_countdown,
                );
                ui.small_button("unstar").clicked()
            })
            .inner
        } else {
            draw_title(ui);
            draw_countdown(ui);
            ui.small_button("unstar").clicked()
        };
        if unstar {
            self.state.featured = None;
            self.changed();
        }
    }

    fn reveal_task_date(&mut self, id: u64) {
        if let Some(deadline) = self.state.task_deadline(id) {
            let date = deadline.at.with_timezone(&Local).date_naive();
            self.selected_date = date;
            self.year = date.year();
            self.month = date.month();
            self.timeline.reveal(date);
        }
    }

    fn calendar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui
                .selectable_label(!self.timeline_view, "Calendar")
                .clicked()
            {
                self.timeline_view = false;
                if self.task_date_timeline {
                    self.close_task_dates();
                }
            }
            if ui
                .selectable_label(self.timeline_view, "Timeline")
                .clicked()
            {
                if !self.timeline_view {
                    self.timeline.reveal(self.selected_date);
                }
                self.timeline_view = true;
            }
        });
        if !self.timeline_view {
            self.calendar_month(ui);
            return;
        }
        let today = Local::now().date_naive();
        let action = self.timeline.ui(ui, &self.state, today);
        if self.timeline.focused {
            self.focus = 2;
        }
        match action {
            Some(TimelineAction::EditTask(id)) => {
                self.begin_task_date(id, false);
                self.task_date_timeline = true;
                if let Some(deadline) = self.state.task_deadline(id) {
                    self.selected_date = deadline.at.with_timezone(&Local).date_naive();
                    self.year = self.selected_date.year();
                    self.month = self.selected_date.month();
                }
            }
            Some(TimelineAction::EditDeadline(id)) => {
                if let Some(deadline) = self.state.deadlines.iter().find(|item| item.id == id) {
                    self.task_date = deadline
                        .at
                        .with_timezone(&Local)
                        .format("%Y-%m-%d")
                        .to_string();
                    self.selected_date = deadline.at.with_timezone(&Local).date_naive();
                    self.year = self.selected_date.year();
                    self.month = self.selected_date.month();
                    self.task_start.clear();
                    self.task_date_edit = None;
                    self.task_date_legacy = Some(id);
                    self.task_date_timeline = true;
                    self.task_date_error = None;
                    self.feature_after_date = false;
                    self.focus_task_date = true;
                }
            }
            None => {}
        }
        if self.task_date_timeline {
            let target = self.task_date_edit.or(self.task_date_legacy);
            if let Some(id) = target {
                ui.add_space(6.0);
                self.task_date_editor(ui, id);
            }
        }
        ui.add_space(6.0);
        if self.weather.controls(ui) {
            self.state.preferences.weather = self.weather.config().cloned();
            if !self.demo {
                self.changed();
            }
        }
        self.weather.selected_day(ui, self.selected_date);
    }

    fn calendar_month(&mut self, ui: &mut Ui) {
        ui.spacing_mut().button_padding.x = 3.0;
        ui.horizontal(|ui| {
            if ui.button("‹").clicked() {
                self.shift_month(-1);
            }
            let month = NaiveDate::from_ymd_opt(self.year, self.month, 1).map_or_else(
                || format!("{} / {}", self.year, self.month),
                |date| date.format("%B %Y").to_string(),
            );
            ui.label(RichText::new(month).color(CREAM));
            if ui.button("›").clicked() {
                self.shift_month(1);
            }
            if ui.small_button("Today").clicked() {
                let today = Local::now().date_naive();
                self.year = today.year();
                self.month = today.month();
                self.selected_date = today;
            }
        });
        let today = Local::now().date_naive();
        let size = ((ui.available_width() - 18.0) / 7.0).max(20.0);
        if let Ok(cells) = calendar::month(self.year, self.month) {
            egui::Grid::new("month-grid")
                .num_columns(7)
                .spacing([3.0, 3.0])
                .show(ui, |ui| {
                    for day in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
                        ui.allocate_ui_with_layout(
                            egui::vec2(size, 18.0),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| {
                                ui.label(RichText::new(day).small().color(BLUE));
                            },
                        );
                    }
                    ui.end_row();
                    let count = cells
                        .iter()
                        .rposition(Option::is_some)
                        .map_or(0, |index| (index / 7 + 1) * 7);
                    for (index, day) in cells[..count].iter().enumerate() {
                        if let Some(day) = day {
                            let mut tasks = self.state.tasks_on_date(*day, &Local);
                            tasks.sort_by_key(|task| task.complete);
                            let color = if *day == today { ACCENT } else { CREAM };
                            let mut button = egui::Button::new(
                                RichText::new(day.day().to_string()).size(14.0).color(color),
                            )
                            .fill(BG)
                            .stroke(Stroke::NONE);
                            if *day == today {
                                button = button.fill(Color32::from_rgb(28, 49, 66));
                            }
                            if *day == self.selected_date {
                                button = button.stroke(Stroke::new(1.0_f32, BLUE));
                            }
                            let response = ui
                                .push_id(("calendar-date", *day), |ui| {
                                    ui.add_sized([size, 34.0], button)
                                })
                                .inner;
                            let label = format!("{}, {} tasks due", day, tasks.len());
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    ui.is_enabled(),
                                    &label,
                                )
                            });
                            // Show distinct colors before repeated marks on busy dates.
                            // The list below remains the complete, named view.
                            let marks = if tasks.len() > 5 { 4 } else { tasks.len() };
                            let mut marked = Vec::with_capacity(marks);
                            for (index, task) in tasks.iter().enumerate() {
                                if !marked
                                    .iter()
                                    .any(|shown: &usize| tasks[*shown].color == task.color)
                                {
                                    marked.push(index);
                                }
                                if marked.len() == marks {
                                    break;
                                }
                            }
                            for index in 0..tasks.len() {
                                if marked.len() == marks {
                                    break;
                                }
                                if !marked.contains(&index) {
                                    marked.push(index);
                                }
                            }
                            let overflow = tasks.len() - marks;
                            let width = marks as f32 * 5.0 + if overflow > 0 { 16.0 } else { 0.0 };
                            let start = response.rect.center().x - width * 0.5 + 2.5;
                            for (index, task) in marked
                                .iter()
                                .take(marks)
                                .map(|index| &tasks[*index])
                                .enumerate()
                            {
                                task_mark(
                                    ui,
                                    egui::pos2(
                                        start + index as f32 * 5.0,
                                        response.rect.bottom() - 4.0,
                                    ),
                                    task.color,
                                    task.complete,
                                    1.7,
                                );
                            }
                            if overflow > 0 {
                                ui.painter().text(
                                    egui::pos2(
                                        start + marks as f32 * 5.0,
                                        response.rect.bottom() - 4.0,
                                    ),
                                    egui::Align2::LEFT_CENTER,
                                    format!("+{overflow}"),
                                    FontId::monospace(9.0),
                                    BLUE,
                                );
                            }
                            let mut tooltip = tasks
                                .iter()
                                .map(|task| {
                                    format!(
                                        "{} · {}",
                                        if task.complete { "Done" } else { "Open" },
                                        task.title
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            if let Some(forecast) = self.weather.day_tooltip(*day) {
                                if !tooltip.is_empty() {
                                    tooltip.push('\n');
                                }
                                tooltip.push_str(&forecast);
                            }
                            if response.has_focus() {
                                self.focus = 2;
                                ui.painter().hline(
                                    response.rect.x_range(),
                                    response.rect.bottom(),
                                    Stroke::new(1.0_f32, ACCENT),
                                );
                            }
                            let clicked = response.clicked();
                            if !tooltip.is_empty() {
                                response.on_hover_text(tooltip);
                            }
                            if clicked {
                                self.selected_date = *day;
                            }
                        } else {
                            ui.allocate_space(egui::vec2(size, 34.0));
                        }
                        if index % 7 == 6 {
                            ui.end_row();
                        }
                    }
                });
        }
        ui.add_space(6.0);
        self.calendar_tasks(ui);
        ui.add_space(6.0);
        if self.weather.controls(ui) {
            self.state.preferences.weather = self.weather.config().cloned();
            if !self.demo {
                self.changed();
            }
        }
        self.weather.selected_day(ui, self.selected_date);
    }

    fn calendar_tasks(&mut self, ui: &mut Ui) {
        let tasks = self.state.tasks_on_date(self.selected_date, &Local);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(self.selected_date.format("%a %d %b").to_string())
                    .color(BLUE)
                    .size(12.0),
            );
            if !tasks.is_empty() {
                ui.label(
                    RichText::new(format!("{} due", tasks.len()))
                        .color(BLUE)
                        .size(12.0),
                );
            }
        });
        if tasks.is_empty() {
            ui.label(RichText::new("No tasks due").color(BLUE).size(12.0));
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt(("calendar-due-tasks", self.selected_date))
            .max_height(120.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for task in tasks {
                    ui.push_id(("calendar-task", task.deadline_id), |ui| {
                        let row = ui.horizontal_top(|ui| {
                            let (rect, _) = ui
                                .allocate_exact_size(egui::vec2(10.0, 18.0), egui::Sense::hover());
                            task_mark(ui, rect.center(), task.color, task.complete, 3.0);
                            ui.label(
                                RichText::new(if task.complete { "Done" } else { "Open" })
                                    .color(BLUE)
                                    .size(11.0),
                            );
                            let text = RichText::new(task.title)
                                .color(if task.complete { BLUE } else { CREAM })
                                .size(12.0);
                            ui.add(
                                egui::Label::new(if task.complete {
                                    text.strikethrough()
                                } else {
                                    text
                                })
                                .wrap(),
                            );
                        });
                        let response = ui.interact(
                            row.response.rect,
                            ui.id().with("due-task"),
                            egui::Sense::focusable_noninteractive(),
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Label,
                                ui.is_enabled(),
                                format!(
                                    "{}: {}",
                                    if task.complete { "Done" } else { "Open" },
                                    task.title
                                ),
                            )
                        });
                        if response.has_focus() {
                            self.focus = 2;
                            response.scroll_to_me(None);
                            ui.painter().hline(
                                response.rect.x_range(),
                                response.rect.bottom(),
                                Stroke::new(0.5_f32, BLUE),
                            );
                        }
                    });
                }
            });
    }

    fn shift_month(&mut self, delta: i32) {
        match calendar::shift(self.year, self.month, delta) {
            Ok((year, month)) => {
                self.year = year;
                self.month = month;
            }
            Err(error) => self.status = error,
        }
    }
}

impl eframe::App for BrowserApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stl.destroy();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !ctx.style().visuals.dark_mode {
            ctx.set_theme(egui::ThemePreference::Dark);
        }
        ctx.request_repaint_after(Duration::from_secs(1));
        self.weather.update();
        if self.stl.update() {
            self.focus = 4;
            self.focus_calculator = false;
            self.focus_task_date = false;
            self.scroll_selected = true;
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
            ctx.request_repaint();
        }
        if self.collect_viewer_undo() {
            ctx.request_repaint();
        }
        if ctx.input(|input| input.pointer.any_pressed()) {
            self.focus_calculator = false;
            self.focus_task_date = false;
        }
        match browser_storage::take_import() {
            Ok(Some(state)) => self.pending_import = Some(state),
            Ok(None) => {}
            Err(error) => {
                self.status = format!("Import rejected; existing data is unchanged: {error}")
            }
        }
        let shortcut = egui::Modifiers {
            ctrl: true,
            alt: true,
            ..Default::default()
        };
        if ctx.input_mut(|input| input.consume_key(shortcut, egui::Key::N)) {
            self.confirm = Some(Confirm::Empty);
        }
        if ctx.input_mut(|input| input.consume_key(shortcut, egui::Key::O)) {
            self.open_local();
        }
        if ctx.input_mut(|input| input.consume_key(shortcut, egui::Key::Num4)) {
            self.focus = 3;
            self.focus_calculator = true;
            ctx.request_repaint();
        }
        if ctx.input_mut(|input| input.consume_key(shortcut, egui::Key::Num5)) {
            self.focus = 4;
            self.focus_calculator = false;
            self.focus_task_date = false;
            self.scroll_selected = true;
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
            self.stl.request_open_focus();
            ctx.request_repaint();
        }
        if ctx.input_mut(|input| input.consume_key(shortcut, egui::Key::S)) {
            self.focus = 4;
            self.focus_calculator = false;
            self.focus_task_date = false;
            // The sizing fields choose their own scroll target, including on
            // narrow displays where the section is below the model viewport.
            self.scroll_selected = false;
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
            self.stl.request_size_focus();
            ctx.request_repaint();
        }
        if ctx.input_mut(|input| input.consume_key(shortcut, egui::Key::H)) {
            self.focus = 4;
            self.focus_calculator = false;
            self.focus_task_date = false;
            self.scroll_selected = true;
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
            self.stl.toggle_human_comparison();
            ctx.request_repaint();
        }
        if ctx.input_mut(|input| input.consume_key(shortcut, egui::Key::G)) {
            self.focus = 4;
            self.focus_calculator = false;
            self.focus_task_date = false;
            self.scroll_selected = true;
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
            self.stl.toggle_grey_matcap();
            ctx.request_repaint();
        }
        if ctx.input_mut(|input| input.consume_key(shortcut, egui::Key::T)) {
            self.focus = 2;
            self.focus_calculator = false;
            self.focus_task_date = false;
            self.scroll_selected = true;
            self.timeline_view = !self.timeline_view;
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
            if self.timeline_view {
                self.timeline.reveal(self.selected_date);
                self.timeline.request_focus();
            } else if self.task_date_timeline {
                self.close_task_dates();
            }
            ctx.request_repaint();
        }
        if ctx.input_mut(|input| input.consume_key(shortcut, egui::Key::D)) {
            self.data_open = !self.data_open;
            ctx.request_repaint();
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::M)) {
            self.expanded = !self.expanded;
            if self.focus == 2
                && self.timeline_view
                && self.task_date_edit.is_none()
                && self.task_date_legacy.is_none()
            {
                self.timeline.request_focus();
            }
            ctx.request_repaint();
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F2)) {
            self.expanded = !self.expanded;
            if self.focus == 2
                && self.timeline_view
                && self.task_date_edit.is_none()
                && self.task_date_legacy.is_none()
            {
                self.timeline.request_focus();
            }
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.expanded = false;
            self.confirm = None;
            self.focus_calculator = false;
            self.focus_task_date = false;
            self.task_color_edit = None;
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
        }
        let editing_text = ctx
            .memory(|memory| memory.focused())
            .is_some_and(|id| TextEdit::load_state(ctx, id).is_some());
        if !editing_text {
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Z)) {
                self.undo_last(ctx);
            }
            let previous = ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft)
                    || input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
            });
            let next = ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)
                    || input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
            });
            if previous || next {
                let order = [0, 2, 1, 3, 4];
                let current = order
                    .iter()
                    .position(|index| *index == self.focus)
                    .unwrap_or(0);
                self.focus =
                    order[(current + if previous { order.len() - 1 } else { 1 }) % order.len()];
                self.focus_calculator = false;
                self.focus_task_date = false;
                self.scroll_selected = true;
                ctx.memory_mut(|memory| {
                    if let Some(id) = memory.focused() {
                        memory.surrender_focus(id);
                    }
                });
                ctx.request_repaint();
            }
        }
        egui::TopBottomPanel::top("jig-header").show(ctx, |ui| self.toolbar(ui));
        egui::TopBottomPanel::bottom("jig-status").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&self.status).size(11.0).color(
                    if self.storage_error || self.dirty {
                        PINK
                    } else {
                        BLUE
                    },
                ));
                ui.hyperlink_to("Source", "https://github.com/biraz-dev/Jig");
                ui.label(
                    RichText::new("Arrows select | Ctrl+M maximize | Esc return")
                        .size(11.0)
                        .color(BLUE),
                );
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            let height = ui.available_height().max(500.0);
            egui::ScrollArea::vertical()
                .id_salt("dashboard")
                .show(ui, |ui| {
                    if self.expanded {
                        self.tool(ui, self.focus, height - 8.0);
                    } else if ui.available_width() >= 760.0 {
                        let featured = self
                            .state
                            .featured
                            .is_some_and(|id| self.state.is_deadline_active(id));
                        self.tool(ui, 0, if featured { 104.0 } else { 48.0 });
                        ui.add_space(8.0);
                        let calendar_width = 330.0;
                        let width = ui.available_width();
                        let main_height = 350.0;
                        ui.horizontal_top(|ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(calendar_width, 0.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| self.tool(ui, 2, 0.0),
                            );
                            ui.add_space(12.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2((width - calendar_width - 20.0).max(250.0), main_height),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| self.tool(ui, 1, main_height),
                            );
                        });
                        ui.add_space(8.0);
                        if width >= 1120.0 {
                            let math_width = (width * 0.35).max(380.0);
                            ui.horizontal_top(|ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(math_width, 380.0),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| self.tool(ui, 3, 380.0),
                                );
                                ui.add_space(12.0);
                                ui.allocate_ui_with_layout(
                                    egui::vec2((width - math_width - 20.0).max(620.0), 380.0),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| self.tool(ui, 4, 380.0),
                                );
                            });
                        } else {
                            self.tool(ui, 3, 180.0);
                            ui.add_space(8.0);
                            self.tool(ui, 4, 380.0);
                        }
                    } else {
                        let featured = self
                            .state
                            .featured
                            .is_some_and(|id| self.state.is_deadline_active(id));
                        self.tool(ui, 0, if featured { 165.0 } else { 48.0 });
                        ui.add_space(8.0);
                        ui.scope(|ui| {
                            ui.set_max_width(ui.available_width().min(330.0));
                            self.tool(ui, 2, 0.0);
                        });
                        ui.add_space(8.0);
                        self.tool(ui, 1, 300.0);
                        ui.add_space(8.0);
                        self.tool(ui, 3, 220.0);
                        ui.add_space(8.0);
                        self.tool(ui, 4, 520.0);
                    }
                });
        });
        if self.collect_viewer_undo() {
            ctx.request_repaint();
        }
        browser_storage::set_unsaved(self.dirty);
    }
}
