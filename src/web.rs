//! egui adapter for the same local utility model used by the CLI and terminal.
use crate::{browser_storage, weather_ui::WeatherPanel};
use chrono::{Datelike, Local, NaiveDate, Utc};
use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke, TextEdit, Ui};
use jig::{calendar, model::State, units};
use std::time::Duration;

const BG: Color32 = Color32::from_rgb(12, 19, 27);
const CREAM: Color32 = Color32::from_rgb(246, 218, 174);
const PINK: Color32 = Color32::from_rgb(234, 133, 173);
const BLUE: Color32 = Color32::from_rgb(144, 161, 202);
const ACCENT: Color32 = Color32::from_rgb(109, 185, 238);
const GREEN: Color32 = Color32::from_rgb(164, 223, 144);

#[derive(Clone, Copy)]
enum Confirm {
    Empty,
    Clear,
}

pub struct BrowserApp {
    state: State,
    weather: WeatherPanel,
    demo: bool,
    dirty: bool,
    storage_error: bool,
    status: String,
    confirm: Option<Confirm>,
    pending_import: Option<State>,
    focus: usize,
    expanded: bool,
    focus_calculator: bool,
    focus_length: bool,
    calculation: String,
    length: String,
    task_text: String,
    task_edit: Option<u64>,
    confirm_clear_completed: bool,
    deadline_title: String,
    deadline_at: String,
    deadline_edit: Option<u64>,
    year: i32,
    month: u32,
    selected_date: NaiveDate,
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
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BLUE);
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
        Self {
            state,
            weather,
            demo,
            dirty: false,
            storage_error,
            status,
            confirm: None,
            pending_import: None,
            focus: 0,
            expanded: false,
            focus_calculator: false,
            focus_length: false,
            calculation: String::new(),
            length: "12in".into(),
            task_text: String::new(),
            task_edit: None,
            confirm_clear_completed: false,
            deadline_title: String::new(),
            deadline_at: String::new(),
            deadline_edit: None,
            year: today.year(),
            month: today.month(),
            selected_date: today,
        }
    }

    fn changed(&mut self) {
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

    fn apply(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => self.changed(),
            Err(error) => self.status = error,
        }
    }

    fn reset_editors(&mut self) {
        self.task_edit = None;
        self.task_text.clear();
        self.confirm_clear_completed = false;
        self.deadline_edit = None;
        self.deadline_title.clear();
        self.deadline_at.clear();
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
        self.changed();
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
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("JIG")
                    .font(FontId::monospace(29.0))
                    .color(CREAM)
                    .strong(),
            );
            ui.label(RichText::new("You are a small Small Giant Psionicist.").color(BLUE));
            ui.label(
                RichText::new(Local::now().format("%a %d %b %Y  %H:%M:%S").to_string()).color(BLUE),
            );
            ui.label(RichText::new("• local").color(GREEN));
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(if self.demo {
                    "DEVELOPMENT PREVIEW — SAMPLE DATA"
                } else {
                    "DEVELOPMENT PREVIEW — YOUR LOCAL DATA"
                })
                .color(PINK)
                .strong(),
            );
            if ui.button("Start empty").clicked() {
                self.confirm = Some(Confirm::Empty);
            }
            if ui.button("Open local data").clicked() {
                self.open_local();
            }
            if ui.button("Sample mode").clicked() {
                if self.dirty {
                    self.status =
                        "Save your changes before switching to sample mode. If saving is unavailable, export JSON, then reload before switching.".into();
                } else {
                    self.state = jig::sample::state(Utc::now());
                    self.restore_weather();
                    self.demo = true;
                    self.reset_editors();
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
            if self.dirty && ui.button("Retry save").clicked() {
                self.changed();
            }
        });
        if let Some(confirm) = self.confirm {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(match confirm { Confirm::Empty => "Replace this browser's saved data with an empty workbench? Export first if needed.", Confirm::Clear => "Delete this browser's saved Jig data? Export a backup first if needed." }).color(PINK));
                if ui.button("Confirm").clicked() {
                    self.confirm = None;
                    match confirm {
                        Confirm::Empty => self.replace_personal(State::default()),
                        Confirm::Clear => match browser_storage::clear() {
                            Ok(()) => { self.weather.disable(); self.state = jig::sample::state(Utc::now()); self.demo = true; self.dirty = false; self.storage_error = false; self.reset_editors(); self.status = "Browser-local data cleared. Displaying temporary sample data.".into(); }
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
        let focused = self.focus == index;
        let expanded = self.expanded;
        let mut expand = false;
        let frame = egui::Frame::default()
            .fill(BG)
            .stroke(Stroke::new(1.0_f32, if focused { PINK } else { BLUE }))
            .inner_margin(egui::Margin::same(11))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(height.max(150.0) - 24.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        expand = ui
                            .small_button(if expanded { "close" } else { "open" })
                            .clicked();
                    });
                });
                ui.add_space(3.0);
                if index == 2 {
                    self.calendar(ui);
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt(("instrument", index))
                        .max_height((height - 62.0).max(100.0))
                        .show(ui, |ui| match index {
                            0 => self.deadlines(ui),
                            1 => self.tasks(ui),
                            _ => self.calculator_length(ui),
                        });
                }
            });
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
        if expand {
            self.focus = index;
            self.expanded = !self.expanded;
        }
    }

    fn calculator(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let response = ui.add(
                TextEdit::singleline(&mut self.calculation)
                    .id_salt("calculator-input")
                    .font(egui::TextStyle::Monospace)
                    .hint_text("125 * 1.15 / ans / 50%")
                    .desired_width((ui.available_width() - 95.0).max(80.0)),
            );
            if response.has_focus() {
                self.focus = 3;
            }
            if self.focus_calculator {
                response.request_focus();
                response.scroll_to_me(None);
                self.focus_calculator = false;
            }
            let enter =
                response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if ui.button("Calculate").clicked() || enter {
                match self.state.calculate(&self.calculation) {
                    Ok(_) => {
                        self.calculation.clear();
                        self.changed();
                    }
                    Err(error) => self.status = error,
                }
            }
        });
        if let Some(last) = self.state.history.last() {
            ui.label(
                RichText::new(format!("= {}", self.state.number(last.result)))
                    .font(FontId::monospace(28.0))
                    .color(GREEN),
            );
        }
        ui.separator();
        for history in self.state.history.iter().rev() {
            ui.horizontal_wrapped(|ui| {
                if ui.link(&history.expression).clicked() {
                    self.calculation = history.expression.clone();
                    self.focus_calculator = true;
                }
                if ui
                    .button(format!("= {}", self.state.number(history.result)))
                    .clicked()
                {
                    self.calculation = history.result.to_string();
                    self.focus_calculator = true;
                }
            });
        }
    }

    fn calculator_length(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
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
        if ui.available_width() >= 480.0 {
            ui.columns(2, |columns| {
                self.calculator(&mut columns[0]);
                self.length(&mut columns[1]);
            });
        } else {
            self.calculator(ui);
            ui.add_space(8.0);
            ui.separator();
            ui.scope(|ui| {
                ui.set_max_width(ui.available_width().min(240.0));
                self.length(ui);
            });
        }
    }

    fn use_calculation_result(&mut self) {
        if let Some(last) = self.state.history.last() {
            let input = self.length.trim();
            let source = input
                .find(|c: char| c.is_ascii_alphabetic())
                .map(|index| input[index..].trim())
                .filter(|unit| units::convert(&format!("1{unit}"), "m").is_ok())
                .unwrap_or("in");
            self.length = format!("{}{source}", last.result);
            self.focus = 3;
            self.focus_calculator = false;
            self.focus_length = true;
        }
    }

    fn length(&mut self, ui: &mut Ui) {
        let response = ui.add(
            TextEdit::singleline(&mut self.length)
                .id_salt("length-input")
                .hint_text("12in / 1 1/2in")
                .desired_width(ui.available_width()),
        );
        if response.has_focus() {
            self.focus = 3;
        }
        if self.focus_length {
            response.request_focus();
            response.scroll_to_me(None);
            self.focus_length = false;
        }
        if ui
            .add_enabled(
                !self.state.history.is_empty(),
                egui::Button::new("Use result").small(),
            )
            .on_hover_text("Use the calculator result without rounding · Ctrl+Alt+U")
            .clicked()
        {
            self.use_calculation_result();
        }
        if let Err(error) = units::convert(&self.length, "m") {
            ui.label(RichText::new(error).color(PINK));
            return;
        }
        egui::Grid::new("length-results")
            .num_columns(2)
            .spacing([14.0, 5.0])
            .show(ui, |ui| {
                for unit in units::UNITS {
                    ui.label(RichText::new(unit).color(ACCENT));
                    if let Ok(result) = units::convert(&self.length, unit) {
                        ui.label(
                            RichText::new(self.state.number(result)).font(FontId::monospace(18.0)),
                        );
                    }
                    ui.end_row();
                }
            });
    }

    fn tasks(&mut self, ui: &mut Ui) {
        let tasks = self.state.tasks.clone();
        for (index, task) in tasks.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                let mut complete = task.complete;
                if ui.checkbox(&mut complete, "").changed() {
                    let result = self.state.toggle_task(task.id);
                    self.apply(result);
                }
                let text =
                    RichText::new(&task.text).color(if task.complete { BLUE } else { CREAM });
                ui.label(if task.complete {
                    text.strikethrough()
                } else {
                    text
                });
                if ui.small_button("edit").clicked() {
                    self.task_edit = Some(task.id);
                    self.task_text = task.text.clone();
                }
                if ui
                    .add_enabled(index > 0, egui::Button::new("↑").small())
                    .clicked()
                {
                    let result = self.state.move_task(task.id, index);
                    self.apply(result);
                }
                if ui
                    .add_enabled(index + 1 < tasks.len(), egui::Button::new("↓").small())
                    .clicked()
                {
                    let result = self.state.move_task(task.id, index + 2);
                    self.apply(result);
                }
                if ui.small_button("del").clicked() {
                    let result = self.state.remove_task(task.id);
                    self.apply(result);
                    if self.task_edit == Some(task.id) {
                        self.task_edit = None;
                        self.task_text.clear();
                    }
                }
            });
        }
        let completed = self.state.tasks.iter().filter(|task| task.complete).count();
        if completed > 0 {
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
        ui.separator();
        ui.horizontal(|ui| {
            let response = ui.add(
                TextEdit::singleline(&mut self.task_text)
                    .hint_text("Add new task")
                    .desired_width((ui.available_width() - 80.0).max(80.0)),
            );
            if response.has_focus() {
                self.focus = 1;
            }
            if ui
                .button(if self.task_edit.is_some() {
                    "Save task"
                } else {
                    "Add task"
                })
                .clicked()
                || (response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
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
        if self.task_edit.is_some() && ui.small_button("Cancel task edit").clicked() {
            self.task_edit = None;
            self.task_text.clear();
        }
    }

    fn deadlines(&mut self, ui: &mut Ui) {
        if let Some(deadline) = self
            .state
            .deadlines
            .iter()
            .find(|d| Some(d.id) == self.state.featured)
        {
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(deadline.title.to_uppercase())
                        .color(CREAM)
                        .strong(),
                );
                let seconds = (deadline.at - Utc::now()).num_seconds();
                if seconds < 0 {
                    ui.label(RichText::new("OVERDUE").color(PINK));
                }
                let n = seconds.unsigned_abs();
                let values = [n / 86400, n / 3600 % 24, n / 60 % 60, n % 60];
                let size = (ui.available_width() / 11.0).clamp(23.0, 57.0);
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
                ui.label(
                    RichText::new(
                        deadline
                            .at
                            .with_timezone(&Local)
                            .format("%a %d %b %Y  %H:%M %:z")
                            .to_string(),
                    )
                    .color(BLUE),
                );
            });
        } else {
            ui.label(RichText::new("No featured deadline").color(BLUE));
        }
        ui.separator();
        ui.label(RichText::new("Upcoming deadlines").color(ACCENT));
        for deadline in self.state.deadlines.clone() {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .selectable_label(self.state.featured == Some(deadline.id), "★")
                    .clicked()
                {
                    self.feature_deadline(deadline.id);
                }
                ui.label(&deadline.title);
                ui.label(
                    RichText::new(
                        deadline
                            .at
                            .with_timezone(&Local)
                            .format("%d %b %H:%M")
                            .to_string(),
                    )
                    .color(BLUE),
                );
                if ui.small_button("edit").clicked() {
                    self.deadline_edit = Some(deadline.id);
                    self.deadline_title = deadline.title.clone();
                    self.deadline_at = deadline.at.to_rfc3339();
                }
                if ui.small_button("del").clicked() {
                    let result = self.state.remove_deadline(deadline.id);
                    self.apply(result);
                    if self.deadline_edit == Some(deadline.id) {
                        self.deadline_edit = None;
                        self.deadline_title.clear();
                        self.deadline_at.clear();
                    }
                }
            });
        }
        ui.add_space(5.0);
        let title_response = ui.add(
            TextEdit::singleline(&mut self.deadline_title)
                .hint_text("Add new deadline")
                .desired_width(f32::INFINITY),
        );
        let time_response = ui.add(
            TextEdit::singleline(&mut self.deadline_at)
                .hint_text("2026-12-01T18:00:00+01:00")
                .desired_width(f32::INFINITY),
        );
        if title_response.has_focus() || time_response.has_focus() {
            self.focus = 0;
        }
        ui.horizontal(|ui| {
            if ui
                .button(if self.deadline_edit.is_some() {
                    "Save deadline"
                } else {
                    "Add deadline"
                })
                .clicked()
            {
                let result = self
                    .state
                    .set_deadline(self.deadline_edit, &self.deadline_title, &self.deadline_at)
                    .map(|_| ());
                if result.is_ok() {
                    self.deadline_edit = None;
                    self.deadline_title.clear();
                    self.deadline_at.clear();
                }
                self.apply(result);
            }
            if self.deadline_edit.is_some() && ui.button("Cancel").clicked() {
                self.deadline_edit = None;
                self.deadline_title.clear();
                self.deadline_at.clear();
            }
        });
    }

    fn feature_deadline(&mut self, id: u64) {
        let date = self
            .state
            .deadlines
            .iter()
            .find(|deadline| deadline.id == id)
            .map(|deadline| deadline.at.with_timezone(&Local).date_naive());
        let result = self.state.feature(id);
        if result.is_ok() {
            if let Some(date) = date {
                self.selected_date = date;
                self.year = date.year();
                self.month = date.month();
            }
        }
        self.apply(result);
    }

    fn calendar(&mut self, ui: &mut Ui) {
        ui.spacing_mut().button_padding.x = 3.0;
        if self.weather.controls(ui) {
            self.state.preferences.weather = self.weather.config().cloned();
            if !self.demo {
                self.changed();
            }
        }
        ui.horizontal(|ui| {
            if ui.button("‹").clicked() {
                self.shift_month(-1);
            }
            ui.label(RichText::new(format!("{:04} / {:02}", self.year, self.month)).color(ACCENT));
            if ui.button("›").clicked() {
                self.shift_month(1);
            }
            if ui.button("Today").clicked() {
                let today = Local::now().date_naive();
                self.year = today.year();
                self.month = today.month();
                self.selected_date = today;
            }
        });
        let today = Local::now().date_naive();
        let featured_date = self
            .state
            .deadlines
            .iter()
            .find(|deadline| Some(deadline.id) == self.state.featured)
            .map(|deadline| deadline.at.with_timezone(&Local).date_naive());
        let size = ((ui.available_width() - 48.0) / 7.0).clamp(22.0, 55.0);
        if let Ok(cells) = calendar::month(self.year, self.month) {
            egui::Grid::new("month-grid")
                .num_columns(7)
                .spacing([6.0, 4.0])
                .show(ui, |ui| {
                    for day in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
                        ui.label(RichText::new(day).color(BLUE));
                    }
                    ui.end_row();
                    let count = cells
                        .iter()
                        .rposition(Option::is_some)
                        .map_or(0, |index| (index / 7 + 1) * 7);
                    let weather_height = if self.weather.is_enabled() {
                        53.0
                    } else {
                        27.0
                    };
                    for (index, day) in cells[..count].iter().enumerate() {
                        if let Some(day) = day {
                            let mut deadlines: Vec<_> = self
                                .state
                                .deadlines
                                .iter()
                                .filter(|deadline| {
                                    deadline.at.with_timezone(&Local).date_naive() == *day
                                })
                                .map(|deadline| (deadline.at, deadline.id, deadline.title.clone()))
                                .collect();
                            deadlines.sort_by_key(|deadline| deadline.0);
                            let color = if *day == today {
                                BG
                            } else if !deadlines.is_empty() {
                                PINK
                            } else {
                                CREAM
                            };
                            let mut button = egui::Button::new(
                                RichText::new(self.weather.cell_text(*day))
                                    .size(12.0)
                                    .color(color),
                            )
                            .min_size(egui::vec2(size, weather_height));
                            if *day == today {
                                button = button.fill(ACCENT);
                            }
                            if Some(*day) == featured_date {
                                button = button.stroke(Stroke::new(2.0_f32, PINK));
                            } else if *day == self.selected_date {
                                button = button.stroke(Stroke::new(1.0_f32, BLUE));
                            }
                            let mut response = ui.add(button);
                            let mut tooltip = deadlines
                                .iter()
                                .map(|deadline| deadline.2.as_str())
                                .collect::<Vec<_>>()
                                .join("\n");
                            if let Some(forecast) = self.weather.day_tooltip(*day) {
                                if !tooltip.is_empty() {
                                    tooltip.push('\n');
                                }
                                tooltip.push_str(&forecast);
                            }
                            if !tooltip.is_empty() {
                                response = response.on_hover_text(tooltip);
                            }
                            if response.has_focus() {
                                self.focus = 2;
                            }
                            if response.clicked() {
                                self.selected_date = *day;
                                if let Some((_, id, _)) = deadlines.first() {
                                    self.feature_deadline(*id);
                                }
                            }
                        } else {
                            ui.allocate_space(egui::vec2(size, weather_height));
                        }
                        if index % 7 == 6 {
                            ui.end_row();
                        }
                    }
                });
        }
        self.weather.selected_day(ui, self.selected_date);
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
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !ctx.style().visuals.dark_mode {
            ctx.set_theme(egui::ThemePreference::Dark);
        }
        ctx.request_repaint_after(Duration::from_secs(1));
        self.weather.update();
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
        if ctx.input_mut(|input| input.consume_key(shortcut, egui::Key::U)) {
            self.use_calculation_result();
            if ctx.screen_rect().width() < 1040.0 {
                self.expanded = true;
            }
        }
        if ctx.input_mut(|input| input.consume_key(shortcut, egui::Key::Num4)) {
            self.focus = 3;
            self.focus_calculator = true;
            self.focus_length = false;
            if ctx.screen_rect().width() < 1040.0 {
                self.expanded = true;
            }
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F2)) {
            self.expanded = !self.expanded;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.expanded = false;
            self.confirm = None;
        }
        egui::TopBottomPanel::top("jig-header").show(ctx, |ui| self.toolbar(ui));
        egui::TopBottomPanel::bottom("jig-status").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(&self.status).color(if self.storage_error || self.dirty {
                        PINK
                    } else {
                        ACCENT
                    }),
                );
                ui.hyperlink_to("Source", "https://github.com/biraz-dev/Jig");
                ui.label(
                    RichText::new("JSON stays on this device. No backend or telemetry.")
                        .small()
                        .color(BLUE),
                );
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            let height = ui.available_height().max(620.0);
            egui::ScrollArea::vertical()
                .id_salt("dashboard")
                .show(ui, |ui| {
                    if self.expanded {
                        self.tool(ui, self.focus, height - 10.0);
                    } else if ui.available_width() >= 1040.0 {
                        ui.columns(2, |columns| {
                            let calendar_height = if self.weather.is_enabled() {
                                500.0
                            } else {
                                300.0
                            };
                            self.tool(
                                &mut columns[0],
                                0,
                                (height - calendar_height - 8.0).max(300.0),
                            );
                            columns[0].add_space(8.0);
                            self.tool(&mut columns[0], 2, calendar_height);
                            self.tool(&mut columns[1], 1, height - 308.0);
                            columns[1].add_space(8.0);
                            self.tool(&mut columns[1], 3, 300.0);
                        });
                    } else {
                        for index in [0, 1, 2, 3] {
                            let tool_height = match index {
                                2 => {
                                    if self.weather.is_enabled() {
                                        500.0
                                    } else {
                                        300.0
                                    }
                                }
                                3 => 430.0,
                                _ => 300.0,
                            };
                            self.tool(ui, index, tool_height);
                            ui.add_space(8.0);
                        }
                    }
                });
        });
        browser_storage::set_unsaved(self.dirty);
    }
}
