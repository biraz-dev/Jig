//! Spreadsheet presentation only. Workbook values and formulas live in the core.
use crate::web_theme::{ACCENT, BG, ERROR, GRID as RULE, MUTED, SELECTED, SURFACE as HEADER, TEXT};
use chrono::{Local, NaiveDate};
use eframe::egui::emath::GuiRounding;
use eframe::egui::{self, Color32, FontId, RichText, Stroke, TextEdit, Ui};
use jig::{
    charts::{ChartKind, ChartSource},
    sheet_formula,
    spreadsheet::{
        self, BorderPreset, CellAddr, CellAlignment, CellFont, CellRange, CellStyle, CellValue,
        Evaluation, NumberFormat, StylePatch, Workbook, MAX_COLS, MAX_ROWS, MAX_SHEETS,
    },
};

const COL_WIDTH: f32 = 112.0;
const ROW_HEIGHT: f32 = 27.0;
const ROW_HEAD: f32 = 44.0;
const COL_HEAD: f32 = 25.0;
const TOOL_ICON_SIZE: f32 = 26.0;
const BORDER_ICON_SIZE: f32 = 28.0;

#[derive(Clone, Copy)]
enum SheetIcon {
    Bold,
    Italic,
    Underline,
    Strike,
    Alignment(CellAlignment),
    TextColor([u8; 3]),
    FillColor([u8; 3]),
    Border(BorderPreset),
}

#[derive(Clone, Debug)]
pub enum SheetAction {
    SetCell {
        sheet: u64,
        addr: CellAddr,
        raw: String,
    },
    SetBlock {
        sheet: u64,
        origin: CellAddr,
        rows: Vec<Vec<String>>,
    },
    Clear {
        sheet: u64,
        range: CellRange,
    },
    AddSheet(String),
    RenameSheet {
        sheet: u64,
        name: String,
    },
    RemoveSheet(u64),
    ImportCsv(u64),
    ExportCsv(u64),
    Decimals(usize),
    PatchStyle {
        sheet: u64,
        range: CellRange,
        patch: StylePatch,
    },
    SetBorders {
        sheet: u64,
        range: CellRange,
        preset: BorderPreset,
    },
    InsertRows {
        sheet: u64,
        at: usize,
        count: usize,
    },
    DeleteRows {
        sheet: u64,
        at: usize,
        count: usize,
    },
    InsertColumns {
        sheet: u64,
        at: usize,
        count: usize,
    },
    DeleteColumns {
        sheet: u64,
        at: usize,
        count: usize,
    },
    Merge {
        sheet: u64,
        range: CellRange,
    },
    Unmerge {
        sheet: u64,
        range: CellRange,
    },
    OpenOds,
    SaveOds,
    ExportPdf(u64),
    ImportPdf(u64),
    ImportJson,
    ExportJson,
    PasteClipboard {
        sheet: u64,
        origin: CellAddr,
    },
    AddChart {
        kind: ChartKind,
        source: ChartSource,
        title: String,
    },
    ShowCharts,
    ShowTasks,
    ShowWorkbench,
    Undo,
}

#[derive(Clone, Copy, PartialEq)]
enum EditOrigin {
    Cell,
    Formula,
}

struct CopiedCells {
    text: String,
    origin: CellAddr,
    rows: Vec<Vec<String>>,
}

/// Selection, text drafts and cached results are transient. The application
/// applies returned actions through its ordinary safe-save and Undo journal.
pub struct SheetPanel {
    active: u64,
    cursor: CellAddr,
    anchor: CellAddr,
    address: String,
    goto_focus: bool,
    grid_focus: bool,
    reveal: bool,
    tab_reveal: bool,
    draft: String,
    original: String,
    editing: Option<EditOrigin>,
    editor_focus: bool,
    renaming: bool,
    rename_focus: bool,
    name: String,
    confirm_delete: bool,
    delete_focus: bool,
    pending_add: Option<usize>,
    values: Option<Evaluation>,
    copied: Option<CopiedCells>,
    error: Option<String>,
    row_offsets: Vec<f32>,
    metrics_sheet: u64,
    expanded_selection: CellRange,
    pending_copy: bool,
    color_menu_key: Option<(u64, CellAddr, CellAddr, bool)>,
    text_color_draft: [u8; 3],
    fill_color_draft: [u8; 3],
    show_gridlines: bool,
    calculated_day: Option<NaiveDate>,
}

