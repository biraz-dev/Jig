//! Small cell-linked controls. Drafts stay in the UI until explicitly applied.
use crate::{chart_ui::ChartAction, text_input, web_theme};
use eframe::egui::{self, RichText, Stroke, TextEdit, Ui};
use jig::{
    model::State,
    sheet_controls::{ControlKind, ControlTarget, SheetControl},
    spreadsheet::{CellAddr, CellValue, Evaluation, Workbook},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, PartialEq)]
enum ControlType {
    Number,
    Slider,
    Result,
}

impl ControlType {
    fn label(self) -> &'static str {
        match self {
            Self::Number => "Number input",
            Self::Slider => "Slider",
            Self::Result => "Calculated result",
        }
    }
}

struct ControlDraft {
    id: Option<u64>,
    original: Option<SheetControl>,
    label: String,
    sheet: u64,
    cell: String,
    kind: ControlType,
    min: String,
    max: String,
    step: String,
    focus: bool,
}

impl ControlDraft {
    fn new(target: Option<ControlTarget>) -> Self {
        Self {
            id: None,
            original: None,
            label: String::new(),
            sheet: target.as_ref().map_or(1, |target| target.sheet),
            cell: target.map_or(String::new(), |target| target.cell.label()),
            kind: ControlType::Number,
            min: "0".into(),
            max: "100".into(),
            step: "1".into(),
            focus: true,
        }
    }

    fn from_control(control: &SheetControl) -> Self {
        let mut draft = Self::new(control.target);
        draft.id = Some(control.id);
        draft.original = Some(control.clone());
        draft.label = control.label.clone();
        draft.kind = match &control.kind {
            ControlKind::Number => ControlType::Number,
            ControlKind::Result => ControlType::Result,
            ControlKind::Slider { min, max, step } => {
                draft.min = min.to_string();
                draft.max = max.to_string();
                draft.step = step.to_string();
                ControlType::Slider
            }
        };
        draft
    }

    fn action(&self) -> Result<ChartAction, String> {
        let target = if self.cell.trim().is_empty() {
            return Err("Choose a source cell, such as A1".into());
        } else {
            Some(ControlTarget {
                sheet: self.sheet,
                cell: CellAddr::parse(&self.cell)?,
            })
        };
        let kind = match self.kind {
            ControlType::Number => ControlKind::Number,
            ControlType::Result => ControlKind::Result,
            ControlType::Slider => ControlKind::Slider {
                min: number(&self.min)?,
                max: number(&self.max)?,
                step: number(&self.step)?,
            },
        };
        let label = self.label.trim().to_owned();
        Ok(match self.id {
            Some(id) => ChartAction::ControlEdit {
                id,
                label,
                target,
                kind,
            },
            None => ChartAction::ControlAdd {
                label,
                target,
                kind,
            },
        })
    }
}

struct NumberDraft {
    source: String,
    text: String,
    dirty: bool,
    error: Option<String>,
}

struct SliderPreview {
    target: ControlTarget,
    source: String,
    value: f64,
}

#[derive(Default)]
pub struct ControlsPanel {
    editor: Option<ControlDraft>,
    error: Option<String>,
    numbers: BTreeMap<u64, NumberDraft>,
    sliders: BTreeMap<u64, SliderPreview>,
    cancelled_sliders: BTreeSet<u64>,
}

impl ControlsPanel {
    pub fn begin_add(&mut self, target: Option<ControlTarget>, kind: ControlKind) {
        if let Some(draft) = &mut self.editor {
            if draft.id.is_some() {
                draft.focus = true;
                self.error =
                    Some("Save or cancel this edit before creating another control.".into());
                return;
            }
        }
        let mut draft = self
            .editor
            .take()
            .unwrap_or_else(|| ControlDraft::new(target));
        draft.kind = match kind {
            ControlKind::Number => ControlType::Number,
            ControlKind::Slider { min, max, step } => {
                draft.min = min.to_string();
                draft.max = max.to_string();
                draft.step = step.to_string();
                ControlType::Slider
            }
            ControlKind::Result => ControlType::Result,
        };
        draft.focus = true;
        self.editor = Some(draft);
        self.error = None;
    }

