//! Charts and small cell-linked interfaces, backed by the shared workbook.
use crate::{
    sheet_controls_ui::{self, ControlsPanel},
    text_input, web_theme,
};
use chrono::{Local, NaiveDate};
use eframe::egui::{self, Color32, FontId, RichText, Stroke, TextEdit, Ui};
use jig::{
    charts::{self, ChartKind, ChartScene, ChartSource, ChartSpec, Primitive, TextAnchor},
    model::State,
    sheet_controls::{ControlKind, ControlTarget},
    spreadsheet::{CellAddr, CellRange, Evaluation, Workbook},
};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub enum ChartAction {
    Add {
        kind: ChartKind,
        source: ChartSource,
        title: String,
    },
    Rename {
        id: u64,
        title: String,
    },
    Remove(u64),
    ControlAdd {
        label: String,
        target: Option<ControlTarget>,
        kind: ControlKind,
    },
    ControlEdit {
        id: u64,
        label: String,
        target: Option<ControlTarget>,
        kind: ControlKind,
    },
    ControlRemove(u64),
    ControlMove {
        id: u64,
        delta: isize,
    },
    ControlValue {
        id: u64,
        value: f64,
    },
    BackToGrid,
}

struct ChartDraft {
    kind: ChartKind,
    sheet: u64,
    range: String,
    title: String,
    tasks: bool,
    focus: bool,
}

impl ChartDraft {
    fn new(sheet: u64, range: CellRange) -> Self {
        Self {
            kind: ChartKind::Bar,
            sheet,
            range: format!("{}:{}", range.start.label(), range.end.label()),
            title: "Bar chart".into(),
            tasks: false,
            focus: true,
        }
    }

    fn action(&self) -> Result<ChartAction, String> {
        let source = if self.kind == ChartKind::Gantt && self.tasks {
            ChartSource::Tasks
        } else {
            let text = self.range.trim();
            let range = match text.split_once(':') {
                Some((first, last)) => {
                    CellRange::new(CellAddr::parse(first)?, CellAddr::parse(last)?)
                }
                None => {
                    let addr = CellAddr::parse(text)?;
                    CellRange::new(addr, addr)
                }
            };
            ChartSource::Range {
                sheet: self.sheet,
                range,
            }
        };
        Ok(ChartAction::Add {
            kind: self.kind,
            source,
            title: self.title.trim().into(),
        })
    }
}

#[derive(Default)]
pub struct ChartPanel {
    creation: Option<ChartDraft>,
    selection: Option<(u64, CellRange)>,
    controls: ControlsPanel,
    edit_layout: bool,
    title_edit: Option<(u64, String)>,
    title_original: Option<ChartSpec>,
    focus_title: bool,
    error: Option<String>,
    values: Option<Evaluation>,
    scenes: BTreeMap<u64, (f64, Result<ChartScene, String>)>,
    scenes_day: Option<NaiveDate>,
    scenes_dark: Option<bool>,
}