impl Default for SheetPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl SheetPanel {
    pub fn new() -> Self {
        Self {
            active: 1,
            cursor: CellAddr::new(0, 0),
            anchor: CellAddr::new(0, 0),
            address: "A1".into(),
            goto_focus: false,
            grid_focus: true,
            reveal: true,
            tab_reveal: true,
            draft: String::new(),
            original: String::new(),
            editing: None,
            editor_focus: false,
            renaming: false,
            rename_focus: false,
            name: String::new(),
            confirm_delete: false,
            delete_focus: false,
            pending_add: None,
            values: None,
            copied: None,
            error: None,
            row_offsets: (0..=MAX_ROWS).map(|row| row as f32 * ROW_HEIGHT).collect(),
            metrics_sheet: 0,
            expanded_selection: CellRange::new(CellAddr::new(0, 0), CellAddr::new(0, 0)),
            pending_copy: false,
            color_menu_key: None,
            text_color_draft: jig::palette::TEXT,
            fill_color_draft: jig::palette::BACKGROUND,
            show_gridlines: true,
            calculated_day: None,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn invalidate(&mut self) {
        self.values = None;
        self.metrics_sheet = 0;
        self.color_menu_key = None;
    }

    pub fn restore_data(&mut self) {
        self.editing = None;
        self.editor_focus = false;
        self.renaming = false;
        self.confirm_delete = false;
        self.pending_add = None;
        self.invalidate();
        self.error = None;
        self.grid_focus = true;
    }

    pub fn request_focus(&mut self) {
        self.grid_focus = true;
        self.reveal = true;
    }

    pub fn active_sheet(&self) -> u64 {
        self.active
    }

    pub fn activate(&mut self, sheet: u64) {
        self.active = sheet;
        self.cursor = CellAddr::new(0, 0);
        self.anchor = self.cursor;
        self.expanded_selection = CellRange::new(self.cursor, self.cursor);
        self.address = "A1".into();
        self.draft.clear();
        self.original.clear();
        self.restore_data();
        self.invalidate();
        self.reveal = true;
        self.tab_reveal = true;
    }

    pub fn selection(&self) -> CellRange {
        self.expanded_selection
    }

    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    pub fn commit_pending(&mut self) -> Option<SheetAction> {
        self.editing.take()?;
        self.editor_focus = false;
        self.grid_focus = true;
        if self.draft == self.original {
            return None;
        }
        let action = SheetAction::SetCell {
            sheet: self.active,
            addr: self.cursor,
            raw: self.draft.clone(),
        };
        Some(action)
    }

    pub fn reject_action(&mut self, action: &SheetAction, error: String) {
        self.error = Some(error);
        self.pending_copy = false;
        if let SheetAction::SetCell { sheet, addr, raw } = action {
            self.active = *sheet;
            self.cursor = *addr;
            self.anchor = *addr;
            self.draft = raw.clone();
            self.editing = Some(EditOrigin::Cell);
            self.editor_focus = true;
            self.grid_focus = false;
            self.reveal = true;
        } else if let SheetAction::RenameSheet { sheet, name } = action {
            self.active = *sheet;
            self.name = name.clone();
            self.renaming = true;
            self.rename_focus = true;
            self.grid_focus = false;
        }
    }

    fn grid_id(&self) -> egui::Id {
        egui::Id::new(("spreadsheet-grid", self.active))
    }

    fn editor_id(&self) -> egui::Id {
        egui::Id::new(("spreadsheet-cell-input", self.active, self.cursor))
    }

    fn begin_edit(&mut self, book: &Workbook, origin: EditOrigin, replacement: Option<String>) {
        self.original = book.raw(self.active, self.cursor).to_owned();
        self.draft = replacement.unwrap_or_else(|| self.original.clone());
        self.editing = Some(origin);
        self.editor_focus = true;
        self.grid_focus = false;
        self.reveal = true;
        self.error = None;
    }

    fn cancel_edit(&mut self) {
        self.draft = self.original.clone();
        self.editing = None;
        self.editor_focus = false;
        self.grid_focus = true;
        self.error = None;
    }

    fn select(&mut self, addr: CellAddr, extend: bool) {
        self.cursor = addr;
        if !extend {
            self.anchor = addr;
        }
        self.expanded_selection = CellRange::new(self.anchor, self.cursor);
        self.address = range_label(self.selection());
        self.reveal = true;
        self.grid_focus = true;
    }

    fn navigate(&mut self, book: &Workbook, row_delta: i32, col_delta: i32, extend: bool) {
        let merged = book.merge_at(self.active, self.cursor);
        let base_row = merged.map_or(self.cursor.row, |range| {
            if row_delta > 0 {
                range.end.row
            } else {
                range.start.row
            }
        });
        let base_col = merged.map_or(self.cursor.col, |range| {
            if col_delta > 0 {
                range.end.col
            } else {
                range.start.col
            }
        });
        let row = (base_row as i32 + row_delta).clamp(0, MAX_ROWS as i32 - 1) as usize;
        let col = (base_col as i32 + col_delta).clamp(0, MAX_COLS as i32 - 1) as usize;
        self.select(CellAddr::new(row, col), extend);
    }

    fn sync_selection(&mut self, book: &Workbook) {
        if let Some(merged) = book.merge_at(self.active, self.cursor) {
            self.cursor = merged.start;
        }
        if let Some(merged) = book.merge_at(self.active, self.anchor) {
            self.anchor = merged.start;
        }
        let mut range = CellRange::new(self.anchor, self.cursor);
        if let Some(sheet) = book.sheet(self.active) {
            loop {
                let previous = range;
                for merge in &sheet.merges {
                    if ranges_intersect(range, *merge) {
                        range = CellRange::new(
                            CellAddr::new(
                                range.start.row.min(merge.start.row),
                                range.start.col.min(merge.start.col),
                            ),
                            CellAddr::new(
                                range.end.row.max(merge.end.row),
                                range.end.col.max(merge.end.col),
                            ),
                        );
                    }
                }
                if range == previous {
                    break;
                }
            }
        }
        self.expanded_selection = range;
    }

    fn geometry(&mut self, book: &Workbook) {
        if self.metrics_sheet == self.active {
            return;
        }
        let mut heights = vec![ROW_HEIGHT; MAX_ROWS];
        if let Some(sheet) = book.sheet(self.active) {
            for (address, style) in &sheet.styles {
                if let Ok(addr) = CellAddr::parse(address) {
                    heights[addr.row] = heights[addr.row].max(f32::from(style.font_size) + 10.0);
                }
            }
        }
        self.row_offsets.clear();
        self.row_offsets.push(0.0);
        for height in heights {
            self.row_offsets
                .push(self.row_offsets.last().copied().unwrap_or(0.0) + height);
        }
        self.metrics_sheet = self.active;
    }

    fn row_at(&self, y: f32) -> usize {
        self.row_offsets
            .partition_point(|offset| *offset <= y.max(0.0))
            .saturating_sub(1)
            .min(MAX_ROWS - 1)
    }

    fn cell_rect(&self, rect: egui::Rect, addr: CellAddr) -> egui::Rect {
        egui::Rect::from_min_size(
            rect.min
                + egui::vec2(
                    ROW_HEAD + addr.col as f32 * COL_WIDTH,
                    COL_HEAD + self.row_offsets[addr.row],
                ),
            egui::vec2(
                COL_WIDTH,
                self.row_offsets[addr.row + 1] - self.row_offsets[addr.row],
            ),
        )
    }

    fn span_rect(&self, rect: egui::Rect, range: CellRange) -> egui::Rect {
        self.cell_rect(rect, range.start)
            .union(self.cell_rect(rect, range.end))
    }

    fn address_at(&self, rect: egui::Rect, pointer: egui::Pos2) -> CellAddr {
        CellAddr::new(
            self.row_at(pointer.y - rect.top() - COL_HEAD),
            ((pointer.x - rect.left() - ROW_HEAD) / COL_WIDTH)
                .floor()
                .clamp(0.0, MAX_COLS as f32 - 1.0) as usize,
        )
    }

    fn commit(&mut self, actions: &mut Vec<SheetAction>) {
        if let Some(action) = self.commit_pending() {
            actions.push(action);
        }
    }

    fn switch_sheet(&mut self, id: u64, actions: &mut Vec<SheetAction>) {
        if self.active == id {
            return;
        }
        self.commit(actions);
        self.active = id;
        self.tab_reveal = true;
        self.cursor = CellAddr::new(0, 0);
        self.anchor = self.cursor;
        self.address = "A1".into();
        self.renaming = false;
        self.confirm_delete = false;
        self.grid_focus = true;
        self.reveal = true;
        self.error = None;
    }

    fn new_sheet(&mut self, book: &Workbook, actions: &mut Vec<SheetAction>) {
        if book.sheets.len() >= MAX_SHEETS {
            self.error = Some(format!("A workbook can have at most {MAX_SHEETS} sheets"));
            return;
        }
        self.commit(actions);
        let mut number = book.sheets.len() + 1;
        let name = loop {
            let name = format!("Sheet {number}");
            if !book
                .sheets
                .iter()
                .any(|sheet| sheet.name.eq_ignore_ascii_case(&name))
            {
                break name;
            }
            number += 1;
        };
        self.pending_add = Some(book.sheets.len());
        actions.push(SheetAction::AddSheet(name));
    }

    pub fn ui(&mut self, ui: &mut Ui, book: &Workbook, decimals: usize) -> Vec<SheetAction> {
        let mut actions = Vec::new();
        if let Some(previous) = self.pending_add.take() {
            if book.sheets.len() > previous {
                if let Some(sheet) = book.sheets.last() {
                    self.switch_sheet(sheet.id, &mut actions);
                }
            }
        }
        if book.sheet(self.active).is_none() {
            if let Some(first) = book.sheets.first() {
                self.active = first.id;
                self.tab_reveal = true;
                self.cursor = CellAddr::new(0, 0);
                self.anchor = self.cursor;
                self.address = "A1".into();
                self.editing = None;
                self.grid_focus = true;
                self.reveal = true;
            }
        }
        let today = Local::now().date_naive();
        if self.values.is_none() || self.calculated_day != Some(today) {
            self.values = Some(book.recalculate_at(today));
            self.calculated_day = Some(today);
        }
        self.geometry(book);
        self.sync_selection(book);
        if self.pending_copy {
            self.pending_copy = false;
            self.copy(ui, book);
        }
        if self.editing.is_none() {
            self.draft = book.raw(self.active, self.cursor).to_owned();
        }
        self.keyboard(ui, book, &mut actions);
        self.geometry(book);
        self.sync_selection(book);
        self.menus(ui, book, &mut actions);
        self.formula_bar(ui, book, &mut actions);
        self.commands(ui, book, &mut actions);
        self.format_toolbar(ui, book, &mut actions);
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).color(ERROR).size(11.0));
        }
        let reserve = if self.renaming || self.confirm_delete {
            98.0
        } else {
            60.0
        };
        let height = (ui.available_height() - reserve).max(150.0);
        self.grid(ui, book, decimals, height, &mut actions);
        self.tabs(ui, book, &mut actions);
        self.summary(ui, decimals, &mut actions);
        actions
    }

    fn keyboard(&mut self, ui: &mut Ui, book: &Workbook, actions: &mut Vec<SheetAction>) {
        if ui.memory(|memory| memory.any_popup_open()) {
            return;
        }
        if !self.renaming {
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::SHIFT, egui::Key::F11)) {
                self.new_sheet(book, actions);
                return;
            }
            let previous =
                ui.input_mut(|input| input.consume_key(egui::Modifiers::ALT, egui::Key::PageUp));
            let next =
                ui.input_mut(|input| input.consume_key(egui::Modifiers::ALT, egui::Key::PageDown));
            if previous || next {
                if let Some(index) = book.sheets.iter().position(|sheet| sheet.id == self.active) {
                    let len = book.sheets.len();
                    let next = (index + if previous { len - 1 } else { 1 }) % len;
                    self.switch_sheet(book.sheets[next].id, actions);
                    ui.ctx().request_repaint();
                }
                return;
            }
        }
        if !self.renaming
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::G))
        {
            self.commit(actions);
            self.goto_focus = true;
            self.grid_focus = false;
            self.address = range_label(self.selection());
            ui.ctx().request_repaint();
            return;
        }
        let focused = ui.memory(|memory| memory.has_focus(self.grid_id()));
        if !focused || self.editing.is_some() || self.renaming || self.confirm_delete {
            return;
        }
        let copied = ui.input_mut(|input| {
            let copied = input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Copy));
            input
                .events
                .retain(|event| !matches!(event, egui::Event::Copy));
            copied || input.consume_key(egui::Modifiers::CTRL, egui::Key::C)
        });
        if copied {
            self.copy(ui, book);
        }
        let pasted = ui.input_mut(|input| {
            let mut pasted = None;
            input.events.retain(|event| {
                if let egui::Event::Paste(text) = event {
                    pasted = Some(text.clone());
                    false
                } else {
                    true
                }
            });
            pasted
        });
        if let Some(text) = pasted {
            match self.paste(&text) {
                Ok(rows) => actions.push(SheetAction::SetBlock {
                    sheet: self.active,
                    origin: self.selection().start,
                    rows,
                }),
                Err(error) => self.error = Some(error),
            }
        }
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::A)) {
            let used = book
                .used_range(self.active)
                .unwrap_or_else(|| CellRange::new(CellAddr::new(0, 0), CellAddr::new(0, 0)));
            self.anchor = used.end;
            self.cursor = used.start;
            self.address = range_label(used);
        }
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Home)) {
            self.select(CellAddr::new(0, 0), false);
        }
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::End)) {
            let addr = book
                .used_range(self.active)
                .map_or(self.cursor, |range| range.end);
            self.select(addr, false);
        }
        if ui.input(|input| input.modifiers.ctrl || input.modifiers.alt || input.modifiers.command)
        {
            return;
        }
        for (key, dr, dc) in [
            (egui::Key::ArrowUp, -1, 0),
            (egui::Key::ArrowDown, 1, 0),
            (egui::Key::ArrowLeft, 0, -1),
            (egui::Key::ArrowRight, 0, 1),
            (egui::Key::PageUp, -15, 0),
            (egui::Key::PageDown, 15, 0),
        ] {
            if let Some(shift) = consume_shift_key(ui, key) {
                self.navigate(book, dr, dc, shift);
            }
        }
        if let Some(shift) = consume_shift_key(ui, egui::Key::Tab) {
            self.navigate(book, 0, if shift { -1 } else { 1 }, false);
        }
        if ui.input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                || input.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
        }) {
            actions.push(SheetAction::Clear {
                sheet: self.active,
                range: self.selection(),
            });
        }
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.anchor = self.cursor;
            self.address = self.cursor.label();
            self.error = None;
        }
        let edit = ui.input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::F2)
                || input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
        });
        if edit {
            self.begin_edit(book, EditOrigin::Cell, None);
            ui.ctx().request_repaint();
            return;
        }
        let text = ui.input_mut(|input| {
            if input.modifiers.ctrl || input.modifiers.alt || input.modifiers.command {
                return None;
            }
            let mut text = String::new();
            input.events.retain(|event| {
                if let egui::Event::Text(value) = event {
                    text.push_str(value);
                    false
                } else {
                    true
                }
            });
            (!text.is_empty()).then_some(text)
        });
        if let Some(text) = text {
            self.begin_edit(book, EditOrigin::Cell, Some(text));
            ui.ctx().request_repaint();
        }
    }

    fn queue(&mut self, actions: &mut Vec<SheetAction>, action: SheetAction) {
        self.commit(actions);
        actions.push(action);
    }

    fn menu_action(
        &mut self,
        ui: &mut Ui,
        actions: &mut Vec<SheetAction>,
        label: &str,
        action: SheetAction,
    ) {
        if ui.button(label).clicked() {
            self.queue(actions, action);
            ui.close_menu();
        }
    }

    fn patch(&mut self, actions: &mut Vec<SheetAction>, patch: StylePatch) {
        self.queue(
            actions,
            SheetAction::PatchStyle {
                sheet: self.active,
                range: self.selection(),
                patch,
            },
        );
    }

    fn menus(&mut self, ui: &mut Ui, book: &Workbook, actions: &mut Vec<SheetAction>) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                self.menu_action(ui, actions, "Open ODS…", SheetAction::OpenOds);
                self.menu_action(ui, actions, "Save ODS", SheetAction::SaveOds);
                ui.separator();
                self.menu_action(
                    ui,
                    actions,
                    "Import PDF or image…",
                    SheetAction::ImportPdf(self.active),
                );
                self.menu_action(
                    ui,
                    actions,
                    "Export PDF",
                    SheetAction::ExportPdf(self.active),
                );
                ui.separator();
                self.menu_action(
                    ui,
                    actions,
                    "Import CSV…",
                    SheetAction::ImportCsv(self.active),
                );
                self.menu_action(
                    ui,
                    actions,
                    "Export CSV values",
                    SheetAction::ExportCsv(self.active),
                );
                ui.separator();
                self.menu_action(
                    ui,
                    actions,
                    "Open Jig JSON backup…",
                    SheetAction::ImportJson,
                );
                self.menu_action(ui, actions, "Save Jig JSON backup", SheetAction::ExportJson);
            });
            ui.menu_button("Edit", |ui| {
                self.menu_action(ui, actions, "Undo     Ctrl+Z", SheetAction::Undo);
                ui.separator();
                if ui.button("Copy cells     Ctrl+C").clicked() {
                    self.commit(actions);
                    self.pending_copy = true;
                    ui.ctx().request_repaint();
                    ui.close_menu();
                }
                self.menu_action(
                    ui,
                    actions,
                    "Paste cells     Ctrl+V",
                    SheetAction::PasteClipboard {
                        sheet: self.active,
                        origin: self.selection().start,
                    },
                );
                self.menu_action(
                    ui,
                    actions,
                    "Clear contents     Delete",
                    SheetAction::Clear {
                        sheet: self.active,
                        range: self.selection(),
                    },
                );
                ui.separator();
                if ui.button("Go to cell or range…     Ctrl+G").clicked() {
                    self.commit(actions);
                    self.goto_focus = true;
                    self.grid_focus = false;
                    ui.close_menu();
                }
            });
            ui.menu_button("Format", |ui| {
                let style = book.style(self.active, self.cursor);
                ui.menu_button("Font", |ui| self.font_choices(ui, &style, actions));
                ui.menu_button("Font size", |ui| self.size_choices(ui, &style, actions));
                if ui.selectable_label(style.bold, "Bold").clicked() {
                    self.patch(
                        actions,
                        StylePatch {
                            bold: Some(!style.bold),
                            ..Default::default()
                        },
                    );
                    ui.close_menu();
                }
                if ui.selectable_label(style.italic, "Italic").clicked() {
                    self.patch(
                        actions,
                        StylePatch {
                            italic: Some(!style.italic),
                            ..Default::default()
                        },
                    );
                    ui.close_menu();
                }
                if ui.selectable_label(style.underline, "Underline").clicked() {
                    self.patch(
                        actions,
                        StylePatch {
                            underline: Some(!style.underline),
                            ..Default::default()
                        },
                    );
                    ui.close_menu();
                }
                if ui.selectable_label(style.strike, "Strikethrough").clicked() {
                    self.patch(
                        actions,
                        StylePatch {
                            strike: Some(!style.strike),
                            ..Default::default()
                        },
                    );
                    ui.close_menu();
                }
                ui.menu_button("Alignment", |ui| {
                    self.alignment_choices(ui, &style, actions)
                });
                ui.separator();
                ui.menu_button("Number format", |ui| {
                    self.number_choices(ui, &style, actions)
                });
                self.color_menu(ui, "Text color", false, &style, actions);
                self.color_menu(ui, "Fill color", true, &style, actions);
                ui.menu_button("Borders", |ui| self.border_choices(ui, actions));
                ui.separator();
                self.menu_action(
                    ui,
                    actions,
                    "Merge cells",
                    SheetAction::Merge {
                        sheet: self.active,
                        range: self.selection(),
                    },
                );
                self.menu_action(
                    ui,
                    actions,
                    "Unmerge cells",
                    SheetAction::Unmerge {
                        sheet: self.active,
                        range: self.selection(),
                    },
                );
                if ui.button("Clear formatting").clicked() {
                    self.patch(
                        actions,
                        StylePatch {
                            font_family: Some(CellFont::Mono),
                            font_size: Some(13),
                            bold: Some(false),
                            italic: Some(false),
                            underline: Some(false),
                            strike: Some(false),
                            alignment: Some(CellAlignment::Auto),
                            text_color: Some(None),
                            fill_color: Some(None),
                            borders: Some(spreadsheet::CellBorders::none()),
                            number_format: Some(NumberFormat::General),
                        },
                    );
                    ui.close_menu();
                }
            });
            ui.menu_button("Insert", |ui| {
                let range = self.selection();
                let sheet = self.active;
                self.menu_action(
                    ui,
                    actions,
                    "Rows above",
                    SheetAction::InsertRows {
                        sheet,
                        at: range.start.row,
                        count: range.rows(),
                    },
                );
                self.menu_action(
                    ui,
                    actions,
                    "Columns before",
                    SheetAction::InsertColumns {
                        sheet,
                        at: range.start.col,
                        count: range.cols(),
                    },
                );
                ui.separator();
                self.menu_action(
                    ui,
                    actions,
                    "Delete selected rows",
                    SheetAction::DeleteRows {
                        sheet,
                        at: range.start.row,
                        count: range.rows(),
                    },
                );
                self.menu_action(
                    ui,
                    actions,
                    "Delete selected columns",
                    SheetAction::DeleteColumns {
                        sheet,
                        at: range.start.col,
                        count: range.cols(),
                    },
                );
                ui.separator();
                let source_range = if range.start == range.end {
                    book.used_range(self.active).unwrap_or(range)
                } else {
                    range
                };
                let source = ChartSource::Range {
                    sheet,
                    range: source_range,
                };
                self.menu_action(
                    ui,
                    actions,
                    "Pie chart · labels and values",
                    SheetAction::AddChart {
                        kind: ChartKind::Pie,
                        source,
                        title: "Pie chart".into(),
                    },
                );
                self.menu_action(
                    ui,
                    actions,
                    "Bar chart · labels and values",
                    SheetAction::AddChart {
                        kind: ChartKind::Bar,
                        source,
                        title: "Bar chart".into(),
                    },
                );
                self.menu_action(
                    ui,
                    actions,
                    "Gantt chart · labels, starts and due dates",
                    SheetAction::AddChart {
                        kind: ChartKind::Gantt,
                        source,
                        title: "Gantt chart".into(),
                    },
                );
                self.menu_action(
                    ui,
                    actions,
                    "Gantt chart from Jig tasks",
                    SheetAction::AddChart {
                        kind: ChartKind::Gantt,
                        source: ChartSource::Tasks,
                        title: "Task timeline".into(),
                    },
                );
                ui.separator();
                if ui.button("New sheet     Shift+F11").clicked() {
                    self.new_sheet(book, actions);
                    ui.close_menu();
                }
            });
            ui.menu_button("View", |ui| {
                self.menu_action(ui, actions, "Tasks", SheetAction::ShowTasks);
                self.menu_action(ui, actions, "Charts", SheetAction::ShowCharts);
                self.menu_action(ui, actions, "Workbench", SheetAction::ShowWorkbench);
                ui.separator();
                ui.checkbox(&mut self.show_gridlines, "Gridlines");
            });
        });
    }

    fn font_choices(&mut self, ui: &mut Ui, style: &CellStyle, actions: &mut Vec<SheetAction>) {
        for (font, label) in [
            (CellFont::Mono, "Mono"),
            (CellFont::Sans, "Sans"),
            (CellFont::Serif, "Serif"),
        ] {
            if ui
                .selectable_label(style.font_family == font, label)
                .clicked()
            {
                self.patch(
                    actions,
                    StylePatch {
                        font_family: Some(font),
                        ..Default::default()
                    },
                );
                ui.close_menu();
            }
        }
    }

    fn size_choices(&mut self, ui: &mut Ui, style: &CellStyle, actions: &mut Vec<SheetAction>) {
        for size in 9..=32_u8 {
            if ui
                .selectable_label(style.font_size == size, size.to_string())
                .clicked()
            {
                self.patch(
                    actions,
                    StylePatch {
                        font_size: Some(size),
                        ..Default::default()
                    },
                );
                ui.close_menu();
            }
        }
    }

    fn number_choices(&mut self, ui: &mut Ui, style: &CellStyle, actions: &mut Vec<SheetAction>) {
        for (format, label) in [
            (NumberFormat::General, "General"),
            (NumberFormat::Number, "Number"),
            (NumberFormat::Percent, "Percent"),
            (NumberFormat::Currency("USD".into()), "Currency · USD"),
            (NumberFormat::Date, "Date"),
        ] {
            if ui
                .selectable_label(style.number_format == format, label)
                .clicked()
            {
                self.patch(
                    actions,
                    StylePatch {
                        number_format: Some(format),
                        ..Default::default()
                    },
                );
                ui.close_menu();
            }
        }
    }

    fn alignment_choices(
        &mut self,
        ui: &mut Ui,
        style: &CellStyle,
        actions: &mut Vec<SheetAction>,
    ) {
        for (alignment, label) in [
            (CellAlignment::Auto, "Auto · numbers right, text left"),
            (CellAlignment::Left, "Left"),
            (CellAlignment::Center, "Center"),
            (CellAlignment::Right, "Right"),
        ] {
            if ui
                .selectable_label(style.alignment == alignment, label)
                .clicked()
            {
                self.patch(
                    actions,
                    StylePatch {
                        alignment: Some(alignment),
                        ..Default::default()
                    },
                );
                ui.close_menu();
            }
        }
    }

    fn border_choices(&mut self, ui: &mut Ui, actions: &mut Vec<SheetAction>) {
        ui.set_min_width(BORDER_ICON_SIZE * 5.0 + 6.0 * 4.0);
        ui.label("Borders");
        let mut caption = "Choose borders";
        egui::Grid::new("spreadsheet-border-presets")
            .num_columns(5)
            .min_col_width(BORDER_ICON_SIZE)
            .min_row_height(BORDER_ICON_SIZE)
            .spacing(egui::vec2(6.0, 6.0))
            .show(ui, |ui| {
                for (index, (preset, label)) in [
                    (BorderPreset::None, "No borders"),
                    (BorderPreset::All, "All borders"),
                    (BorderPreset::Outer, "Outside border"),
                    (BorderPreset::Inner, "Inner borders"),
                    (BorderPreset::Horizontal, "Horizontal borders"),
                    (BorderPreset::Vertical, "Vertical borders"),
                    (BorderPreset::Top, "Top border"),
                    (BorderPreset::Bottom, "Bottom border"),
                    (BorderPreset::Left, "Left border"),
                    (BorderPreset::Right, "Right border"),
                ]
                .into_iter()
                .enumerate()
                {
                    let response = sheet_icon_button(
                        ui,
                        SheetIcon::Border(preset),
                        label,
                        false,
                        BORDER_ICON_SIZE,
                    );
                    if response.hovered() || response.has_focus() {
                        caption = label;
                    }
                    if response.clicked() {
                        self.queue(
                            actions,
                            SheetAction::SetBorders {
                                sheet: self.active,
                                range: self.selection(),
                                preset,
                            },
                        );
                        ui.close_menu();
                    }
                    if index % 5 == 4 {
                        ui.end_row();
                    }
                }
            });
        ui.label(RichText::new(caption).color(MUTED));
    }

    fn color_menu(
        &mut self,
        ui: &mut Ui,
        label: &str,
        fill: bool,
        style: &CellStyle,
        actions: &mut Vec<SheetAction>,
    ) {
        ui.menu_button(label, |ui| {
            self.color_choices(ui, label, fill, style, actions)
        });
    }

    fn color_choices(
        &mut self,
        ui: &mut Ui,
        label: &str,
        fill: bool,
        style: &CellStyle,
        actions: &mut Vec<SheetAction>,
    ) {
        let range = self.selection();
        let key = (self.active, range.start, range.end, fill);
        if self.color_menu_key != Some(key) {
            let color = if fill {
                style.fill_color.unwrap_or(jig::palette::BACKGROUND)
            } else {
                style.text_color.unwrap_or(jig::palette::TEXT)
            };
            if fill {
                self.fill_color_draft = color;
            } else {
                self.text_color_draft = color;
            }
            self.color_menu_key = Some(key);
        }
        ui.horizontal_wrapped(|ui| {
            for (color, name) in [
                ([246, 218, 174], "Cream"),
                ([234, 133, 173], "Rose"),
                ([218, 185, 126], "Amber"),
                ([148, 188, 160], "Sage"),
                ([109, 185, 238], "Blue"),
                ([179, 158, 207], "Lilac"),
                ([245, 245, 245], "White"),
                ([28, 28, 28], "Black"),
            ] {
                let response = ui.add_sized([22.0, 22.0], egui::Button::new(" ").fill(rgb(color)));
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        ui.is_enabled(),
                        format!("{label}: {name}"),
                    )
                });
                if response.on_hover_text(name).clicked() {
                    let mut patch = StylePatch::default();
                    if fill {
                        patch.fill_color = Some(Some(color));
                    } else {
                        patch.text_color = Some(Some(color));
                    }
                    self.patch(actions, patch);
                    ui.close_menu();
                }
            }
        });
        let draft = if fill {
            &mut self.fill_color_draft
        } else {
            &mut self.text_color_draft
        };
        ui.horizontal(|ui| {
            ui.label("Custom");
            ui.color_edit_button_srgb(draft);
        });
        if ui.button("Apply custom color").clicked() {
            let color = if fill {
                self.fill_color_draft
            } else {
                self.text_color_draft
            };
            let mut patch = StylePatch::default();
            if fill {
                patch.fill_color = Some(Some(color));
            } else {
                patch.text_color = Some(Some(color));
            }
            self.patch(actions, patch);
            ui.close_menu();
        }
        if ui.button("Default").clicked() {
            let mut patch = StylePatch::default();
            if fill {
                patch.fill_color = Some(None);
            } else {
                patch.text_color = Some(None);
            }
            self.patch(actions, patch);
            ui.close_menu();
        }
    }

    fn format_toolbar(&mut self, ui: &mut Ui, book: &Workbook, actions: &mut Vec<SheetAction>) {
        let style = book.style(self.active, self.cursor);
        ui.horizontal_wrapped(|ui| {
            ui.menu_button(font_label(style.font_family), |ui| {
                self.font_choices(ui, &style, actions)
            });
            ui.menu_button(style.font_size.to_string(), |ui| {
                self.size_choices(ui, &style, actions)
            });
            for (icon, name, selected, property) in [
                (SheetIcon::Bold, "Bold", style.bold, 0),
                (SheetIcon::Italic, "Italic", style.italic, 1),
                (SheetIcon::Underline, "Underline", style.underline, 2),
                (SheetIcon::Strike, "Strikethrough", style.strike, 3),
            ] {
                if sheet_icon_button(ui, icon, name, selected, TOOL_ICON_SIZE).clicked() {
                    let mut patch = StylePatch::default();
                    match property {
                        0 => patch.bold = Some(!selected),
                        1 => patch.italic = Some(!selected),
                        2 => patch.underline = Some(!selected),
                        _ => patch.strike = Some(!selected),
                    }
                    self.patch(actions, patch);
                }
            }
            for (name, alignment) in [
                ("Align left", CellAlignment::Left),
                ("Align center", CellAlignment::Center),
                ("Align right", CellAlignment::Right),
            ] {
                if sheet_icon_button(
                    ui,
                    SheetIcon::Alignment(alignment),
                    name,
                    style.alignment == alignment,
                    TOOL_ICON_SIZE,
                )
                .clicked()
                {
                    self.patch(
                        actions,
                        StylePatch {
                            alignment: Some(alignment),
                            ..Default::default()
                        },
                    );
                }
            }
            sheet_icon_menu(
                ui,
                SheetIcon::TextColor(style.text_color.unwrap_or(jig::palette::TEXT)),
                "Text color",
                |ui| self.color_choices(ui, "Text color", false, &style, actions),
            );
            sheet_icon_menu(
                ui,
                SheetIcon::FillColor(style.fill_color.unwrap_or(jig::palette::BACKGROUND)),
                "Fill color",
                |ui| self.color_choices(ui, "Fill color", true, &style, actions),
            );
            sheet_icon_menu(ui, SheetIcon::Border(BorderPreset::All), "Borders", |ui| {
                self.border_choices(ui, actions)
            });
            ui.menu_button(format_label(&style.number_format), |ui| {
                self.number_choices(ui, &style, actions);
            });
            if ui
                .button("Merge")
                .on_hover_text("Merge selected cells; existing data is preserved")
                .clicked()
            {
                self.queue(
                    actions,
                    SheetAction::Merge {
                        sheet: self.active,
                        range: self.selection(),
                    },
                );
            }
            if ui.button("Unmerge").clicked() {
                self.queue(
                    actions,
                    SheetAction::Unmerge {
                        sheet: self.active,
                        range: self.selection(),
                    },
                );
            }
        });
    }

    fn formula_bar(&mut self, ui: &mut Ui, book: &Workbook, actions: &mut Vec<SheetAction>) {
        ui.horizontal(|ui| {
            let id = egui::Id::new("spreadsheet-address-input");
            if self.goto_focus {
                ui.memory_mut(|memory| memory.request_focus(id));
                self.goto_focus = false;
                ui.ctx().request_repaint();
            }
            if !ui.memory(|memory| memory.has_focus(id)) {
                self.address = range_label(self.selection());
            }
            let address = ui
                .add(
                    TextEdit::singleline(&mut self.address)
                        .id(id)
                        .return_key(None)
                        .desired_width(76.0),
                )
                .on_hover_text("Go to a cell or range · Ctrl+G");
            let address_escape = (address.has_focus() || address.lost_focus())
                && ui
                    .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if address_escape {
                self.address = range_label(self.selection());
                self.grid_focus = true;
                ui.memory_mut(|memory| memory.surrender_focus(id));
            } else if address.has_focus()
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
            {
                match parse_range(&self.address) {
                    Ok(range) => {
                        self.commit(actions);
                        self.anchor = range.start;
                        self.cursor = range.end;
                        self.sync_selection(book);
                        self.grid_focus = true;
                        self.reveal = true;
                        self.error = None;
                        ui.memory_mut(|memory| memory.surrender_focus(id));
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            ui.label(RichText::new("fx").color(ACCENT));
            let field = egui::Id::new(("spreadsheet-formula-input", self.active, self.cursor));
            let width = (ui.available_width() - 58.0).max(60.0);
            if self.editing == Some(EditOrigin::Cell) {
                let mut text = self.draft.clone();
                ui.add_enabled(
                    false,
                    TextEdit::singleline(&mut text)
                        .id(field)
                        .desired_width(width),
                );
            } else {
                let response = ui.add(
                    TextEdit::singleline(&mut self.draft)
                        .id(field)
                        .return_key(None)
                        .hint_text("Value or =formula")
                        .desired_width(width),
                );
                if response.gained_focus() && self.editing.is_none() {
                    self.original = book.raw(self.active, self.cursor).to_owned();
                    self.editing = Some(EditOrigin::Formula);
                    self.error = None;
                }
                let escape = (response.has_focus() || response.lost_focus())
                    && ui.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                    });
                let enter = response.has_focus()
                    && ui.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                    });
                if escape {
                    self.cancel_edit();
                    ui.memory_mut(|memory| memory.surrender_focus(field));
                } else if enter
                    || response.lost_focus() && self.editing == Some(EditOrigin::Formula)
                {
                    self.commit(actions);
                    if !enter {
                        // Tab or a pointer chose another control. Commit the
                        // formula without stealing that control's focus.
                        self.grid_focus = false;
                    }
                }
            }
            if ui.button("Enter").clicked() {
                self.commit(actions);
            }
        });
    }

    fn commands(&mut self, ui: &mut Ui, _book: &Workbook, actions: &mut Vec<SheetAction>) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Import CSV")
                .on_hover_text("Import CSV at the selected cell")
                .clicked()
            {
                self.commit(actions);
                actions.push(SheetAction::ImportCsv(self.active));
            }
            if ui.button("Export CSV").on_hover_text(
                "Export this sheet's used cells as evaluated values. JSON backups retain formulas.",
            ).clicked() {
                self.commit(actions);
                actions.push(SheetAction::ExportCsv(self.active));
            }
        });
    }

    fn grid(
        &mut self,
        ui: &mut Ui,
        book: &Workbook,
        decimals: usize,
        height: f32,
        actions: &mut Vec<SheetAction>,
    ) {
        let active = self.active;
        egui::ScrollArea::both()
            .id_salt(("spreadsheet-scroll", active))
            .max_height(height)
            .drag_to_scroll(ui.input(|input| input.any_touches()))
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, _viewport| {
                let size = egui::vec2(
                    ROW_HEAD + MAX_COLS as f32 * COL_WIDTH,
                    COL_HEAD + self.row_offsets[MAX_ROWS],
                );
                let id = self.grid_id();
                if self.grid_focus && self.editing.is_none() && !self.renaming {
                    ui.memory_mut(|memory| memory.request_focus(id));
                    self.grid_focus = false;
                    ui.ctx().request_repaint();
                }
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let visible = rect.intersect(ui.clip_rect());
                let body = egui::Rect::from_min_max(
                    visible.min + egui::vec2(ROW_HEAD, COL_HEAD),
                    visible.max,
                );
                let response = ui.interact(body, id, egui::Sense::click_and_drag());
                if response.has_focus() {
                    ui.memory_mut(|memory| {
                        memory.set_focus_lock_filter(
                            id,
                            egui::EventFilter {
                                horizontal_arrows: true,
                                vertical_arrows: true,
                                tab: true,
                                escape: true,
                            },
                        )
                    });
                }
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        ui.is_enabled(),
                        format!(
                            "Spreadsheet {}, selected {}, value {}",
                            book.sheet(active).map_or("", |sheet| sheet.name.as_str()),
                            range_label(self.selection()),
                            self.values
                                .as_ref()
                                .map(|values| values.value(active, self.cursor).display(decimals))
                                .unwrap_or_default()
                        ),
                    )
                });
                if response.clicked() || response.double_clicked() || response.drag_started() {
                    if let Some(pointer) = ui.input(|input| input.pointer.interact_pos()) {
                        self.commit(actions);
                        let extend = ui.input(|input| {
                            input
                                .events
                                .iter()
                                .rev()
                                .find_map(|event| match event {
                                    egui::Event::PointerButton {
                                        button: egui::PointerButton::Primary,
                                        modifiers,
                                        ..
                                    } => Some(modifiers.shift),
                                    _ => None,
                                })
                                .unwrap_or(input.modifiers.shift)
                        });
                        if body.contains(pointer) {
                            let addr = self.address_at(rect, pointer);
                            self.cursor = addr;
                            if !extend {
                                self.anchor = addr;
                            }
                            if response.double_clicked() {
                                let pending =
                                    actions.iter().rev().find_map(|action| match action {
                                        SheetAction::SetCell {
                                            sheet,
                                            addr: changed,
                                            raw,
                                        } if *sheet == active && *changed == addr => {
                                            Some(raw.clone())
                                        }
                                        _ => None,
                                    });
                                self.begin_edit(book, EditOrigin::Cell, pending);
                            }
                        }
                        self.sync_selection(book);
                        self.address = range_label(self.selection());
                        if self.editing.is_none() {
                            response.request_focus();
                        }
                    }
                }
                if response.dragged() && self.editing.is_none() {
                    if let Some(pointer) = ui.input(|input| input.pointer.interact_pos()) {
                        self.cursor = self.address_at(rect, pointer);
                        self.sync_selection(book);
                        self.address = range_label(self.selection());
                    }
                }
                let first_col = ((body.left() - rect.left() - ROW_HEAD) / COL_WIDTH)
                    .floor()
                    .max(0.0) as usize;
                let last_col =
                    (((body.right() - rect.left() - ROW_HEAD) / COL_WIDTH).ceil() as usize + 1)
                        .min(MAX_COLS);
                let first_row = self.row_at(body.top() - rect.top() - COL_HEAD);
                let last_row =
                    (self.row_at(body.bottom() - rect.top() - COL_HEAD) + 2).min(MAX_ROWS);
                let painter = ui.painter().with_clip_rect(body);
                painter.rect_filled(body, 0.0, BG);
                let selection = self.selection();
                let mut drawn = std::collections::HashSet::new();
                for row in first_row..last_row {
                    for col in first_col..last_col {
                        let addr = CellAddr::new(row, col);
                        let span = book
                            .merge_at(active, addr)
                            .unwrap_or_else(|| CellRange::new(addr, addr));
                        if !drawn.insert(span.start) {
                            continue;
                        }
                        let cell = self.span_rect(rect, span);
                        let style = book.rendered_style(active, span.start);
                        let fill = style.fill_color.map(rgb).unwrap_or_else(|| {
                            if ranges_intersect(selection, span) {
                                SELECTED
                            } else {
                                BG
                            }
                        });
                        painter.rect_filled(cell, 0.0, fill);
                        if self.show_gridlines {
                            painter.hline(
                                cell.x_range(),
                                cell.bottom() - 0.5,
                                Stroke::new(1.0_f32, RULE),
                            );
                            painter.vline(
                                cell.right() - 0.5,
                                cell.y_range(),
                                Stroke::new(1.0_f32, RULE),
                            );
                        }
                        let value = self
                            .values
                            .as_ref()
                            .map(|values| values.value(active, span.start))
                            .unwrap_or(CellValue::Blank);
                        let number = matches!(value, CellValue::Number(_) | CellValue::Date(_));
                        let error = matches!(value, CellValue::Error(_));
                        let text = value.formatted(&style.number_format, decimals);
                        let color =
                            style
                                .text_color
                                .map(rgb)
                                .unwrap_or(if error { ERROR } else { TEXT });
                        let mut job = egui::text::LayoutJob::simple_singleline(
                            text,
                            cell_font(&style),
                            color,
                        );
                        if let Some(section) = job.sections.first_mut() {
                            section.format.italics = style.italic;
                            if style.underline {
                                section.format.underline = Stroke::new(0.9_f32, color);
                            }
                            if style.strike {
                                section.format.strikethrough = Stroke::new(0.9_f32, color);
                            }
                        }
                        let mut galley = ui.fonts(|fonts| fonts.layout_job(job.clone()));
                        if number && galley.size().x > cell.width() - 12.0 {
                            let full_value = job.text.clone();
                            job.text = "####".into();
                            loop {
                                if let Some(section) = job.sections.first_mut() {
                                    section.byte_range = 0..job.text.len();
                                }
                                galley = ui.fonts(|fonts| fonts.layout_job(job.clone()));
                                if galley.size().x <= cell.width() - 12.0 || job.text.len() == 1 {
                                    break;
                                }
                                job.text.pop();
                            }
                            ui.interact(
                                cell.intersect(body),
                                ui.id().with(("sheet-width-value", span.start)),
                                egui::Sense::hover(),
                            )
                            .on_hover_text(full_value);
                        }
                        let position = egui::pos2(
                            match style.alignment {
                                CellAlignment::Auto if number => {
                                    cell.right() - 6.0 - galley.size().x
                                }
                                CellAlignment::Auto | CellAlignment::Left => cell.left() + 6.0,
                                CellAlignment::Center => cell.center().x - galley.size().x * 0.5,
                                CellAlignment::Right => cell.right() - 6.0 - galley.size().x,
                            },
                            cell.center().y - galley.size().y * 0.5,
                        );
                        let text_painter =
                            painter.with_clip_rect(cell.shrink2(egui::vec2(5.0, 1.0)));
                        text_painter.galley(position, galley.clone(), color);
                        if style.bold {
                            text_painter.galley(position + egui::vec2(0.5, 0.0), galley, color);
                        }
                        let border = Stroke::new(1.1_f32, MUTED);
                        if style.borders.top {
                            painter.hline(cell.x_range(), cell.top(), border);
                        }
                        if style.borders.bottom {
                            painter.hline(cell.x_range(), cell.bottom(), border);
                        }
                        if style.borders.left {
                            painter.vline(cell.left(), cell.y_range(), border);
                        }
                        if style.borders.right {
                            painter.vline(cell.right(), cell.y_range(), border);
                        }
                    }
                }
                let selected = self.span_rect(
                    rect,
                    book.merge_at(active, self.cursor)
                        .unwrap_or_else(|| CellRange::new(self.cursor, self.cursor)),
                );
                if selected.intersects(body) {
                    let clip = painter.with_clip_rect(body);
                    clip.hline(
                        selected.x_range(),
                        selected.top(),
                        Stroke::new(1.0_f32, ACCENT),
                    );
                    clip.hline(
                        selected.x_range(),
                        selected.bottom(),
                        Stroke::new(1.0_f32, ACCENT),
                    );
                    clip.vline(
                        selected.left(),
                        selected.y_range(),
                        Stroke::new(1.0_f32, ACCENT),
                    );
                    clip.vline(
                        selected.right(),
                        selected.y_range(),
                        Stroke::new(1.0_f32, ACCENT),
                    );
                }
                self.headers(
                    ui, book, rect, visible, first_row, last_row, first_col, last_col, actions,
                );
                if self.reveal {
                    ui.scroll_to_rect(
                        selected.expand2(egui::vec2(ROW_HEAD + 4.0, COL_HEAD + 4.0)),
                        None,
                    );
                    self.reveal = false;
                    ui.ctx().request_repaint();
                }
                if self.editing == Some(EditOrigin::Cell) {
                    let editor = self.editor_id();
                    if self.editor_focus {
                        ui.memory_mut(|memory| memory.request_focus(editor));
                        self.editor_focus = false;
                        ui.ctx().request_repaint();
                    }
                    let response = ui.put(
                        selected.shrink(1.0),
                        TextEdit::singleline(&mut self.draft)
                            .id(editor)
                            .lock_focus(true)
                            .return_key(None)
                            .font(FontId::monospace(13.0)),
                    );
                    let escape = (response.has_focus() || response.lost_focus())
                        && ui.input_mut(|input| {
                            input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                        });
                    let enter = response
                        .has_focus()
                        .then(|| consume_shift_key(ui, egui::Key::Enter))
                        .flatten();
                    let tab = response
                        .has_focus()
                        .then(|| consume_shift_key(ui, egui::Key::Tab))
                        .flatten();
                    if escape {
                        self.cancel_edit();
                        ui.memory_mut(|memory| memory.surrender_focus(editor));
                    } else if enter.is_some() || tab.is_some() {
                        self.commit(actions);
                        self.navigate(
                            book,
                            enter.map_or(0, |shift| if shift { -1 } else { 1 }),
                            tab.map_or(0, |shift| if shift { -1 } else { 1 }),
                            false,
                        );
                    } else if response.lost_focus() {
                        self.commit(actions);
                        self.grid_focus = false;
                    }
                }
            });
    }

    #[allow(clippy::too_many_arguments)]
    fn headers(
        &mut self,
        ui: &mut Ui,
        book: &Workbook,
        rect: egui::Rect,
        visible: egui::Rect,
        first_row: usize,
        last_row: usize,
        first_col: usize,
        last_col: usize,
        actions: &mut Vec<SheetAction>,
    ) {
        let top = egui::Rect::from_min_max(
            visible.min,
            egui::pos2(visible.right(), visible.top() + COL_HEAD),
        );
        let left = egui::Rect::from_min_max(
            visible.min,
            egui::pos2(visible.left() + ROW_HEAD, visible.bottom()),
        );
        let menu_id = ui.id().with(("spreadsheet-header-menu", self.active));
        let mut menu = egui::menu::BarState::load(ui.ctx(), menu_id);
        let had_menu = menu.is_some();
        let selection = self.selection();
        let rows_selected = selection.start.col == 0 && selection.end.col == MAX_COLS - 1;
        let columns_selected = selection.start.row == 0 && selection.end.row == MAX_ROWS - 1;
        let keyboard_menu = self.editing.is_none()
            && (rows_selected || columns_selected)
            && ui.memory(|memory| memory.has_focus(self.grid_id()))
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::SHIFT, egui::Key::F10));
        let painter = ui.painter().with_clip_rect(visible);
        painter.rect_filled(top, 0.0, HEADER);
        painter.rect_filled(left, 0.0, HEADER);
        for col in first_col..last_col {
            let x = rect.left() + ROW_HEAD + col as f32 * COL_WIDTH;
            let cell = egui::Rect::from_min_size(
                egui::pos2(x, top.top()),
                egui::vec2(COL_WIDTH, COL_HEAD),
            );
            let clip = egui::Rect::from_min_max(egui::pos2(left.right(), top.top()), top.max);
            let response = ui.interact(
                cell.intersect(clip),
                ui.id()
                    .with(("spreadsheet-column-header", self.active, col)),
                egui::Sense::click(),
            );
            self.header_interaction(
                ui,
                book,
                &response,
                false,
                col,
                keyboard_menu
                    && !rows_selected
                    && col == self.cursor.col.clamp(first_col, last_col.saturating_sub(1)),
                &mut menu,
                actions,
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    format!(
                        "Column {}. Right-click for insert and delete",
                        column_label(col)
                    ),
                )
            });
            let clipped = painter.with_clip_rect(clip);
            if col >= self.selection().start.col && col <= self.selection().end.col {
                clipped.rect_filled(cell, 0.0, SELECTED);
            }
            clipped.text(
                cell.center(),
                egui::Align2::CENTER_CENTER,
                column_label(col),
                FontId::monospace(11.0),
                MUTED,
            );
            clipped.vline(cell.right(), cell.y_range(), Stroke::new(0.5_f32, RULE));
        }
        for row in first_row..last_row {
            let y = rect.top() + COL_HEAD + self.row_offsets[row];
            let cell = egui::Rect::from_min_size(
                egui::pos2(left.left(), y),
                egui::vec2(ROW_HEAD, self.row_offsets[row + 1] - self.row_offsets[row]),
            );
            let clip = egui::Rect::from_min_max(egui::pos2(left.left(), top.bottom()), left.max);
            let response = ui.interact(
                cell.intersect(clip),
                ui.id().with(("spreadsheet-row-header", self.active, row)),
                egui::Sense::click(),
            );
            self.header_interaction(
                ui,
                book,
                &response,
                true,
                row,
                keyboard_menu
                    && rows_selected
                    && row == self.cursor.row.clamp(first_row, last_row.saturating_sub(1)),
                &mut menu,
                actions,
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    format!("Row {}. Right-click for insert and delete", row + 1),
                )
            });
            let clipped = painter.with_clip_rect(clip);
            if row >= self.selection().start.row && row <= self.selection().end.row {
                clipped.rect_filled(cell, 0.0, SELECTED);
            }
            clipped.text(
                cell.center(),
                egui::Align2::CENTER_CENTER,
                (row + 1).to_string(),
                FontId::monospace(11.0),
                MUTED,
            );
            clipped.hline(cell.x_range(), cell.bottom(), Stroke::new(0.5_f32, RULE));
        }
        let corner = egui::Rect::from_min_size(visible.min, egui::vec2(ROW_HEAD, COL_HEAD));
        if ui
            .interact(
                corner,
                ui.id().with(("spreadsheet-select-all", self.active)),
                egui::Sense::click(),
            )
            .on_hover_text("Select all cells")
            .clicked()
        {
            self.commit(actions);
            self.anchor = CellAddr::new(MAX_ROWS - 1, MAX_COLS - 1);
            self.cursor = CellAddr::new(0, 0);
            self.sync_selection(book);
            self.address = range_label(self.selection());
            ui.memory_mut(|memory| memory.request_focus(self.grid_id()));
        }
        painter.rect_filled(corner, 0.0, HEADER);
        painter.hline(top.x_range(), top.bottom(), Stroke::new(1.0_f32, RULE));
        painter.vline(left.right(), left.y_range(), Stroke::new(1.0_f32, RULE));
        if had_menu && menu.is_none() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.grid_focus = true;
        }
        menu.store(ui.ctx(), menu_id);
    }

    #[allow(clippy::too_many_arguments)]
    fn header_interaction(
        &mut self,
        ui: &mut Ui,
        book: &Workbook,
        response: &egui::Response,
        rows: bool,
        index: usize,
        keyboard_open: bool,
        menu: &mut egui::menu::BarState,
        actions: &mut Vec<SheetAction>,
    ) {
        if response.clicked() || response.secondary_clicked() {
            self.commit(actions);
            let range = self.selection();
            let already_selected = if rows {
                range.start.col == 0
                    && range.end.col == MAX_COLS - 1
                    && (range.start.row..=range.end.row).contains(&index)
            } else {
                range.start.row == 0
                    && range.end.row == MAX_ROWS - 1
                    && (range.start.col..=range.end.col).contains(&index)
            };
            if response.clicked() || !already_selected {
                let extend = response.clicked()
                    && ui.input(|input| {
                        input
                            .events
                            .iter()
                            .rev()
                            .find_map(|event| match event {
                                egui::Event::PointerButton {
                                    button: egui::PointerButton::Primary,
                                    modifiers,
                                    ..
                                } => Some(modifiers.shift),
                                _ => None,
                            })
                            .unwrap_or(input.modifiers.shift)
                    });
                if rows {
                    let anchor = if extend { self.anchor.row } else { index };
                    self.anchor = CellAddr::new(anchor, MAX_COLS - 1);
                    self.cursor = CellAddr::new(index, 0);
                } else {
                    let anchor = if extend { self.anchor.col } else { index };
                    self.anchor = CellAddr::new(MAX_ROWS - 1, anchor);
                    self.cursor = CellAddr::new(0, index);
                }
                self.sync_selection(book);
                self.address = range_label(self.selection());
            }
            if response.secondary_clicked() {
                response.request_focus();
                self.grid_focus = false;
            } else {
                ui.memory_mut(|memory| memory.request_focus(self.grid_id()));
            }
        }
        // Use egui's ordinary context menu machinery for pointer and touch,
        // with the same popup available from a selected header via Shift+F10.
        egui::menu::MenuRoot::context_click_interaction(response, menu);
        if keyboard_open {
            egui::menu::MenuRoot::handle_menu_response(
                menu,
                egui::menu::MenuResponse::Create(response.rect.left_bottom(), response.id),
            );
            response.request_focus();
            self.grid_focus = false;
        }
        menu.show(response, |ui| {
            self.header_choices(ui, rows, keyboard_open, actions)
        });
    }

    fn header_choices(
        &mut self,
        ui: &mut Ui,
        rows: bool,
        mut focus_first: bool,
        actions: &mut Vec<SheetAction>,
    ) {
        let range = self.selection();
        let (first, last, count, limit, noun, before, after) = if rows {
            (
                range.start.row,
                range.end.row,
                range.rows(),
                MAX_ROWS,
                "row",
                "above",
                "below",
            )
        } else {
            (
                range.start.col,
                range.end.col,
                range.cols(),
                MAX_COLS,
                "column",
                "left",
                "right",
            )
        };
        let target = if count == 1 {
            noun.to_owned()
        } else {
            format!("{count} {noun}s")
        };
        for (at, direction) in [(first, before), (last + 1, after)] {
            let response = ui.add_enabled(
                at < limit && count <= limit.saturating_sub(at),
                egui::Button::new(format!("Insert {target} {direction}")),
            );
            if focus_first && response.enabled() {
                response.request_focus();
                focus_first = false;
            }
            if response.clicked() {
                let action = if rows {
                    SheetAction::InsertRows {
                        sheet: self.active,
                        at,
                        count,
                    }
                } else {
                    SheetAction::InsertColumns {
                        sheet: self.active,
                        at,
                        count,
                    }
                };
                self.queue(actions, action);
                self.grid_focus = true;
                ui.close_menu();
            }
        }
        ui.separator();
        let response = ui.button(format!("Delete {target}"));
        if focus_first {
            response.request_focus();
        }
        if response.clicked() {
            let action = if rows {
                SheetAction::DeleteRows {
                    sheet: self.active,
                    at: first,
                    count,
                }
            } else {
                SheetAction::DeleteColumns {
                    sheet: self.active,
                    at: first,
                    count,
                }
            };
            self.queue(actions, action);
            self.grid_focus = true;
            ui.close_menu();
        }
    }

    fn tabs(&mut self, ui: &mut Ui, book: &Workbook, actions: &mut Vec<SheetAction>) {
        egui::ScrollArea::horizontal()
            .id_salt("spreadsheet-tabs")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for sheet in &book.sheets {
                        let response = ui
                            .selectable_label(self.active == sheet.id, &sheet.name)
                            .on_hover_text(
                                "Select sheet · Double-click to rename · Alt+PageUp / Alt+PageDown",
                            );
                        if self.active == sheet.id && self.tab_reveal {
                            response.scroll_to_me(Some(egui::Align::Center));
                            self.tab_reveal = false;
                        }
                        if response.clicked() {
                            self.switch_sheet(sheet.id, actions);
                        }
                        if response.double_clicked() {
                            self.commit(actions);
                            self.active = sheet.id;
                            self.name = sheet.name.clone();
                            self.renaming = true;
                            self.rename_focus = true;
                            self.confirm_delete = false;
                        }
                    }
                    if ui
                        .add_enabled(book.sheets.len() < MAX_SHEETS, egui::Button::new("+"))
                        .on_hover_text("Add a sheet · Shift+F11")
                        .clicked()
                    {
                        self.new_sheet(book, actions);
                    }
                    let delete = ui.add_enabled(book.sheets.len() > 1, egui::Button::new("-"));
                    delete.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            book.sheets.len() > 1 && ui.is_enabled(),
                            "Delete active sheet",
                        )
                    });
                    if delete.on_hover_text("Delete active sheet").clicked() {
                        self.commit(actions);
                        self.confirm_delete = true;
                        self.delete_focus = true;
                        self.renaming = false;
                    }
                })
            });
        if self.renaming {
            ui.horizontal_wrapped(|ui| {
                let id = egui::Id::new("spreadsheet-sheet-name");
                if self.rename_focus {
                    ui.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Space);
                    });
                    ui.memory_mut(|memory| memory.request_focus(id));
                    self.rename_focus = false;
                    ui.ctx().request_repaint();
                }
                let response = ui.add(
                    TextEdit::singleline(&mut self.name)
                        .id(id)
                        .return_key(None)
                        .desired_width(180.0),
                );
                let enter = response.has_focus()
                    && ui.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                    });
                if ui.button("Save").clicked() || enter {
                    actions.push(SheetAction::RenameSheet {
                        sheet: self.active,
                        name: self.name.clone(),
                    });
                    self.tab_reveal = true;
                    self.renaming = false;
                    self.grid_focus = true;
                }
                if ui.button("Cancel").clicked()
                    || (response.has_focus() || response.lost_focus())
                        && ui.input_mut(|input| {
                            input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                        })
                {
                    self.renaming = false;
                    self.grid_focus = true;
                }
            });
        }
        if self.confirm_delete {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!(
                        "Delete {}?",
                        book.sheet(self.active)
                            .map_or("sheet", |sheet| sheet.name.as_str())
                    ))
                    .color(ERROR),
                );
                if self.delete_focus {
                    let id = ui.next_auto_id();
                    ui.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Space);
                    });
                    ui.memory_mut(|memory| memory.request_focus(id));
                    self.delete_focus = false;
                    ui.ctx().request_repaint();
                }
                if ui.button("Delete sheet").clicked() {
                    actions.push(SheetAction::RemoveSheet(self.active));
                    self.confirm_delete = false;
                    self.grid_focus = true;
                }
                if ui.button("Cancel").clicked() {
                    self.confirm_delete = false;
                    self.grid_focus = true;
                }
            });
        }
    }

    fn summary(&mut self, ui: &mut Ui, decimals: usize, actions: &mut Vec<SheetAction>) {
        ui.horizontal_wrapped(|ui| {
            let selection = self.selection();
            ui.label(
                RichText::new(format!(
                    "{} · {} cells",
                    range_label(selection),
                    selection.rows() * selection.cols()
                ))
                .color(MUTED)
                .size(11.0),
            );
            if let Some(values) = &self.values {
                let mut count = 0;
                let mut sum = 0.0;
                for row in selection.start.row..=selection.end.row {
                    for col in selection.start.col..=selection.end.col {
                        if let Some(CellValue::Number(value)) =
                            values.get(self.active, CellAddr::new(row, col))
                        {
                            count += 1;
                            sum += value;
                        }
                    }
                }
                if count > 0 && sum.is_finite() {
                    ui.label(
                        RichText::new(format!("Sum {sum:.decimals$}"))
                            .color(TEXT)
                            .size(11.0),
                    );
                }
            }
            ui.label(RichText::new("Decimals").color(MUTED).size(11.0));
            let mut number = decimals as u32;
            if ui
                .add(egui::DragValue::new(&mut number).range(0..=10))
                .changed()
            {
                actions.push(SheetAction::Decimals(number as usize));
            }
        });
    }

    fn copy(&mut self, ui: &Ui, book: &Workbook) {
        let selection = self.selection();
        let mut raw = Vec::new();
        let mut values = Vec::new();
        for row in selection.start.row..=selection.end.row {
            let mut raw_row = Vec::new();
            let mut value_row = Vec::new();
            for col in selection.start.col..=selection.end.col {
                let addr = CellAddr::new(row, col);
                raw_row.push(book.raw(self.active, addr).to_owned());
                value_row.push(
                    self.values
                        .as_ref()
                        .map(|values| values.value(self.active, addr).csv_value())
                        .unwrap_or_default(),
                );
            }
            raw.push(raw_row);
            values.push(value_row);
        }
        let text = spreadsheet::encode_delimited(&values, '\t');
        ui.ctx().copy_text(text.clone());
        self.copied = Some(CopiedCells {
            text,
            origin: selection.start,
            rows: raw,
        });
    }

    fn paste(&self, text: &str) -> Result<Vec<Vec<String>>, String> {
        self.pasted_rows(self.active, self.selection().start, text)
    }

    pub fn paste_at(
        &self,
        sheet: u64,
        origin: CellAddr,
        text: &str,
    ) -> Result<SheetAction, String> {
        Ok(SheetAction::SetBlock {
            sheet,
            origin,
            rows: self.pasted_rows(sheet, origin, text)?,
        })
    }

    fn pasted_rows(
        &self,
        _sheet: u64,
        origin: CellAddr,
        text: &str,
    ) -> Result<Vec<Vec<String>>, String> {
        if let Some(copied) = &self.copied {
            if copied.text == text {
                let dr = origin.row as i32 - copied.origin.row as i32;
                let dc = origin.col as i32 - copied.origin.col as i32;
                return Ok(copied
                    .rows
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|raw| sheet_formula::translate(raw, dr, dc))
                            .collect()
                    })
                    .collect());
            }
        }
        spreadsheet::parse_delimited(text, '\t')
    }
}