    pub fn cancel_gesture(&mut self) {
        self.cancelled_sliders.extend(self.sliders.keys().copied());
        self.sliders.clear();
    }

    pub fn has_draft(&self) -> bool {
        self.editor.is_some() || self.numbers.values().any(|draft| draft.dirty)
    }

    pub fn cancel_draft(&mut self) {
        self.editor = None;
        self.error = None;
        self.numbers.clear();
        self.cancel_gesture();
    }

    pub fn accept_action(&mut self, action: &ChartAction) {
        match action {
            ChartAction::ControlAdd { .. } | ChartAction::ControlEdit { .. } => {
                self.editor = None;
                self.error = None;
                if let ChartAction::ControlEdit { id, .. } = action {
                    self.numbers.remove(id);
                    self.sliders.remove(id);
                }
            }
            ChartAction::ControlRemove(id) => {
                self.numbers.remove(id);
                self.sliders.remove(id);
                if self
                    .editor
                    .as_ref()
                    .is_some_and(|draft| draft.id == Some(*id))
                {
                    self.editor = None;
                }
            }
            ChartAction::ControlValue { id, .. } => {
                if let Some(draft) = self.numbers.get_mut(id) {
                    draft.dirty = false;
                    draft.error = None;
                }
                self.sliders.remove(id);
            }
            _ => {}
        }
    }