impl ChartPanel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        let mut controls = std::mem::take(&mut self.controls);
        controls.reset();
        *self = Self {
            controls,
            ..Self::new()
        };
    }

    pub fn invalidate(&mut self) {
        self.scenes.clear();
        self.values = None;
        self.cancel_gesture();
    }

    pub fn cancel_gesture(&mut self) {
        self.controls.cancel_gesture();
    }

    pub fn has_draft(&self) -> bool {
        self.creation.is_some() || self.title_edit.is_some() || self.controls.has_draft()
    }

    pub fn cancel_draft(&mut self) {
        self.creation = None;
        self.title_edit = None;
        self.title_original = None;
        self.error = None;
        self.controls.cancel_draft();
    }

    /// Update defaults for the next creation without changing any open drafts.
    pub fn set_selection_context(&mut self, sheet: u64, range: CellRange) {
        self.selection = Some((sheet, range));
    }

    /// Select cells in Sheets and send their live range to the chart creator.
    pub fn prefill_from_selection(&mut self, sheet: u64, range: CellRange) {
        self.set_selection_context(sheet, range);
        if let Some(draft) = &mut self.creation {
            draft.sheet = sheet;
            draft.range = format!("{}:{}", range.start.label(), range.end.label());
            draft.tasks = false;
            draft.focus = true;
        } else {
            self.creation = Some(ChartDraft::new(sheet, range));
        }
        self.error = None;
    }

    pub fn accept_action(&mut self, action: &ChartAction) {
        self.controls.accept_action(action);
        match action {
            ChartAction::Add { .. } => {
                self.creation = None;
                self.error = None;
            }
            ChartAction::Rename { .. } => {
                self.title_edit = None;
                self.title_original = None;
                self.error = None;
            }
            ChartAction::Remove(id)
                if self
                    .title_edit
                    .as_ref()
                    .is_some_and(|(editing, _)| editing == id) =>
            {
                self.title_edit = None;
                self.title_original = None;
            }
            _ => {}
        }
    }

    pub fn reject_action(&mut self, action: &ChartAction, error: String) {
        self.controls.reject_action(action, &error);
        match action {
            ChartAction::Add { .. } => self.error = Some(error),
            ChartAction::Rename { id, title } => {
                self.error = Some(error);
                self.title_edit = Some((*id, title.clone()));
                self.focus_title = true;
            }
            ChartAction::Remove(_) => self.error = Some(error),
            _ => {}
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &State) -> Vec<ChartAction> {
        let colors = web_theme::colors(ui.ctx());
        let today = Local::now().date_naive();
        let dark = ui.visuals().dark_mode;
        if self.scenes_day != Some(today) || self.scenes_dark != Some(dark) {
            self.invalidate();
            self.scenes_day = Some(today);
            self.scenes_dark = Some(dark);
        }
        if self.has_draft()
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.cancel_draft();
        }
        let empty = Workbook::default();
        let book = state.workbook.as_ref().unwrap_or(&empty);
        if let Some((id, _)) = &self.title_edit {
            if book.charts.iter().find(|chart| chart.id == *id) != self.title_original.as_ref() {
                self.title_edit = None;
                self.title_original = None;
                self.error = Some(
                    "This chart changed or was removed. Its old title draft was discarded; open Rename again to edit the current chart."
                        .into(),
                );
            }
        }
        if self.values.is_none() {
            self.values = Some(book.recalculate_at(today));
        }
        let mut actions = Vec::new();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Back to cells").clicked() {
                actions.push(ChartAction::BackToGrid);
            }
            ui.menu_button("Create", |ui| {
                for kind in [
                    ChartKind::Pie,
                    ChartKind::Line,
                    ChartKind::Bar,
                    ChartKind::Gantt,
                ] {
                    if ui.button(format!("{} chart", kind.label())).clicked() {
                        if let Some(draft) = &mut self.creation {
                            if draft.title == format!("{} chart", draft.kind.label()) {
                                draft.title = format!("{} chart", kind.label());
                            }
                            draft.kind = kind;
                            if kind != ChartKind::Gantt {
                                draft.tasks = false;
                            }
                            draft.focus = true;
                        } else {
                            let (sheet, range) = self.selection.unwrap_or_else(|| {
                                let sheet = book.sheets[0].id;
                                (
                                    sheet,
                                    book.used_range(sheet).unwrap_or(CellRange::new(
                                        CellAddr::new(0, 0),
                                        CellAddr::new(1, 1),
                                    )),
                                )
                            });
                            let mut draft = ChartDraft::new(sheet, range);
                            draft.kind = kind;
                            draft.title = format!("{} chart", kind.label());
                            self.creation = Some(draft);
                        }
                        self.error = None;
                        ui.close_menu();
                    }
                }
                ui.separator();
                for (label, kind) in [
                    ("Number input", ControlKind::Number),
                    (
                        "Slider",
                        ControlKind::Slider {
                            min: 0.0,
                            max: 100.0,
                            step: 1.0,
                        },
                    ),
                    ("Calculated result", ControlKind::Result),
                ] {
                    if ui.button(label).clicked() {
                        let target = self
                            .selection
                            .map(|(sheet, range)| ControlTarget {
                                sheet,
                                cell: range.start,
                            })
                            .unwrap_or(ControlTarget {
                                sheet: book.sheets[0].id,
                                cell: CellAddr::new(0, 0),
                            });
                        self.controls.begin_add(Some(target), kind);
                        ui.close_menu();
                    }
                }
            });
            ui.toggle_value(&mut self.edit_layout, "Edit layout");
        });
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).color(colors.error).size(12.0));
        }
        egui::ScrollArea::vertical()
            .id_salt("chart-gallery")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.creation_ui(ui, book, &mut actions);
                actions.extend(self.controls.ui(
                    ui,
                    state,
                    self.values.as_ref().unwrap(),
                    self.edit_layout,
                ));
                if book.charts.is_empty() && book.controls.is_empty() && !self.has_draft() {
                    ui.label(
                        RichText::new("Create a chart or add controls linked to your sheet cells.")
                            .color(colors.muted),
                    );
                }
                for chart in &book.charts {
                    ui.push_id(("chart-card", chart.id), |ui| {
                        self.chart_card(ui, state, book, chart, dark, &mut actions)
                    });
                    ui.add_space(10.0);
                }
            });
        actions
    }

    fn creation_ui(&mut self, ui: &mut Ui, book: &Workbook, actions: &mut Vec<ChartAction>) {
        let Some(draft) = &mut self.creation else {
            return;
        };
        let colors = web_theme::colors(ui.ctx());
        let mut create = false;
        let mut cancel = false;
        let opening = draft.focus;
        let frame = egui::Frame::default()
            .fill(colors.bg)
            .stroke(Stroke::new(0.6_f32, colors.rule))
            .inner_margin(egui::Margin::same(9))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for kind in [
                        ChartKind::Pie,
                        ChartKind::Line,
                        ChartKind::Bar,
                        ChartKind::Gantt,
                    ] {
                        if ui
                            .selectable_label(draft.kind == kind, kind.label())
                            .clicked()
                        {
                            if draft.title == format!("{} chart", draft.kind.label()) {
                                draft.title = format!("{} chart", kind.label());
                            }
                            draft.kind = kind;
                            if kind != ChartKind::Gantt {
                                draft.tasks = false;
                            }
                        }
                    }
                });
                if draft.focus {
                    text_input::consume_opening_keys(ui.ctx());
                    ui.memory_mut(|memory| memory.request_focus(egui::Id::new("chart-new-title")));
                    draft.focus = false;
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label("Title");
                    let response = ui.add(
                        TextEdit::singleline(&mut draft.title)
                            .id(egui::Id::new("chart-new-title"))
                            .return_key(None)
                            .char_limit(120)
                            .desired_width(260.0_f32.min(ui.available_width())),
                    );
                    if opening {
                        response.request_focus();
                    }
                    create |= sheet_controls_ui::enter(ui, &response);
                });
                if draft.kind == ChartKind::Gantt {
                    ui.checkbox(&mut draft.tasks, "Live Jig tasks");
                }
                if !draft.tasks {
                    ui.horizontal_wrapped(|ui| {
                        sheet_controls_ui::sheet_choice(
                            ui,
                            "chart-source-sheet",
                            book,
                            &mut draft.sheet,
                        );
                        ui.label("Range");
                        let response = ui.add(
                            TextEdit::singleline(&mut draft.range)
                                .id(egui::Id::new("chart-new-range"))
                                .return_key(None)
                                .hint_text("A1:B8")
                                .char_limit(40)
                                .desired_width(130.0_f32.min(ui.available_width())),
                        );
                        create |= sheet_controls_ui::enter(ui, &response);
                    });
                }
                ui.horizontal(|ui| {
                    create |= ui.button("Create chart").clicked();
                    cancel = ui.small_button("Cancel").clicked();
                });
            });
        if opening {
            frame.response.scroll_to_me(Some(egui::Align::Min));
        }
        ui.add_space(8.0);
        if cancel {
            self.creation = None;
            self.error = None;
        } else if create {
            match draft.action() {
                Ok(action) => {
                    self.error = None;
                    actions.push(action);
                }
                Err(error) => self.error = Some(error),
            }
        }
    }

    fn chart_card(
        &mut self,
        ui: &mut Ui,
        state: &State,
        book: &Workbook,
        chart: &ChartSpec,
        dark: bool,
        actions: &mut Vec<ChartAction>,
    ) {
        let colors = web_theme::colors(ui.ctx());
        egui::Frame::default()
            .fill(colors.bg)
            .stroke(Stroke::new(0.6_f32, colors.rule))
            .inner_margin(egui::Margin::same(9))
            .show(ui, |ui| {
                let source = match chart.source {
                    ChartSource::Tasks => "Live tasks".into(),
                    ChartSource::Range { sheet, range } => {
                        let name = book
                            .sheet(sheet)
                            .map_or("Missing sheet", |sheet| sheet.name.as_str());
                        format!("{name} · {}:{}", range.start.label(), range.end.label())
                    }
                };
                if self.edit_layout {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(chart.kind.label())
                                .color(colors.accent)
                                .size(12.0),
                        );
                        ui.label(RichText::new(&source).color(colors.muted).size(11.0));
                        if ui.small_button("Rename").clicked() {
                            if self.title_edit.is_some() {
                                self.error =
                                    Some("Save or cancel the open title editor first.".into());
                            } else {
                                self.title_edit = Some((chart.id, chart.title.clone()));
                                self.title_original = Some(chart.clone());
                                self.error = None;
                            }
                            self.focus_title = true;
                        }
                        if ui
                            .small_button("del")
                            .on_hover_text("Remove this chart; Undo restores it")
                            .clicked()
                        {
                            actions.push(ChartAction::Remove(chart.id));
                        }
                    });
                }
                self.rename_ui(ui, chart.id, actions);
                let width = f64::from(ui.available_width().clamp(280.0, 2000.0));
                let scene = || {
                    if dark {
                        charts::scene(state, chart, width, 280.0)
                    } else {
                        charts::scene_light(state, chart, width, 280.0)
                    }
                };
                let cached = self
                    .scenes
                    .entry(chart.id)
                    .or_insert_with(|| (width, scene()));
                if cached.0 != width {
                    *cached = (width, scene());
                }
                match &cached.1 {
                    Ok(scene) => {
                        paint_scene(ui, scene).on_hover_text(source);
                    }
                    Err(error) => {
                        ui.label(RichText::new(&chart.title).color(colors.text));
                        ui.label(RichText::new(error).color(colors.error).size(12.0));
                    }
                }
            });
    }

    fn rename_ui(&mut self, ui: &mut Ui, id: u64, actions: &mut Vec<ChartAction>) {
        if !self
            .title_edit
            .as_ref()
            .is_some_and(|(editing, _)| *editing == id)
        {
            return;
        }
        let mut save = false;
        let mut cancel = false;
        ui.horizontal_wrapped(|ui| {
            let field = egui::Id::new(("chart-title", id));
            if self.focus_title {
                text_input::consume_opening_keys(ui.ctx());
                ui.memory_mut(|memory| memory.request_focus(field));
                self.focus_title = false;
            }
            let (_, title) = self.title_edit.as_mut().unwrap();
            let response = ui.add(
                TextEdit::singleline(title)
                    .id(field)
                    .return_key(None)
                    .char_limit(120)
                    .desired_width(280.0_f32.min(ui.available_width())),
            );
            save = ui.button("Save title").clicked() || sheet_controls_ui::enter(ui, &response);
            cancel = ui.small_button("Cancel").clicked();
        });
        if cancel {
            self.title_edit = None;
            self.title_original = None;
            self.error = None;
        } else if save {
            if let Some((id, title)) = &self.title_edit {
                actions.push(ChartAction::Rename {
                    id: *id,
                    title: title.trim().into(),
                });
            }
        }
    }
}