// Use the modifier captured with the key press. Shift may already be released
// by the time a browser frame processes its queued keyboard events.
fn consume_shift_key(ui: &Ui, key: egui::Key) -> Option<bool> {
    ui.input_mut(|input| {
        let shift = input.events.iter().find_map(|event| match event {
            egui::Event::Key {
                key: pressed,
                pressed: true,
                modifiers,
                ..
            } if *pressed == key && !modifiers.ctrl && !modifiers.alt && !modifiers.command => {
                Some(modifiers.shift)
            }
            _ => None,
        })?;
        input
            .consume_key(
                if shift {
                    egui::Modifiers::SHIFT
                } else {
                    egui::Modifiers::NONE
                },
                key,
            )
            .then_some(shift)
    })
}

fn sheet_icon_button(
    ui: &mut Ui,
    icon: SheetIcon,
    label: &str,
    selected: bool,
    size: f32,
) -> egui::Response {
    let response = ui.add(
        egui::Button::new("")
            .min_size(egui::vec2(size, size))
            .selected(selected),
    );
    sheet_icon_response(ui, response, icon, label, selected)
}

fn sheet_icon_menu(ui: &mut Ui, icon: SheetIcon, label: &str, add_contents: impl FnOnce(&mut Ui)) {
    // egui 0.31 returns menu.inner only when closing; the closure marks visibility.
    let mut open = false;
    let menu = egui::menu::menu_custom_button(
        ui,
        egui::Button::new("").min_size(egui::Vec2::splat(TOOL_ICON_SIZE)),
        |ui| {
            open = true;
            add_contents(ui);
        },
    );
    if open {
        ui.painter()
            .rect_filled(menu.response.rect.shrink(1.0), 2.0, SELECTED);
    }
    sheet_icon_response(ui, menu.response, icon, label, open);
}