    pub fn reject_action(&mut self, action: &ChartAction, error: &str) {
        match action {
            ChartAction::ControlValue { id, .. } => {
                self.sliders.remove(id);
                if let Some(draft) = self.numbers.get_mut(id) {
                    draft.error = Some(error.into());
                } else {
                    self.error = Some(error.into());
                }
            }
            ChartAction::ControlAdd { .. } | ChartAction::ControlEdit { .. } => {
                self.error = Some(error.into());
                if let Some(draft) = &mut self.editor {
                    draft.focus = true;
                }
            }
            _ => {}
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        state: &State,
        values: &Evaluation,
        editing: bool,
    ) -> Vec<ChartAction> {
        let mut actions = Vec::new();
        let empty = Workbook::default();
        let book = state.workbook.as_ref().unwrap_or(&empty);
        if ui.input(|input| {
            !input.focused
                || input.key_pressed(egui::Key::Escape)
                || input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Touch {
                            phase: egui::TouchPhase::Cancel,
                            ..
                        }
                    )
                })
        }) {
            self.cancel_gesture();
        }
        self.numbers
            .retain(|id, _| book.controls.iter().any(|control| control.id == *id));
        self.sliders
            .retain(|id, _| book.controls.iter().any(|control| control.id == *id));
        self.cancelled_sliders
            .retain(|id| book.controls.iter().any(|control| control.id == *id));
        if self.editor.is_none() {
            if let Some(error) = &self.error {
                ui.label(
                    RichText::new(error)
                        .color(web_theme::colors(ui.ctx()).error)
                        .size(12.0),
                );
            }
        }
        self.editor_ui(ui, book, &mut actions);
        if book.controls.is_empty() {
            return actions;
        }
        let columns = if ui.available_width() >= 680.0 { 2 } else { 1 };
        for row in book.controls.chunks(columns) {
            ui.columns(columns, |columns| {
                for (ui, control) in columns.iter_mut().zip(row) {
                    let index = book
                        .controls
                        .iter()
                        .position(|item| item.id == control.id)
                        .unwrap();
                    ui.push_id(("sheet-control", control.id), |ui| {
                        self.card(
                            ui,
                            state,
                            book,
                            values,
                            control,
                            index,
                            editing,
                            &mut actions,
                        );
                    });
                }
            });
            ui.add_space(8.0);
        }
        actions
    }

    fn editor_ui(&mut self, ui: &mut Ui, book: &Workbook, actions: &mut Vec<ChartAction>) {
        let Some(draft) = &mut self.editor else {
            return;
        };
        let colors = web_theme::colors(ui.ctx());
        let mut save = false;
        let mut cancel = false;
        egui::Frame::default()
            .fill(colors.bg)
            .stroke(Stroke::new(0.6_f32, colors.rule))
            .inner_margin(egui::Margin::same(9))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(if draft.id.is_some() {
                        "Edit control"
                    } else {
                        "New control"
                    })
                    .color(colors.accent),
                );
                if draft.focus {
                    text_input::consume_opening_keys(ui.ctx());
                    ui.memory_mut(|memory| memory.request_focus(egui::Id::new("control-label")));
                    draft.focus = false;
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label("Label");
                    let response = ui.add(
                        TextEdit::singleline(&mut draft.label)
                            .id(egui::Id::new("control-label"))
                            .return_key(None)
                            .char_limit(120)
                            .desired_width(220.0_f32.min(ui.available_width())),
                    );
                    save |= enter(ui, &response);
                    egui::ComboBox::from_id_salt("control-kind")
                        .selected_text(draft.kind.label())
                        .show_ui(ui, |ui| {
                            for kind in [
                                ControlType::Number,
                                ControlType::Slider,
                                ControlType::Result,
                            ] {
                                ui.selectable_value(&mut draft.kind, kind, kind.label());
                            }
                        });
                });
                ui.horizontal_wrapped(|ui| {
                    sheet_choice(ui, "control-source-sheet", book, &mut draft.sheet);
                    ui.label("Cell");
                    let response = ui.add(
                        TextEdit::singleline(&mut draft.cell)
                            .id(egui::Id::new("control-cell"))
                            .return_key(None)
                            .hint_text("A1")
                            .char_limit(20)
                            .desired_width(80.0),
                    );
                    save |= enter(ui, &response);
                });
                if draft.kind == ControlType::Slider {
                    ui.horizontal_wrapped(|ui| {
                        for (label, value) in [
                            ("Min", &mut draft.min),
                            ("Max", &mut draft.max),
                            ("Step", &mut draft.step),
                        ] {
                            ui.label(label);
                            let response = ui.add(
                                TextEdit::singleline(value)
                                    .return_key(None)
                                    .char_limit(64)
                                    .desired_width(80.0),
                            );
                            save |= enter(ui, &response);
                        }
                    });
                }
                if let Some(error) = &self.error {
                    ui.label(RichText::new(error).color(colors.error).size(12.0));
                }
                ui.horizontal(|ui| {
                    save |= ui
                        .button(if draft.id.is_some() {
                            "Save control"
                        } else {
                            "Add control"
                        })
                        .clicked();
                    cancel = ui.small_button("Cancel").clicked();
                });
            });
        ui.add_space(8.0);
        if cancel {
            self.editor = None;
            self.error = None;
        } else if save {
            if let Some(original) = &draft.original {
                if book
                    .controls
                    .iter()
                    .find(|control| control.id == original.id)
                    != Some(original)
                {
                    self.error = Some(
                        "This control changed while you were editing. Cancel and reopen Edit to use its current cell link. Your draft has been kept."
                            .into(),
                    );
                    return;
                }
            }
            match draft.action() {
                Ok(action) => {
                    self.error = None;
                    actions.push(action);
                }
                Err(error) => self.error = Some(error),
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn card(
        &mut self,
        ui: &mut Ui,
        state: &State,
        book: &Workbook,
        values: &Evaluation,
        control: &SheetControl,
        index: usize,
        editing: bool,
        actions: &mut Vec<ChartAction>,
    ) {
        let colors = web_theme::colors(ui.ctx());
        egui::Frame::default()
            .fill(colors.bg)
            .stroke(Stroke::new(0.6_f32, colors.rule))
            .inner_margin(egui::Margin::same(9))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    let label = ui.label(RichText::new(&control.label).strong().color(colors.text));
                    if let Some(target) = &control.target {
                        label.on_hover_text(format!(
                            "{} · {}",
                            book.sheet(target.sheet)
                                .map_or("Missing sheet", |sheet| sheet.name.as_str()),
                            target.cell.label()
                        ));
                    }
                    if editing && ui.small_button("Edit").clicked() {
                        if let Some(draft) = &mut self.editor {
                            draft.focus = true;
                            self.error =
                                Some("Save or cancel the open control editor first.".into());
                        } else {
                            self.editor = Some(ControlDraft::from_control(control));
                            self.error = None;
                        }
                        self.cancel_gesture();
                    }
                    if editing
                        && ui
                            .add_enabled(index > 0, egui::Button::new("↑").small())
                            .on_hover_text("Move control up")
                            .clicked()
                    {
                        actions.push(ChartAction::ControlMove {
                            id: control.id,
                            delta: -1,
                        });
                    }
                    if editing
                        && ui
                            .add_enabled(
                                index + 1 < book.controls.len(),
                                egui::Button::new("↓").small(),
                            )
                            .on_hover_text("Move control down")
                            .clicked()
                    {
                        actions.push(ChartAction::ControlMove {
                            id: control.id,
                            delta: 1,
                        });
                    }
                    if editing
                        && ui
                            .small_button("del")
                            .on_hover_text("Remove control; Undo restores it")
                            .clicked()
                    {
                        actions.push(ChartAction::ControlRemove(control.id));
                    }
                });
                let Some(target) = &control.target else {
                    ui.label(
                        RichText::new("Source cell was removed. Edit to choose a cell.")
                            .color(colors.muted)
                            .size(12.0),
                    );
                    return;
                };
                let Some(sheet) = book.sheet(target.sheet) else {
                    ui.label(
                        RichText::new("Source sheet is missing. Edit to choose a cell.")
                            .color(colors.error)
                            .size(12.0),
                    );
                    return;
                };
                if editing {
                    ui.label(
                        RichText::new(format!("{} · {}", sheet.name, target.cell.label()))
                            .size(11.0)
                            .color(colors.muted),
                    );
                }
                let value = values.value(target.sheet, target.cell);
                if matches!(control.kind, ControlKind::Result) {
                    let style = book.style(target.sheet, target.cell);
                    let text = value.formatted(&style.number_format, state.preferences.decimals);
                    let color = if matches!(value, CellValue::Error(_)) {
                        colors.error
                    } else {
                        style
                            .text_color
                            .map_or(colors.text, |[r, g, b]| egui::Color32::from_rgb(r, g, b))
                    };
                    let mut result = RichText::new(if text.is_empty() { "—" } else { &text })
                        .monospace()
                        .size(24.0)
                        .color(color);
                    if style.bold {
                        result = result.strong();
                    }
                    if style.italic {
                        result = result.italics();
                    }
                    if style.underline {
                        result = result.underline();
                    }
                    if style.strike {
                        result = result.strikethrough();
                    }
                    ui.label(result);
                    return;
                }
                let writable = book.control_input_target(control.id);
                let source = book.raw(target.sheet, target.cell);
                ui.add_enabled_ui(writable.is_ok(), |ui| {
                    self.number_input(ui, control, source, &value, actions);
                    if let ControlKind::Slider { min, max, step } = control.kind {
                        self.slider(
                            ui,
                            control.id,
                            target,
                            source,
                            &value,
                            min,
                            max,
                            step,
                            state.preferences.decimals,
                            actions,
                        );
                    }
                });
                if let Err(error) = writable {
                    self.sliders.remove(&control.id);
                    ui.label(RichText::new(error).size(12.0).color(colors.error));
                }
            });
    }

    fn number_input(
        &mut self,
        ui: &mut Ui,
        control: &SheetControl,
        source: &str,
        value: &CellValue,
        actions: &mut Vec<ChartAction>,
    ) {
        let colors = web_theme::colors(ui.ctx());
        let draft = self
            .numbers
            .entry(control.id)
            .or_insert_with(|| NumberDraft {
                source: source.into(),
                text: value.csv_value(),
                dirty: false,
                error: None,
            });
        if !draft.dirty && draft.source != source {
            draft.source = source.into();
            draft.text = value.csv_value();
            draft.error = None;
        }
        let conflicted = draft.dirty && draft.source != source;
        let mut apply = false;
        ui.horizontal_wrapped(|ui| {
            let response = ui.add(
                TextEdit::singleline(&mut draft.text)
                    .id(egui::Id::new(("control-number-value", control.id)))
                    .return_key(None)
                    .char_limit(64)
                    .hint_text("Value")
                    .desired_width(150.0_f32.min(ui.available_width())),
            );
            if response.changed() {
                draft.dirty = true;
                draft.error = None;
            }
            let submitted = enter(ui, &response);
            apply = ui
                .add_enabled(!conflicted, egui::Button::new("Apply"))
                .clicked()
                || (!conflicted && submitted);
            if draft.dirty && ui.small_button("Reset").clicked() {
                draft.text = value.csv_value();
                draft.source = source.into();
                draft.dirty = false;
                draft.error = None;
            }
        });
        if apply {
            match number(&draft.text) {
                Ok(value) => actions.push(ChartAction::ControlValue {
                    id: control.id,
                    value,
                }),
                Err(error) => draft.error = Some(error),
            }
        }
        if let Some(error) = &draft.error {
            ui.label(RichText::new(error).size(12.0).color(colors.error));
        }
        if conflicted {
            ui.label(
                RichText::new("This cell changed. Reset to its current value before applying.")
                    .size(12.0)
                    .color(colors.error),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn slider(
        &mut self,
        ui: &mut Ui,
        id: u64,
        target: &ControlTarget,
        source: &str,
        current: &CellValue,
        min: f64,
        max: f64,
        step: f64,
        decimals: usize,
        actions: &mut Vec<ChartAction>,
    ) {
        if self.sliders.get(&id).is_some_and(|preview| {
            preview.target.sheet != target.sheet
                || preview.target.cell != target.cell
                || preview.source != source
        }) {
            self.sliders.remove(&id);
            self.cancelled_sliders.insert(id);
        }
        let mut value = self
            .sliders
            .get(&id)
            .map_or_else(
                || match current {
                    CellValue::Number(value) => *value,
                    _ => min,
                },
                |preview| preview.value,
            )
            .clamp(min, max);
        // Slider indices keep steps anchored at min, with a final position for
        // max even when the range is not an exact multiple of the step.
        let last = ((max - min) / step).ceil();
        let mut position = if value == max {
            last
        } else {
            ((value - min) / step).round().clamp(0.0, last)
        };
        let response = ui
            .horizontal(|ui| {
                let response = ui.add(
                    egui::Slider::new(&mut position, 0.0..=last)
                        .step_by(1.0)
                        .integer()
                        .show_value(false),
                );
                if response.changed() {
                    value = if position >= last {
                        max
                    } else {
                        position.mul_add(step, min)
                    };
                    value = ControlKind::Slider { min, max, step }
                        .normalize_value(value)
                        .unwrap_or(min);
                }
                ui.label(RichText::new(format!("{value:.*}", decimals.min(10))).monospace());
                response
            })
            .inner;
        let pointer_down = ui.input(|input| input.pointer.any_down());
        if self.cancelled_sliders.contains(&id) {
            if !pointer_down {
                self.cancelled_sliders.remove(&id);
            }
            return;
        }
        if response.changed() && pointer_down {
            self.sliders.insert(
                id,
                SliderPreview {
                    target: *target,
                    source: source.into(),
                    value,
                },
            );
        } else if response.changed() || (self.sliders.contains_key(&id) && !pointer_down) {
            self.sliders.remove(&id);
            actions.push(ChartAction::ControlValue { id, value });
        }
    }
}

pub fn sheet_choice(ui: &mut Ui, id: &'static str, book: &Workbook, sheet: &mut u64) {
    let name = book
        .sheet(*sheet)
        .map_or("Choose sheet", |sheet| sheet.name.as_str());
    egui::ComboBox::from_id_salt(id)
        .width(150.0_f32.min(ui.available_width()))
        .selected_text(name)
        .show_ui(ui, |ui| {
            for item in &book.sheets {
                ui.selectable_value(sheet, item.id, &item.name);
            }
        });
}

pub fn enter(ui: &Ui, response: &egui::Response) -> bool {
    response.has_focus()
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
}

fn number(text: &str) -> Result<f64, String> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| "Enter a finite number, such as 12.5".into())
}