pub fn paint_scene(ui: &mut Ui, scene: &ChartScene) -> egui::Response {
    let scale = (ui.available_width() / scene.width as f32).clamp(0.1, 1.0);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(scene.width as f32, scene.height as f32) * scale,
        egui::Sense::focusable_noninteractive(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), &scene.title)
    });
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let point = |x: f64, y: f64| rect.min + egui::vec2(x as f32, y as f32) * scale;
    let ink = |color: [u8; 3]| Color32::from_rgb(color[0], color[1], color[2]);
    for primitive in &scene.primitives {
        match primitive {
            Primitive::Rect {
                x,
                y,
                width,
                height,
                fill,
                stroke,
            } => {
                let bounds = egui::Rect::from_min_size(
                    point(*x, *y),
                    egui::vec2(*width as f32, *height as f32) * scale,
                );
                if let Some(fill) = fill {
                    painter.rect_filled(bounds, 0.0, ink(*fill));
                }
                if let Some(stroke) = stroke {
                    let stroke = Stroke::new(stroke.width as f32 * scale, ink(stroke.color));
                    painter.line_segment([bounds.left_top(), bounds.right_top()], stroke);
                    painter.line_segment([bounds.right_top(), bounds.right_bottom()], stroke);
                    painter.line_segment([bounds.right_bottom(), bounds.left_bottom()], stroke);
                    painter.line_segment([bounds.left_bottom(), bounds.left_top()], stroke);
                }
            }
            Primitive::Line {
                x1,
                y1,
                x2,
                y2,
                stroke,
            } => {
                painter.line_segment(
                    [point(*x1, *y1), point(*x2, *y2)],
                    Stroke::new(stroke.width as f32 * scale, ink(stroke.color)),
                );
            }
            Primitive::Text {
                x,
                y,
                text,
                size,
                color,
                anchor,
            } => {
                let align = match anchor {
                    TextAnchor::Start => egui::Align2::LEFT_BOTTOM,
                    TextAnchor::Middle => egui::Align2::CENTER_BOTTOM,
                    TextAnchor::End => egui::Align2::RIGHT_BOTTOM,
                };
                painter.text(
                    point(*x, *y + *size * 0.18),
                    align,
                    text,
                    FontId::monospace(*size as f32 * scale),
                    ink(*color),
                );
            }
            Primitive::Polygon {
                points,
                fill,
                stroke,
            } => {
                let points = points.iter().map(|p| point(p[0], p[1])).collect::<Vec<_>>();
                if let Some(fill) = fill {
                    let mut mesh = egui::epaint::Mesh::default();
                    for p in &points {
                        mesh.colored_vertex(*p, ink(*fill));
                    }
                    for index in 1..points.len().saturating_sub(1) {
                        mesh.add_triangle(0, index as u32, index as u32 + 1);
                    }
                    painter.add(egui::Shape::mesh(mesh));
                }
                if let Some(stroke) = stroke {
                    painter.add(egui::Shape::closed_line(
                        points,
                        Stroke::new(stroke.width as f32 * scale, ink(stroke.color)),
                    ));
                }
            }
        }
    }
    response
}