fn sheet_icon_response(
    ui: &Ui,
    response: egui::Response,
    icon: SheetIcon,
    label: &str,
    selected: bool,
) -> egui::Response {
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, label)
    });
    let color = ui.style().interact(&response).fg_stroke.color;
    paint_sheet_icon(ui.painter(), response.rect, icon, color);
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.shrink(1.0),
            2.0,
            Stroke::new(1.0_f32, ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    response.on_hover_text(label)
}

fn paint_sheet_icon(painter: &egui::Painter, rect: egui::Rect, icon: SheetIcon, color: Color32) {
    let center = rect.center();
    let stroke = Stroke::new(1.4_f32, color);
    let line = |start: egui::Pos2, end: egui::Pos2, stroke: Stroke| {
        painter.line_segment(
            [
                start.round_to_pixel_center(painter.pixels_per_point()),
                end.round_to_pixel_center(painter.pixels_per_point()),
            ],
            stroke,
        );
    };
    match icon {
        SheetIcon::Bold
        | SheetIcon::Italic
        | SheetIcon::Underline
        | SheetIcon::Strike
        | SheetIcon::TextColor(_) => {
            let text = match icon {
                SheetIcon::Bold => "B",
                SheetIcon::Italic => "I",
                SheetIcon::Underline => "U",
                SheetIcon::Strike => "S",
                _ => "A",
            };
            let mut job = egui::text::LayoutJob::simple_singleline(
                text.into(),
                FontId::new(16.0, egui::FontFamily::Name("jig-sans".into())),
                color,
            );
            if let Some(section) = job.sections.first_mut() {
                section.format.italics = matches!(icon, SheetIcon::Italic);
            }
            let galley = painter.fonts(|fonts| fonts.layout_job(job));
            let offset = if matches!(icon, SheetIcon::TextColor(_)) {
                egui::vec2(0.0, -2.0)
            } else {
                egui::Vec2::ZERO
            };
            let position = center - galley.size() * 0.5 + offset;
            painter.galley(position, galley.clone(), color);
            if matches!(icon, SheetIcon::Bold) {
                painter.galley(position + egui::vec2(0.6, 0.0), galley, color);
            }
            if matches!(icon, SheetIcon::Underline | SheetIcon::Strike) {
                let y = center.y
                    + if matches!(icon, SheetIcon::Underline) {
                        7.0
                    } else {
                        0.0
                    };
                line(
                    egui::pos2(center.x - 5.5, y),
                    egui::pos2(center.x + 5.5, y),
                    Stroke::new(1.2_f32, color),
                );
            }
            if let SheetIcon::TextColor(swatch) = icon {
                line(
                    center + egui::vec2(-8.0, 8.0),
                    center + egui::vec2(8.0, 8.0),
                    Stroke::new(3.0_f32, rgb(swatch)),
                );
            }
        }
        SheetIcon::Alignment(alignment) => {
            for (row, width) in [16.0, 10.0, 16.0, 10.0].into_iter().enumerate() {
                let left = match alignment {
                    CellAlignment::Right => center.x + 8.0 - width,
                    CellAlignment::Center => center.x - width * 0.5,
                    CellAlignment::Auto | CellAlignment::Left => center.x - 8.0,
                };
                let y = center.y - 6.0 + row as f32 * 4.0;
                line(egui::pos2(left, y), egui::pos2(left + width, y), stroke);
            }
        }
        SheetIcon::FillColor(swatch) => {
            let bucket = [(-7.0, -1.0), (-1.0, -7.0), (5.0, -1.0), (-1.0, 5.0)]
                .map(|(x, y)| center + egui::vec2(x, y));
            painter.add(egui::Shape::closed_line(bucket.to_vec(), stroke));
            line(bucket[0], bucket[2], Stroke::new(1.0_f32, color));
            painter.add(egui::Shape::line(
                [(-4.0, -4.0), (-5.5, -6.0), (-4.0, -8.0), (-2.0, -8.0)]
                    .map(|(x, y)| center + egui::vec2(x, y))
                    .to_vec(),
                Stroke::new(1.1_f32, color),
            ));
            painter.add(egui::Shape::convex_polygon(
                [(7.5, 0.0), (5.5, 3.5), (7.5, 5.0), (9.5, 3.5)]
                    .map(|(x, y)| center + egui::vec2(x, y))
                    .to_vec(),
                color,
                Stroke::NONE,
            ));
            line(
                center + egui::vec2(-8.0, 8.0),
                center + egui::vec2(8.0, 8.0),
                Stroke::new(3.0_f32, rgb(swatch)),
            );
        }
        SheetIcon::Border(preset) => {
            let guide = Stroke::new(0.8_f32, MUTED.gamma_multiply(0.5));
            let start = center - egui::vec2(8.0, 8.0);
            for index in 0..3 {
                let offset = index as f32 * 8.0;
                for dash in 0..6 {
                    let along = dash as f32 * 3.0;
                    let end = (along + 1.4).min(16.0);
                    line(
                        start + egui::vec2(along, offset),
                        start + egui::vec2(end, offset),
                        guide,
                    );
                    line(
                        start + egui::vec2(offset, along),
                        start + egui::vec2(offset, end),
                        guide,
                    );
                }
            }
            for index in 0..3 {
                let offset = index as f32 * 8.0;
                let horizontal = match preset {
                    BorderPreset::All | BorderPreset::Horizontal => true,
                    BorderPreset::Outer => index != 1,
                    BorderPreset::Inner => index == 1,
                    BorderPreset::Top => index == 0,
                    BorderPreset::Bottom => index == 2,
                    _ => false,
                };
                let vertical = match preset {
                    BorderPreset::All | BorderPreset::Vertical => true,
                    BorderPreset::Outer => index != 1,
                    BorderPreset::Inner => index == 1,
                    BorderPreset::Left => index == 0,
                    BorderPreset::Right => index == 2,
                    _ => false,
                };
                if horizontal {
                    line(
                        start + egui::vec2(0.0, offset),
                        start + egui::vec2(16.0, offset),
                        stroke,
                    );
                }
                if vertical {
                    line(
                        start + egui::vec2(offset, 0.0),
                        start + egui::vec2(offset, 16.0),
                        stroke,
                    );
                }
            }
        }
    }
}

fn font_label(font: CellFont) -> &'static str {
    match font {
        CellFont::Mono => "Mono",
        CellFont::Sans => "Sans",
        CellFont::Serif => "Serif",
    }
}

fn format_label(format: &NumberFormat) -> String {
    match format {
        NumberFormat::General => "General".into(),
        NumberFormat::Number => "Number".into(),
        NumberFormat::Percent => "Percent".into(),
        NumberFormat::Currency(code) => format!("Currency · {code}"),
        NumberFormat::Date => "Date".into(),
    }
}

fn rgb(color: [u8; 3]) -> Color32 {
    Color32::from_rgb(color[0], color[1], color[2])
}

fn cell_font(style: &CellStyle) -> FontId {
    let family = match style.font_family {
        CellFont::Mono => "jig-mono",
        CellFont::Sans => "jig-sans",
        CellFont::Serif => "jig-serif",
    };
    FontId::new(
        f32::from(style.font_size),
        egui::FontFamily::Name(family.into()),
    )
}

fn ranges_intersect(a: CellRange, b: CellRange) -> bool {
    a.start.row <= b.end.row
        && a.end.row >= b.start.row
        && a.start.col <= b.end.col
        && a.end.col >= b.start.col
}

fn range_label(range: CellRange) -> String {
    if range.start == range.end {
        range.start.label()
    } else {
        format!("{}:{}", range.start.label(), range.end.label())
    }
}

fn parse_range(text: &str) -> Result<CellRange, String> {
    let mut parts = text.trim().split(':');
    let start = CellAddr::parse(parts.next().unwrap_or(""))?;
    let end = parts
        .next()
        .map(CellAddr::parse)
        .transpose()?
        .unwrap_or(start);
    if parts.next().is_some() {
        return Err("Use a cell or range such as A1:C4".into());
    }
    Ok(CellRange::new(start, end))
}

fn column_label(col: usize) -> String {
    let label = CellAddr::new(0, col).label();
    label.trim_end_matches('1').to_owned()
}
