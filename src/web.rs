//! egui adapter for the same local utility model used by the CLI and terminal.
use crate::web_theme;
use jig::app_settings::{Action as AppAction, AppSettings};

#[path = "web_info.rs"]
mod info;
#[path = "web_settings.rs"]
mod settings;
use crate::{
    browser_storage,
    chart_ui::{ChartAction, ChartPanel},
    notes_ui::{NoteAction, NoteDraftSnapshot, NoteDragPayload, NotesPanel},
    pdf_import_ui::{PdfImportAction, PdfImportPanel},
    spreadsheet_ui::{SheetAction, SheetPanel},
    stl_ui::{Snapshot as StlSnapshot, StlPanel},
    timeline_ui::{TimelineAction, TimelinePanel},
    weather_ui::WeatherPanel,
};
use chrono::{Local, NaiveDate, Utc};
use eframe::egui::emath::GuiRounding;
use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke, TextEdit, Ui};
use info::InfoPage;
use jig::{
    model::{State, TaskColor, MAX_TASK_LISTS},
    spreadsheet::{self, CellAddr, Workbook},
    timeline,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const UNDO_BYTES: usize = 64 * 1024 * 1024;
const TASK_SPLIT_WIDTH: f32 = 12.0;
const DRAFT_BACKUP_BYTES: usize = 128 * 1024;

#[derive(Serialize, Deserialize)]
struct WorkingDrafts {
    version: u8,
    kind: String,
    note: Option<NoteDraftSnapshot>,
    task: Option<TaskDraft>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    schedule: Option<ScheduleDraft>,
    selected_lists: Vec<u64>,
    active_list: u64,
}

#[derive(Serialize, Deserialize)]
struct TaskDraft {
    id: Option<u64>,
    text: String,
    details: String,
    list_id: Option<u64>,
    original: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ScheduleDraft {
    task_id: Option<u64>,
    legacy_id: Option<u64>,
    due: String,
    start: String,
    feature: bool,
    timeline: bool,
    initial_due: String,
    initial_start: String,
    initial_feature: bool,
    original: String,
}

#[derive(Clone, Copy)]
enum TaskEditorTarget {
    Task(u64),
    Deadline(u64),
}

#[derive(Clone, Copy)]
enum TaskDateField {
    Start,
    Due,
}

fn task_split_id() -> egui::Id {
    egui::Id::new("task-timeline-divider")
}

enum UndoEntry {
    Data { state: Box<State>, bytes: usize },
    Viewer(StlSnapshot),
}

enum PendingBackup {
    Write,
    Download,
}

fn same_schedule_source(original: &str, current: &str) -> bool {
    let schedule = |source: &str| {
        serde_json::from_str::<(Option<jig::model::Task>, Option<jig::model::Deadline>)>(source)
            .ok()
            .map(|(task, deadline)| {
                (
                    task.map(|task| (task.id, task.deadline, task.start)),
                    deadline.map(|deadline| (deadline.id, deadline.at)),
                )
            })
    };
    schedule(original).is_some_and(|original| schedule(current) == Some(original))
}

pub(crate) fn task_color(color: Option<TaskColor>, complete: bool) -> Color32 {
    let color = match color {
        Some(TaskColor::Rose) => Color32::from_rgb(234, 133, 173),
        Some(TaskColor::Amber) => Color32::from_rgb(218, 185, 126),
        Some(TaskColor::Sage) => Color32::from_rgb(148, 188, 160),
        Some(TaskColor::Blue) => Color32::from_rgb(109, 185, 238),
        Some(TaskColor::Lilac) => Color32::from_rgb(179, 158, 207),
        None => Color32::from_rgb(144, 161, 202),
    };
    if complete {
        color.gamma_multiply(0.7)
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

#[derive(Clone, Copy)]
enum DeferredView {
    Sheet,
    Charts(Option<(u64, spreadsheet::CellRange)>),
    Calculator,
    Settings,
    Info(InfoPage),
    Calendar,
    CalendarMonth(bool),
    Tasks,
    Notes,
}

struct ChecklistRow {
    task: Option<u64>,
    deadline: Option<u64>,
    title: String,
    complete: bool,
    color: Option<TaskColor>,
    position: Option<usize>,
    list_id: u64,
}

#[derive(Clone)]
struct TaskDragPayload {
    id: u64,
    order: Vec<(u64, u64)>,
    lists: Vec<u64>,
}

struct TaskDropRow {
    id: u64,
    list_id: u64,
    rect: egui::Rect,
}

pub struct BrowserApp {
    ctx: egui::Context,
    state: State,
    settings: AppSettings,
    settings_panel: crate::settings_ui::SettingsPanel,
    settings_open: bool,
    info_page: Option<InfoPage>,
    focus_info: bool,
    pending_settings: Option<AppSettings>,
    auto_backup_due: Option<f64>,
    initial_backup_pending: bool,
    pending_backup: Option<PendingBackup>,
    weather: WeatherPanel,
    calendar_panel: crate::calendar_ui::CalendarPanel,
    stl: StlPanel,
    undo_history: Vec<UndoEntry>,
    redo_history: Vec<UndoEntry>,
    undo_baseline: State,
    undo_baseline_json: Vec<u8>,
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
    task_details: String,
    task_edit: Option<u64>,
    task_edit_original: Option<String>,
    focus_task_input: bool,
    task_edit_list: Option<u64>,
    selected_task_lists: Vec<u64>,
    active_task_list: u64,
    task_lists_combine: bool,
    task_list_rename: Option<u64>,
    task_list_name: String,
    focus_task_list_name: bool,
    task_list_remove: Option<(u64, u64)>,
    task_list_tab_ids: Vec<(egui::Id, u64)>,
    last_task_list_click: Option<(u64, f64)>,
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
    task_date_original: Option<String>,
    task_date_initial: Option<(String, String, bool)>,
    calendar_task_date_target: Option<TaskDateField>,
    selected_date: NaiveDate,
    calendar_weather_date: Option<NaiveDate>,
    timeline_view: bool,
    task_split_draft: Option<f32>,
    timeline: TimelinePanel,
    sheet: SheetPanel,
    sheet_workspace: bool,
    pending_csv_target: Option<(u64, CellAddr)>,
    pending_paste_target: Option<(u64, CellAddr)>,
    tasks_workspace: bool,
    notes_workspace: bool,
    notes: NotesPanel,
    pending_note_input: bool,
    pending_chart_input: bool,
    ordered_input: crate::text_input::OrderedInput,
    pending_task_input: Option<egui::Id>,
    deferred_task_list: Option<(u64, bool)>,
    deferred_task_edit: Option<TaskEditorTarget>,
    deferred_view: Option<DeferredView>,
    draft_backup_payload: Option<String>,
    draft_backup_error: Option<String>,
    draft_backup_blocked: bool,
    charts_view: bool,
    charts: ChartPanel,
    pdf_target: Option<(u64, CellAddr)>,
    pdf_preview: Option<PdfImportPanel>,
    pdf_documents: Vec<jig::pdf_import::ExtractedPdf>,
    pdf_errors: Vec<String>,
    ocr_original: Option<jig::pdf_import::ExtractedPdf>,
    ocr_running: bool,
}

impl BrowserApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let loaded_settings = crate::settings_storage::load()
            .and_then(|text| text.map(|text| AppSettings::from_json(&text)).transpose());
        let settings_error = loaded_settings.as_ref().err().cloned();
        let settings = loaded_settings.ok().flatten().unwrap_or_default();
        web_theme::apply(&cc.egui_ctx, settings.theme);
        let settings_error = settings_error
            .or_else(|| crate::settings_storage::configure_shortcuts(&settings).err());
        let mut fonts = egui::FontDefinitions::default();
        for (name, bytes) in [
            ("jig-mono", jig::sheet_fonts::MONO),
            ("jig-sans", jig::sheet_fonts::SANS),
            ("jig-serif", jig::sheet_fonts::SERIF),
        ] {
            fonts
                .font_data
                .insert(name.into(), egui::FontData::from_static(bytes).into());
            fonts
                .families
                .insert(FontFamily::Name(name.into()), vec![name.into()]);
        }
        cc.egui_ctx.set_fonts(fonts);
        let loaded = browser_storage::load();
        let has_saved_personal = matches!(&loaded, Ok(Some(_)));
        let (mut state, status, storage_error) = match loaded {
            Ok(Some(state)) => (state, "Restored your browser-local data.".into(), false),
            Ok(None) => (
                State::default(),
                "Your tasks and notes save in this browser. Export JSON for a portable backup."
                    .into(),
                false,
            ),
            Err(error) => (
                State::default(),
                format!("Stored data could not be opened and has been preserved: {error}"),
                true,
            ),
        };
        let today = Local::now().date_naive();
        let weather = WeatherPanel::new(state.preferences.weather.clone());
        state.preferences.weather = weather.config().cloned();
        let undo_baseline = state.clone();
        let undo_baseline_json = serde_json::to_vec(&state).unwrap_or_default();
        let preferred = browser_storage::preferred_workspace();
        let workspace = if matches!(preferred.as_str(), "sheet" | "charts" | "notes") {
            preferred
        } else {
            "tasks".into()
        };
        browser_storage::set_workspace(&workspace);
        let initial_backup_pending = settings.auto_backup && has_saved_personal && !storage_error;
        let mut app = Self {
            ctx: cc.egui_ctx.clone(),
            state,
            settings,
            settings_panel: crate::settings_ui::SettingsPanel::new(),
            settings_open: false,
            info_page: None,
            focus_info: false,
            pending_settings: None,
            auto_backup_due: None,
            initial_backup_pending,
            pending_backup: None,
            weather,
            calendar_panel: crate::calendar_ui::CalendarPanel::new(today),
            stl: StlPanel::new(cc),
            undo_history: Vec::new(),
            redo_history: Vec::new(),
            undo_baseline,
            undo_baseline_json,
            dirty: false,
            storage_error,
            status,
            confirm: None,
            data_open: false,
            pending_import: None,
            focus: 1,
            expanded: false,
            focus_calculator: false,
            scroll_selected: false,
            calculation: String::new(),
            task_text: String::new(),
            task_details: String::new(),
            task_edit: None,
            task_edit_original: None,
            focus_task_input: false,
            task_edit_list: None,
            selected_task_lists: Vec::new(),
            active_task_list: 1,
            task_lists_combine: false,
            task_list_rename: None,
            task_list_name: String::new(),
            focus_task_list_name: false,
            task_list_remove: None,
            task_list_tab_ids: Vec::new(),
            last_task_list_click: None,
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
            task_date_original: None,
            task_date_initial: None,
            calendar_task_date_target: None,
            selected_date: today,
            calendar_weather_date: None,
            timeline_view: workspace == "tasks",
            task_split_draft: None,
            timeline: TimelinePanel::new(today),
            sheet: SheetPanel::new(),
            sheet_workspace: matches!(workspace.as_str(), "sheet" | "charts"),
            pending_csv_target: None,
            pending_paste_target: None,
            tasks_workspace: workspace == "tasks",
            notes_workspace: workspace == "notes",
            notes: NotesPanel::new(),
            pending_note_input: false,
            pending_chart_input: false,
            ordered_input: crate::text_input::OrderedInput::default(),
            pending_task_input: None,
            deferred_task_list: None,
            deferred_task_edit: None,
            deferred_view: None,
            draft_backup_payload: None,
            draft_backup_error: None,
            draft_backup_blocked: false,
            charts_view: workspace == "charts",
            charts: ChartPanel::new(),
            pdf_target: None,
            pdf_preview: None,
            pdf_documents: Vec::new(),
            pdf_errors: Vec::new(),
            ocr_original: None,
            ocr_running: false,
        };
        app.sanitize_task_lists();
        app.restore_working_drafts();
        if let Some(error) = settings_error {
            app.status.push_str(&format!(
                " Settings could not be loaded; defaults are in use: {error}"
            ));
        }
        app
    }

    fn has_task_draft(&self) -> bool {
        match self.task_edit {
            Some(id) => self
                .state
                .tasks
                .iter()
                .find(|task| task.id == id)
                .is_none_or(|task| {
                    self.task_text != task.text
                        || self.task_details != task.details.as_deref().unwrap_or_default()
                        || self.task_edit_list.is_some_and(|id| id != task.list_id)
                }),
            None => !self.task_text.is_empty() || !self.task_details.is_empty(),
        }
    }

    fn task_draft_snapshot(&self) -> Option<TaskDraft> {
        self.has_task_draft().then(|| TaskDraft {
            id: self.task_edit,
            text: self.task_text.clone(),
            details: self.task_details.clone(),
            list_id: self.task_edit_list.or(Some(self.active_task_list)),
            original: self.task_edit.and(self.task_edit_original.clone()),
        })
    }

    fn schedule_source(&self, task_id: Option<u64>, legacy_id: Option<u64>) -> Option<String> {
        let task = task_id.and_then(|id| self.state.tasks.iter().find(|task| task.id == id));
        let deadline = match legacy_id {
            Some(id) => self
                .state
                .deadlines
                .iter()
                .find(|deadline| deadline.id == id),
            None => task.and_then(|task| self.state.task_deadline(task.id)),
        };
        if task_id.is_some() && task.is_none() || legacy_id.is_some() && deadline.is_none() {
            return None;
        }
        (task.is_some() || deadline.is_some())
            .then(|| serde_json::to_string(&(task, deadline)).ok())
            .flatten()
    }

    fn capture_schedule_draft(&mut self) {
        self.task_date_original = self.schedule_source(self.task_date_edit, self.task_date_legacy);
        self.task_date_initial = Some((
            self.task_date.clone(),
            self.task_start.clone(),
            self.feature_after_date,
        ));
    }

    fn has_schedule_draft(&self) -> bool {
        (self.task_date_edit.is_some() || self.task_date_legacy.is_some())
            && self
                .task_date_initial
                .as_ref()
                .is_some_and(|(due, start, feature)| {
                    self.task_date != *due
                        || self.task_start != *start
                        || self.feature_after_date != *feature
                })
    }

    fn schedule_draft_snapshot(&self) -> Option<ScheduleDraft> {
        if !self.has_schedule_draft() {
            return None;
        }
        let (due, start, feature) = self.task_date_initial.as_ref()?;
        Some(ScheduleDraft {
            task_id: self.task_date_edit,
            legacy_id: self.task_date_legacy,
            due: self.task_date.clone(),
            start: self.task_start.clone(),
            feature: self.feature_after_date,
            timeline: self.task_date_timeline,
            initial_due: due.clone(),
            initial_start: start.clone(),
            initial_feature: *feature,
            original: self.task_date_original.clone()?,
        })
    }

    fn restore_working_drafts(&mut self) {
        let restored = (|| -> Result<(), String> {
            let Some(raw) = browser_storage::load_drafts()? else {
                return Ok(());
            };
            if raw.len() > DRAFT_BACKUP_BYTES {
                return Err("Draft backup exceeds 128 KiB".into());
            }
            let drafts: WorkingDrafts = serde_json::from_str(&raw)
                .map_err(|error| format!("Draft backup could not be decoded: {error}"))?;
            if drafts.version != 1 {
                return Err("Unsupported draft backup version".into());
            }
            if !matches!(drafts.kind.as_str(), "personal" | "demo") {
                return Err("Unknown draft backup kind".into());
            }
            if drafts.kind != "personal" {
                // Never restore an old temporary sample draft as personal work.
                return Ok(());
            }
            self.draft_backup_payload = Some(raw);
            self.selected_task_lists = drafts.selected_lists;
            self.selected_task_lists.sort_unstable();
            self.selected_task_lists.dedup();
            self.active_task_list = drafts.active_list;
            self.sanitize_task_lists();
            let mut recovered = false;
            if let Some(note) = drafts.note {
                recovered |= self.notes.restore_draft(note, &self.state)?;
            }
            if let Some(task) = drafts.task {
                let existing = task
                    .id
                    .and_then(|id| self.state.tasks.iter().find(|item| item.id == id));
                let matches = existing.is_some_and(|item| {
                    task.original.as_ref().is_some_and(|original| {
                        serde_json::to_string(item).is_ok_and(|current| current == *original)
                    })
                });
                self.task_text = task.text;
                self.task_details = task.details;
                self.task_edit_list = task
                    .list_id
                    .filter(|id| self.state.task_list(*id).is_some());
                if matches {
                    self.task_edit = task.id;
                    self.task_edit_original = task.original;
                } else {
                    recovered |= task.id.is_some();
                    self.task_edit = None;
                    self.task_edit_original = None;
                    self.active_task_list = self.task_edit_list.unwrap_or(self.active_task_list);
                    if !self.selected_task_lists.contains(&self.active_task_list) {
                        self.selected_task_lists.push(self.active_task_list);
                    }
                }
                self.focus_task_input = !self.notes_workspace;
            }
            if let Some(schedule) = drafts.schedule {
                if schedule.task_id.is_some() == schedule.legacy_id.is_some()
                    || self
                        .schedule_source(schedule.task_id, schedule.legacy_id)
                        .as_ref()
                        != Some(&schedule.original)
                {
                    return Err("An unfinished date edit belongs to a changed or missing item. Its backup has been preserved.".into());
                }
                self.task_date_edit = schedule.task_id;
                self.task_date_legacy = schedule.legacy_id;
                self.task_date = schedule.due;
                self.task_start = schedule.start;
                self.feature_after_date = schedule.feature;
                self.task_date_timeline = schedule.timeline;
                self.task_date_initial = Some((
                    schedule.initial_due,
                    schedule.initial_start,
                    schedule.initial_feature,
                ));
                self.task_date_original = Some(schedule.original);
                self.focus_task_date = true;
                if self.task_date_timeline {
                    self.timeline_view = true;
                }
                let list = schedule
                    .task_id
                    .and_then(|id| self.state.tasks.iter().find(|task| task.id == id))
                    .map_or(self.state.default_list_id(), |task| task.list_id);
                if !self.selected_task_lists.contains(&list) {
                    self.selected_task_lists.push(list);
                }
            }
            if !self.storage_error
                && (self.notes.has_unsaved_changes()
                    || self.has_task_draft()
                    || self.has_schedule_draft())
            {
                self.status = if recovered {
                    "Recovered unfinished edits as new items because the saved originals changed."
                        .into()
                } else {
                    "Restored your unfinished edits. Save them to keep them after closing this tab."
                        .into()
                };
            }
            Ok(())
        })();
        if let Err(error) = restored {
            self.draft_backup_error = Some(error);
            self.draft_backup_blocked = true;
        }
    }

    /// Working text is backed up per tab, without committing a partial record
    /// or adding an Undo entry for each keystroke.
    fn backup_working_drafts(&mut self) -> bool {
        let task = self.task_draft_snapshot();
        let schedule = self.schedule_draft_snapshot();
        let has_task_draft = task.is_some() || schedule.is_some();
        if self.draft_backup_blocked {
            return has_task_draft;
        }
        let note = self.notes.draft_snapshot();
        let payload = if note.is_none() && task.is_none() && schedule.is_none() {
            None
        } else {
            match serde_json::to_string(&WorkingDrafts {
                version: 1,
                kind: "personal".into(),
                note,
                task,
                schedule,
                selected_lists: self.selected_task_lists.clone(),
                active_list: self.active_task_list,
            }) {
                Ok(payload) => Some(payload),
                Err(error) => {
                    self.draft_backup_error = Some(error.to_string());
                    return has_task_draft;
                }
            }
        };
        if payload == self.draft_backup_payload {
            return has_task_draft;
        }
        let saved = match &payload {
            Some(text) if text.len() > DRAFT_BACKUP_BYTES => Err(
                "Draft backup exceeds 128 KiB. Save or copy unfinished edits before closing."
                    .into(),
            ),
            Some(text) => browser_storage::save_drafts(text),
            None => browser_storage::clear_drafts(),
        };
        // Remember failed attempts too: retry when text changes, rather than
        // repeatedly writing the same payload every paint or countdown tick.
        self.draft_backup_payload = payload;
        self.draft_backup_error = saved.err();
        has_task_draft
    }

    fn clear_working_drafts(&mut self) {
        self.draft_backup_payload = None;
        self.draft_backup_blocked = false;
        self.draft_backup_error = browser_storage::clear_drafts().err();
    }

    fn changed(&mut self) {
        self.sheet.invalidate();
        self.charts.invalidate();
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
        self.dirty = true;
        if self.storage_error {
            self.status = "Stored data could not be decoded and has been preserved. Export your current work, then explicitly import a valid file or confirm Start empty before saving.".into();
            return;
        }
        match browser_storage::save(&self.state) {
            Ok(()) => {
                self.dirty = false;
                self.storage_error = false;
                // Rebase unfinished dates after saved local metadata changes,
                // while retaining conflicts with changed dates or identities.
                if let (Some(original), Some(current)) = (
                    self.task_date_original.as_deref(),
                    self.schedule_source(self.task_date_edit, self.task_date_legacy),
                ) {
                    if same_schedule_source(original, &current) {
                        self.task_date_original = Some(current);
                    }
                }
                self.status = "Saved in this browser. Export JSON for a portable backup.".into();
                if self.settings.auto_backup && self.auto_backup_due.is_none() {
                    self.auto_backup_due = Some(self.ctx.input(|input| input.time) + 300.0);
                }
            }
            Err(error) => {
                self.status = format!(
                    "Not saved: {error}. Your changes remain in memory; retry or export JSON."
                )
            }
        }
    }

    fn push_undo(&mut self, entry: UndoEntry) {
        // Only a newly committed action abandons the redo branch.
        self.redo_history.clear();
        self.undo_history.push(entry);
        self.trim_history();
    }

    fn trim_history(&mut self) {
        while self.undo_history.len() + self.redo_history.len() > self.settings.undo_levels
            || self.history_bytes() > UNDO_BYTES
        {
            if self.undo_history.is_empty() && self.redo_history.is_empty() {
                break;
            }
            // The bottom of either stack is its furthest reachable state.
            // Retain the closest steps on both sides of the current state.
            if self.undo_history.len() >= self.redo_history.len() {
                self.undo_history.remove(0);
            } else {
                self.redo_history.remove(0);
            }
        }
    }

    fn history_bytes(&self) -> usize {
        let mut bytes = 0_usize;
        let mut meshes = std::collections::HashSet::new();
        if let Some((identity, size)) = self.stl.mesh_identity() {
            meshes.insert(identity);
            bytes = bytes.saturating_add(size);
        }
        for entry in self.undo_history.iter().chain(&self.redo_history) {
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

    fn reset_document_session(&mut self) {
        self.ctx.stop_dragging();
        self.settings_panel.cancel_capture();
        crate::settings_storage::set_capture(false);
        crate::settings_storage::reset_document();
        self.auto_backup_due = None;
        self.initial_backup_pending = false;
        self.pending_backup = None;
        self.clear_working_drafts();
        self.cancel_document_import();
        self.charts.reset();
        self.sheet.reset();
        self.notes.reset();
        self.deferred_view = None;
        self.pending_note_input = false;
        self.pending_chart_input = false;
        self.pending_task_input = None;
        self.deferred_task_edit = None;
        self.deferred_task_list = None;
        self.pending_csv_target = None;
        browser_storage::cancel_csv();
        self.timeline = TimelinePanel::new(Local::now().date_naive());
        self.calendar_weather_date = None;
        self.last_task_list_click = None;
        self.selected_task_lists.clear();
        self.sanitize_task_lists();
        self.undo_history.clear();
        self.redo_history.clear();
        self.stl.clear_undo();
        self.undo_baseline = self.state.clone();
        self.undo_baseline_json = serde_json::to_vec(&self.state).unwrap_or_default();
    }

    fn undo_last(&mut self, ctx: &egui::Context) {
        self.step_history(ctx, false);
    }

    fn redo_last(&mut self, ctx: &egui::Context) {
        self.step_history(ctx, true);
    }

    fn step_history(&mut self, ctx: &egui::Context, redo: bool) {
        ctx.stop_dragging();
        self.charts.cancel_gesture();
        self.cancel_task_note_drag(ctx);
        let action = if redo { "Redo" } else { "Undo" };
        if self.pending_chart_input || self.charts.has_draft() {
            self.status =
                format!("Save or cancel chart/interface edits in Charts before using {action}.");
            return;
        }
        if self.notes.has_draft() {
            self.status = format!("Save or cancel the note edit before using {action}.");
            return;
        }
        if self.pending_task_input.is_some() || self.has_task_draft() || self.has_schedule_draft() {
            self.status = format!("Save or clear task edits before using {action}.");
            return;
        }
        self.stl.finish_undo_gesture();
        self.collect_viewer_undo();
        let entry = if redo {
            self.redo_history.pop()
        } else {
            self.undo_history.pop()
        };
        let Some(entry) = entry else {
            return;
        };
        ctx.memory_mut(|memory| memory.close_popup());
        let reverse = match entry {
            UndoEntry::Data { state, .. } => {
                let previous = std::mem::replace(&mut self.state, *state);
                let reverse = UndoEntry::Data {
                    state: Box::new(previous),
                    bytes: self.undo_baseline_json.len(),
                };
                if self.weather.config() != self.state.preferences.weather.as_ref() {
                    self.weather
                        .replace_config(self.state.preferences.weather.clone());
                }
                self.reset_editors();
                // Keep the existing corrupt-file and stale-tab save guards.
                self.save_current();
                reverse
            }
            UndoEntry::Viewer(snapshot) => {
                let reverse = self.stl.snapshot();
                self.stl.restore(snapshot);
                UndoEntry::Viewer(reverse)
            }
        };
        // History navigation moves a snapshot; it is not a new edit.
        if redo {
            self.undo_history.push(reverse);
        } else {
            self.redo_history.push(reverse);
        }
        self.trim_history();
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
        self.ctx.stop_dragging();
        self.ctx.memory_mut(|memory| memory.close_popup());
        self.task_split_draft = None;
        self.timeline.cancel_gesture();
        self.sheet.restore_data();
        self.notes.reset();
        self.deferred_view = None;
        self.pending_note_input = false;
        self.pending_chart_input = false;
        self.pending_task_input = None;
        self.deferred_task_edit = None;
        self.deferred_task_list = None;
        self.charts.reset();
        self.task_edit = None;
        self.task_edit_original = None;
        self.focus_task_input = false;
        self.task_edit_list = None;
        self.task_list_rename = None;
        self.task_list_remove = None;
        self.last_task_list_click = None;
        self.focus_task_list_name = false;
        self.sanitize_task_lists();
        self.task_color_edit = None;
        self.task_text.clear();
        self.task_details.clear();
        self.confirm_clear_completed = false;
        self.task_date_edit = None;
        self.task_date.clear();
        self.task_start.clear();
        self.task_date_timeline = false;
        self.task_date_legacy = None;
        self.task_date_original = None;
        self.task_date_initial = None;
        self.calendar_task_date_target = None;
        self.task_date_error = None;
        self.feature_after_date = false;
        self.focus_task_date = false;
        self.calculation.clear();
    }

    fn apply_sheet_action(&mut self, action: SheetAction) -> bool {
        if matches!(
            action,
            SheetAction::SaveOds
                | SheetAction::ExportPdf(_)
                | SheetAction::OpenOds
                | SheetAction::ImportCsv(_)
                | SheetAction::ExportCsv(_)
        ) && !self.flush_notes_edit()
        {
            return false;
        }
        match &action {
            SheetAction::ImportCsv(id) => {
                self.cancel_document_import();
                self.pending_csv_target = Some((*id, self.sheet.selection().start));
                if let Err(error) = browser_storage::request_csv() {
                    self.pending_csv_target = None;
                    self.status = error;
                    return false;
                }
                return true;
            }
            SheetAction::ExportCsv(id) => {
                let empty = Workbook::default();
                let book = self.state.workbook.as_ref().unwrap_or(&empty);
                let result = book
                    .export_csv_at(*id, Local::now().date_naive())
                    .and_then(|csv| {
                        browser_storage::download_csv(&csv, &format!("jig-sheet-{id}.csv"))
                    });
                self.status = match result {
                    Ok(()) => {
                        "CSV values downloaded. Save ODS to retain formulas, formatting and Jig tools."
                            .into()
                    }
                    Err(error) => error,
                };
                return true;
            }
            SheetAction::OpenOds => {
                self.cancel_document_import();
                if let Err(error) = browser_storage::request_ods() {
                    self.status = error;
                    return false;
                }
                return true;
            }
            SheetAction::SaveOds => {
                let result = jig::documents::save_ods(&self.state).and_then(|bytes| {
                    browser_storage::download_binary(
                        &bytes,
                        "jig.ods",
                        "application/vnd.oasis.opendocument.spreadsheet",
                    )
                });
                self.status = match result {
                    Ok(()) => {
                        "ODS document download prepared with your workbook, tasks and Jig data."
                            .into()
                    }
                    Err(error) => error,
                };
                return true;
            }
            SheetAction::ExportPdf(id) => {
                let selected = self.sheet.selection();
                let area = (selected.rows() > 1 || selected.cols() > 1).then_some(selected);
                let result = jig::documents::export_pdf(&self.state, *id, area).and_then(|bytes| {
                    browser_storage::download_binary(&bytes, "jig-sheet.pdf", "application/pdf")
                });
                self.status = match result {
                    Ok(()) => "PDF download prepared from the sheet and its charts.".into(),
                    Err(error) => error,
                };
                return true;
            }
            SheetAction::ImportPdf(id) => {
                self.cancel_document_import();
                let mut origin = self.sheet.selection().start;
                if origin == CellAddr::new(0, 0) {
                    if let Some(range) = self
                        .state
                        .workbook
                        .as_ref()
                        .and_then(|book| book.used_range(*id))
                    {
                        if range.end.row + 1 < spreadsheet::MAX_ROWS {
                            origin.row = range.end.row + 1;
                        } else {
                            self.status = "No rows remain below this sheet. Choose a blank sheet for the document import.".into();
                            return false;
                        }
                    }
                }
                self.pdf_target = Some((*id, origin));
                self.charts_view = false;
                browser_storage::set_workspace("sheet");
                if let Err(error) = browser_storage::request_pdf() {
                    self.pdf_target = None;
                    self.status = error;
                    return false;
                }
                return true;
            }
            SheetAction::ImportJson => {
                self.cancel_document_import();
                if let Err(error) = browser_storage::request_import() {
                    self.status = error;
                    return false;
                }
                return true;
            }
            SheetAction::ExportJson => {
                if let Err(error) = browser_storage::download(&self.state) {
                    self.status = error;
                    return false;
                }
                self.status = "JSON backup download prepared.".into();
                return true;
            }
            SheetAction::PasteClipboard { sheet, origin } => {
                self.pending_paste_target = Some((*sheet, *origin));
                if let Err(error) = browser_storage::request_clipboard() {
                    self.status = error;
                    return false;
                }
                return true;
            }
            SheetAction::ShowCharts => {
                return self.show_charts(&self.ctx.clone(), None);
            }
            SheetAction::SendToCharts { sheet, range } => {
                return self.show_charts(&self.ctx.clone(), Some((*sheet, *range)));
            }
            SheetAction::Decimals(decimals) => {
                if *decimals <= 10 && self.state.preferences.decimals != *decimals {
                    self.state.preferences.decimals = *decimals;
                    self.changed();
                }
                return true;
            }
            _ => {}
        }
        let mut book = self.state.workbook.clone().unwrap_or_default();
        let result = match &action {
            SheetAction::SetCell { sheet, addr, raw } => book.set_cell(*sheet, *addr, raw),
            SheetAction::SetBlock {
                sheet,
                origin,
                rows,
            } => book.set_block(*sheet, *origin, rows),
            SheetAction::Clear { sheet, range } => book.clear(*sheet, *range),
            SheetAction::AddSheet(name) => book.add_sheet(name).map(|_| true),
            SheetAction::RenameSheet { sheet, name } => book.rename_sheet(*sheet, name),
            SheetAction::RemoveSheet(sheet) => book.remove_sheet(*sheet),
            SheetAction::PatchStyle {
                sheet,
                range,
                patch,
            } => book.patch_style(*sheet, *range, patch.clone()),
            SheetAction::SetBorders {
                sheet,
                range,
                preset,
            } => book.set_borders(*sheet, *range, *preset),
            SheetAction::InsertRows { sheet, at, count } => book.insert_rows(*sheet, *at, *count),
            SheetAction::DeleteRows { sheet, at, count } => book.delete_rows(*sheet, *at, *count),
            SheetAction::InsertColumns { sheet, at, count } => {
                book.insert_columns(*sheet, *at, *count)
            }
            SheetAction::DeleteColumns { sheet, at, count } => {
                book.delete_columns(*sheet, *at, *count)
            }
            SheetAction::Merge { sheet, range } => book.merge(*sheet, *range),
            SheetAction::Unmerge { sheet, range } => book.unmerge(*sheet, *range),
            SheetAction::AddChart {
                kind,
                source,
                title,
            } => book.add_chart(*kind, *source, title).map(|_| true),
            _ => unreachable!("Non-data sheet actions were handled above"),
        };
        match result {
            Ok(changed) => {
                if changed {
                    self.state.workbook = Some(book);
                    self.changed();
                    if matches!(action, SheetAction::AddChart { .. }) {
                        self.show_charts(&self.ctx.clone(), None);
                    }
                }
                true
            }
            Err(error) => {
                self.status = error.clone();
                self.sheet.reject_action(&action, error);
                false
            }
        }
    }

    fn flush_sheet_edit(&mut self) -> bool {
        match self.sheet.commit_pending() {
            Some(action) => self.apply_sheet_action(action),
            None => true,
        }
    }

    fn apply_note_action(&mut self, action: NoteAction) -> bool {
        let result = match &action {
            NoteAction::Add { text } => self.state.add_note(text).map(|_| ()),
            NoteAction::Edit { id, text } => self.state.edit_note(*id, text),
            NoteAction::Remove(id) => self.state.remove_note(*id),
            NoteAction::Pin { id, pinned } => self.state.set_note_pinned(*id, *pinned),
            NoteAction::Move { id, target, after } => {
                self.state.move_note_relative(*id, *target, *after)
            }
            NoteAction::ToTask { id, list_id } => {
                self.state.note_to_task(*id, *list_id).map(|_| ())
            }
        };
        match result {
            Ok(()) => {
                self.changed();
                true
            }
            Err(error) => {
                self.status = error.clone();
                self.notes.reject_action(&action, error);
                false
            }
        }
    }

    fn flush_notes_edit(&mut self) -> bool {
        if self.pending_note_input {
            self.status = "Save the note edit before continuing.".into();
            return false;
        }
        if let Some(action) = self.notes.finish_edit() {
            return self.apply_note_action(action);
        }
        if self.notes.has_draft() {
            self.status = "Finish or cancel the note edit before continuing.".into();
            return false;
        }
        true
    }

    fn notes_board(&mut self, ui: &mut Ui) {
        let actions = self.notes.ui(ui, &self.state, self.active_task_list);
        self.pending_note_input = false;
        for action in actions {
            if !self.apply_note_action(action) {
                break;
            }
            ui.ctx().request_repaint();
        }
    }

    fn show_notes(&mut self, ctx: &egui::Context) -> bool {
        if self.has_pending_editor_input() {
            self.deferred_view = Some(DeferredView::Notes);
            return false;
        }
        if !self.flush_sheet_edit() || !self.flush_notes_edit() {
            return false;
        }
        self.charts.cancel_gesture();
        self.cancel_task_note_drag(ctx);
        self.calendar_task_date_target = None;
        ctx.memory_mut(|memory| memory.close_popup());
        self.sheet_workspace = false;
        self.tasks_workspace = false;
        self.close_settings();
        self.info_page = None;
        self.notes_workspace = true;
        self.task_split_draft = None;
        self.timeline.cancel_gesture();
        self.expanded = false;
        self.focus_calculator = false;
        self.focus_task_date = false;
        browser_storage::set_workspace("notes");
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        ctx.request_repaint();
        true
    }

    fn has_pending_editor_input(&self) -> bool {
        self.pending_note_input || self.pending_task_input.is_some() || self.pending_chart_input
    }

    fn set_workspace(&mut self, sheet: bool, ctx: &egui::Context) -> bool {
        if !sheet {
            return self.show_tasks(ctx);
        }
        self.show_sheet_workspace(ctx, false)
    }

    fn show_charts(
        &mut self,
        ctx: &egui::Context,
        source: Option<(u64, spreadsheet::CellRange)>,
    ) -> bool {
        if self.has_pending_editor_input() {
            self.deferred_view = Some(DeferredView::Charts(source));
            return false;
        }
        if !self.show_sheet_workspace(ctx, true) {
            return false;
        }
        self.charts
            .set_selection_context(self.sheet.active_sheet(), self.sheet.selection());
        if let Some((sheet, range)) = source {
            self.charts.prefill_from_selection(sheet, range);
        }
        true
    }

    fn show_sheet_workspace(&mut self, ctx: &egui::Context, charts: bool) -> bool {
        if self.has_pending_editor_input() {
            self.deferred_view = Some(if charts {
                DeferredView::Charts(None)
            } else {
                DeferredView::Sheet
            });
            return false;
        }
        if !self.flush_sheet_edit() || !self.flush_notes_edit() {
            return false;
        }
        self.charts.cancel_gesture();
        self.cancel_task_note_drag(ctx);
        self.calendar_task_date_target = None;
        ctx.memory_mut(|memory| memory.close_popup());
        self.close_settings();
        self.info_page = None;
        self.sheet_workspace = true;
        self.charts_view = charts;
        self.tasks_workspace = false;
        self.notes_workspace = false;
        self.task_split_draft = None;
        self.timeline.cancel_gesture();
        browser_storage::set_workspace(if charts { "charts" } else { "sheet" });
        self.focus_calculator = false;
        self.focus_task_date = false;
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        if !charts {
            self.sheet.request_focus();
        }
        ctx.request_repaint();
        true
    }

    fn spreadsheet(&mut self, ui: &mut Ui) {
        if self.document_import_panel(ui) {
            return;
        }
        if self.charts_view {
            ui.horizontal(|ui| {
                if ui.button("Save .ods").clicked() {
                    self.apply_sheet_action(SheetAction::SaveOds);
                }
                if ui
                    .button("Export sheet PDF")
                    .on_hover_text("Export the active sheet and its linked charts")
                    .clicked()
                {
                    self.apply_sheet_action(SheetAction::ExportPdf(self.sheet.active_sheet()));
                }
            });
            for action in self.charts.ui(ui, &self.state) {
                self.apply_chart_action(action);
            }
            self.pending_chart_input = false;
            return;
        }
        let empty = Workbook::default();
        let book = self.state.workbook.as_ref().unwrap_or(&empty);
        let actions = self.sheet.ui(ui, book, self.state.preferences.decimals);
        for action in actions {
            let applied = match action {
                SheetAction::ShowTasks => self.show_tasks(ui.ctx()),
                SheetAction::ShowCalculator => {
                    self.open_calculator(ui.ctx());
                    true
                }
                SheetAction::Undo => {
                    self.undo_last(ui.ctx());
                    true
                }
                action => self.apply_sheet_action(action),
            };
            ui.ctx().request_repaint();
            if !applied {
                break;
            }
        }
    }

    fn show_tasks(&mut self, ctx: &egui::Context) -> bool {
        if self.has_pending_editor_input() {
            self.deferred_view = Some(DeferredView::Tasks);
            return false;
        }
        if !self.flush_sheet_edit() || !self.flush_notes_edit() {
            return false;
        }
        self.charts.cancel_gesture();
        self.cancel_task_note_drag(ctx);
        self.calendar_task_date_target = None;
        ctx.memory_mut(|memory| memory.close_popup());
        self.timeline.cancel_gesture();
        self.sheet_workspace = false;
        self.close_settings();
        self.info_page = None;
        self.tasks_workspace = true;
        self.notes_workspace = false;
        self.focus = 1;
        self.expanded = false;
        self.timeline_view = true;
        self.focus_calculator = false;
        self.focus_task_date = false;
        browser_storage::set_workspace("tasks");
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        ctx.request_repaint();
        true
    }

    fn workspace_buttons(&mut self, ui: &mut Ui) {
        let sense = egui::Sense::click().difference(egui::Sense::focusable_noninteractive());
        if ui
            .add(
                egui::Button::new("Tasks")
                    .selected(
                        self.tasks_workspace && !self.settings_open && self.info_page.is_none(),
                    )
                    .sense(sense),
            )
            .on_hover_text(format!(
                "Your existing tasks and Gantt timeline · {}",
                self.shortcut_label(AppAction::Tasks)
            ))
            .clicked()
        {
            self.show_tasks(ui.ctx());
        }
        if ui
            .add(
                egui::Button::new("Notes")
                    .selected(
                        self.notes_workspace && !self.settings_open && self.info_page.is_none(),
                    )
                    .sense(sense),
            )
            .on_hover_text(format!(
                "Plain text notes · {}",
                self.shortcut_label(AppAction::Notes)
            ))
            .clicked()
        {
            self.show_notes(ui.ctx());
        }
        if ui
            .add(
                egui::Button::new("Sheets")
                    .selected(
                        self.sheet_workspace
                            && !self.charts_view
                            && !self.settings_open
                            && self.info_page.is_none(),
                    )
                    .sense(sense),
            )
            .on_hover_text(format!(
                "Spreadsheet workspace · {} switches Sheets/Tasks",
                self.shortcut_label(AppAction::ToggleWorkspace)
            ))
            .clicked()
        {
            self.set_workspace(true, ui.ctx());
        }
        if ui
            .add(
                egui::Button::new("Charts")
                    .selected(
                        self.sheet_workspace
                            && self.charts_view
                            && !self.settings_open
                            && self.info_page.is_none(),
                    )
                    .sense(sense),
            )
            .on_hover_text("Charts and cell-linked interfaces")
            .clicked()
        {
            self.show_charts(ui.ctx(), None);
        }
    }

    fn sanitize_task_lists(&mut self) {
        self.selected_task_lists
            .retain(|id| self.state.task_list(*id).is_some());
        if self.selected_task_lists.is_empty() {
            self.selected_task_lists.push(self.state.default_list_id());
        }
        if !self.selected_task_lists.contains(&self.active_task_list) {
            self.active_task_list = self.selected_task_lists[0];
        }
        if self
            .task_edit_list
            .is_some_and(|id| self.state.task_list(id).is_none())
        {
            self.task_edit_list = self.task_edit.and_then(|id| {
                self.state
                    .tasks
                    .iter()
                    .find(|task| task.id == id)
                    .map(|task| task.list_id)
            });
        }
        if self
            .task_list_rename
            .is_some_and(|id| self.state.task_list(id).is_none())
        {
            self.task_list_rename = None;
        }
        if self.task_list_remove.is_some_and(|(id, target)| {
            self.state.task_list(id).is_none() || self.state.task_list(target).is_none()
        }) {
            self.task_list_remove = None;
        }
    }

    fn select_task_list(&mut self, id: u64, combine: bool, ctx: &egui::Context) {
        if self.pending_task_input.is_some() {
            self.deferred_task_list = Some((id, combine));
            ctx.request_repaint();
            return;
        }
        let previous = (self.selected_task_lists.clone(), self.active_task_list);
        if combine {
            if self.selected_task_lists.contains(&id) {
                if self.selected_task_lists.len() > 1 {
                    self.selected_task_lists.retain(|selected| *selected != id);
                }
            } else {
                self.selected_task_lists.push(id);
                self.active_task_list = id;
            }
        } else {
            self.selected_task_lists = vec![id];
            self.active_task_list = id;
        }
        self.sanitize_task_lists();
        let schedule_list = self
            .task_date_edit
            .and_then(|id| self.state.tasks.iter().find(|task| task.id == id))
            .map_or(self.state.default_list_id(), |task| task.list_id);
        if self.has_schedule_draft() && !self.selected_task_lists.contains(&schedule_list) {
            self.selected_task_lists = previous.0;
            self.active_task_list = previous.1;
            self.status = "Save or cancel the date edit before hiding its list.".into();
            ctx.request_repaint();
            return;
        }
        if self.has_task_draft()
            && self.task_edit.is_some_and(|id| {
                self.state
                    .tasks
                    .iter()
                    .find(|task| task.id == id)
                    .is_some_and(|task| !self.selected_task_lists.contains(&task.list_id))
            })
        {
            self.selected_task_lists = previous.0;
            self.active_task_list = previous.1;
            self.status = "Save or cancel the task edit before hiding its list.".into();
            ctx.request_repaint();
            return;
        }
        self.timeline.cancel_gesture();
        self.confirm_clear_completed = false;
        self.cancel_task_note_drag(ctx);
        self.task_list_rename = None;
        self.focus_task_list_name = false;
        self.task_list_remove = None;
        if self.task_edit.is_some_and(|id| {
            self.state
                .tasks
                .iter()
                .find(|task| task.id == id)
                .is_some_and(|task| !self.selected_task_lists.contains(&task.list_id))
        }) {
            self.task_edit = None;
            self.task_edit_original = None;
            self.task_edit_list = None;
            self.task_text.clear();
            self.task_details.clear();
        }
        if self.task_date_edit.is_some_and(|id| {
            self.state
                .tasks
                .iter()
                .find(|task| task.id == id)
                .is_some_and(|task| !self.selected_task_lists.contains(&task.list_id))
        }) || (self.task_date_legacy.is_some()
            && !self
                .selected_task_lists
                .contains(&self.state.default_list_id()))
        {
            self.close_task_dates();
        }
        ctx.request_repaint();
    }

    fn begin_task_list_rename(&mut self, id: u64, ctx: &egui::Context) {
        if let Some(list) = self.state.task_list(id) {
            self.task_list_rename = Some(id);
            self.task_list_name = list.name.clone();
            self.focus_task_list_name = true;
            self.task_list_remove = None;
            ctx.request_repaint();
        }
    }

    fn task_list_tabs(&mut self, ui: &mut Ui) {
        let palette = web_theme::colors(ui.ctx());
        self.sanitize_task_lists();
        self.task_list_tab_ids.clear();
        let (pointer_clicked, click_time) = ui.input(|input| {
            (
                input.pointer.button_clicked(egui::PointerButton::Primary),
                input.time,
            )
        });
        // egui counts nearby clicks globally. A rename needs consecutive
        // pointer clicks on this same tab, never quick choices across tabs.
        let previous_click = if pointer_clicked {
            self.last_task_list_click.take()
        } else {
            None
        };
        let lists = self.state.task_lists.clone();
        let mut reveal_tabs = false;
        let mut confirm_remove = false;
        let strip = ui.horizontal(|ui| {
            let controls = if lists.len() > 1 { 148.0 } else { 76.0 };
            let natural_width = lists.iter().map(|list| {
                (list.name.chars().count() as f32 * 9.0 + 18.0).clamp(64.0, 160.0)
            }).sum::<f32>() + ui.spacing().item_spacing.x * lists.len().saturating_sub(1) as f32;
            let tabs_width = natural_width.min((ui.available_width() - controls).max(24.0));
            ui.allocate_ui_with_layout(
                egui::vec2(tabs_width, 32.0),
                egui::Layout::left_to_right(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt("task-list-tabs")
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (index, list) in lists.iter().enumerate() {
                                    let selected = self.selected_task_lists.contains(&list.id);
                                    let active = selected && self.active_task_list == list.id;
                                    let text = RichText::new(&list.name);
                                    let mut button = egui::Button::new(if active {
                                        text.strong()
                                    } else {
                                        text
                                    })
                                    .truncate()
                                    .selected(selected);
                                    if selected && self.selected_task_lists.len() > 1 {
                                        button = button.fill(web_theme::task_list_fill_for(ui.ctx(), index));
                                    }
                                    if active {
                                        button = button.stroke(Stroke::new(1.0_f32, palette.accent));
                                    }
                                    let width = (list.name.chars().count() as f32 * 9.0 + 18.0)
                                        .clamp(64.0, 160.0);
                                    let response = ui.push_id(("task-list-tab", list.id), |ui| {
                                        ui.add_sized([width, 30.0], button)
                                    }).inner;
                                    self.task_list_tab_ids.push((response.id, list.id));
                                    if response.gained_focus() {
                                        response.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
                                        reveal_tabs = true;
                                    }
                                    let pointer_tab_click = pointer_clicked
                                        && response.clicked_by(egui::PointerButton::Primary)
                                        && response.interact_pointer_pos().is_some_and(|position| response.rect.contains(position));
                                    if pointer_tab_click {
                                        self.last_task_list_click = Some((list.id, click_time));
                                    }
                                    let shift_key = response.has_focus()
                                        && ui.input_mut(|input| {
                                            input.consume_key(egui::Modifiers::SHIFT, egui::Key::Space)
                                                || input.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter)
                                        });
                                    if response.clicked() || shift_key {
                                        response.request_focus();
                                        let combine = self.task_lists_combine
                                            || shift_key
                                            || ui.input(|input| input.modifiers.shift);
                                        self.select_task_list(list.id, combine, ui.ctx());
                                    }
                                    if response.double_clicked()
                                        && pointer_tab_click
                                        && previous_click.is_some_and(|(id, time)| id == list.id && time != click_time)
                                        && !self.task_lists_combine
                                        && !ui.input(|input| input.modifiers.shift)
                                    {
                                        self.begin_task_list_rename(list.id, ui.ctx());
                                    }
                                    response.on_hover_text(format!(
                                        "{} · Click to show · Shift-click to combine · Double-click or {} to rename{}",
                                        list.name,
                                        self.shortcut_label(AppAction::RenameTaskList),
                                        if active { " · New tasks go here" } else { "" }
                                    ));
                                }
                            });
                        });
                },
            );
            if ui
                .add_enabled(lists.len() < MAX_TASK_LISTS, egui::Button::new("+").min_size(egui::vec2(26.0, 30.0)))
                .on_hover_text("Add a task list · Maximum five")
                .clicked()
            {
                let name = (1..)
                    .map(|number| format!("List {number}"))
                    .find(|name| !lists.iter().any(|list| list.name.eq_ignore_ascii_case(name)))
                    .unwrap_or_else(|| "New list".into());
                match self.state.add_task_list(&name) {
                    Ok(id) => {
                        self.changed();
                        self.select_task_list(id, false, ui.ctx());
                        self.begin_task_list_rename(id, ui.ctx());
                    }
                    Err(error) => self.status = error,
                }
            }
            if ui
                .add_enabled(lists.len() > 1, egui::Button::new("−").min_size(egui::vec2(26.0, 30.0)))
                .on_hover_text("Remove the active list · Click again to confirm · Its tasks move to a remaining list")
                .clicked()
            {
                if self.task_list_remove.is_some_and(|(id, _)| id == self.active_task_list) {
                    confirm_remove = true;
                } else if let Some(target) = lists.iter().find(|list| list.id != self.active_task_list) {
                    self.task_list_remove = Some((self.active_task_list, target.id));
                    self.task_list_rename = None;
                }
            }
            if lists.len() > 1 {
                ui.toggle_value(&mut self.task_lists_combine, "Multi")
                    .on_hover_text("Tap tabs to combine lists · Shift-click also works");
            }
        });
        if reveal_tabs {
            // The inner horizontal area consumes both scroll targets. Reveal
            // the strip vertically only after that area has finished.
            strip
                .response
                .scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
        }
        if let Some(id) = self.task_list_rename {
            ui.horizontal_wrapped(|ui| {
                ui.label("List name");
                let response = ui.add(
                    TextEdit::singleline(&mut self.task_list_name)
                        .id(egui::Id::new("task-list-name"))
                        .return_key(None)
                        .char_limit(64)
                        .desired_width((ui.available_width() - 125.0).clamp(100.0, 260.0)),
                );
                if self.focus_task_list_name {
                    response.request_focus();
                    self.focus_task_list_name = false;
                    if let Some(mut state) = TextEdit::load_state(ui.ctx(), response.id) {
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(self.task_list_name.chars().count()),
                            )));
                        TextEdit::store_state(ui.ctx(), response.id, state);
                    }
                    ui.ctx().request_repaint();
                }
                let enter = response.has_focus()
                    && ui.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                    });
                if ui.button("Save").clicked() || enter {
                    let result = self.state.rename_task_list(id, &self.task_list_name);
                    if result.is_ok() {
                        self.task_list_rename = None;
                    }
                    self.apply(result);
                }
                if ui.button("Cancel").clicked() {
                    self.task_list_rename = None;
                }
            });
        }
        if let Some((id, target)) = self.task_list_remove {
            let name = self
                .state
                .task_list(id)
                .map(|list| list.name.clone())
                .unwrap_or_default();
            let target_name = self
                .state
                .task_list(target)
                .map(|list| list.name.clone())
                .unwrap_or_default();
            let count = self
                .state
                .tasks
                .iter()
                .filter(|task| task.list_id == id)
                .count();
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "Remove {name}? Its {count} tasks move to {target_name}."
                ));
                if ui.button("Remove").clicked() || confirm_remove {
                    let result = self.state.remove_task_list(id, target);
                    if result.is_ok() {
                        self.task_list_remove = None;
                        self.select_task_list(target, false, ui.ctx());
                    }
                    self.apply(result);
                }
                if ui.button("Cancel").clicked() {
                    self.task_list_remove = None;
                }
            });
        }
    }

    fn tasks_main(&mut self, ui: &mut Ui, height: f32) {
        let palette = web_theme::colors(ui.ctx());
        // Apply queued note text before allowing an upper control to move focus.
        ui.scope(|ui| {
            if self.pending_note_input {
                ui.disable();
            }
            let top = ui.cursor().top();
            self.task_list_tabs(ui);
            ui.add_space(3.0);
            if let Some(deadline) = self.state.deadlines.iter().find(|deadline| {
                Some(deadline.id) == self.state.featured
                    && self.state.is_deadline_active(deadline.id)
            }) {
                let seconds = (deadline.at - Utc::now()).num_seconds();
                let n = seconds.unsigned_abs();
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&deadline.title).strong().color(palette.text));
                    ui.label(
                        RichText::new(format!(
                            "{}{:02}d {:02}h {:02}m {:02}s",
                            if seconds < 0 { "OVERDUE " } else { "" },
                            n / 86400,
                            n / 3600 % 24,
                            n / 60 % 60,
                            n % 60
                        ))
                        .monospace()
                        .size(18.0)
                        .color(if seconds < 0 {
                            palette.error
                        } else {
                            palette.accent
                        }),
                    );
                });
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button("Gantt chart").clicked() {
                    let action = SheetAction::AddChart {
                        kind: jig::charts::ChartKind::Gantt,
                        source: jig::charts::ChartSource::Tasks,
                        title: "Task plan".into(),
                    };
                    if self.apply_sheet_action(action) {
                        self.show_charts(ui.ctx(), None);
                    }
                }
                if ui.button("Save .ods").clicked() {
                    self.apply_sheet_action(SheetAction::SaveOds);
                }
            });
            let height = (height - (ui.cursor().top() - top) - 8.0).max(230.0);
            let panel_height = if self.expanded {
                height
            } else if !self.state.notes.is_empty() || self.notes.has_draft() {
                (height * 0.58).clamp(350.0, 560.0).min(height)
            } else {
                (height - 52.0).max(230.0)
            };
            if self.expanded {
                self.cancel_task_split(ui.ctx());
                self.tool(ui, if self.focus == 2 { 2 } else { 1 }, panel_height);
            } else if ui.available_width() >= 800.0 {
                self.task_timeline_panels(ui, panel_height, true);
            } else {
                self.cancel_task_split(ui.ctx());
                self.tool(ui, 1, 300.0);
                ui.add_space(8.0);
                self.tool(ui, 2, 350.0);
            }
        });
        if !self.expanded {
            ui.add_space(10.0);
            self.notes_board(ui);
        }
    }

    fn cancel_task_split(&mut self, ctx: &egui::Context) {
        self.task_split_draft = None;
        ctx.memory_mut(|memory| memory.surrender_focus(task_split_id()));
    }

    fn commit_task_split(&mut self, percent: Option<u8>) {
        self.task_split_draft = None;
        if self.state.preferences.task_panel_percent != percent {
            self.state.preferences.task_panel_percent = percent;
            self.changed();
        }
    }

    fn task_timeline_panels(&mut self, ui: &mut Ui, height: f32, tasks_left: bool) {
        let width = ui.available_width();
        let available = width - TASK_SPLIT_WIDTH;
        let default = if tasks_left {
            (width * 0.43).clamp(330.0, 600.0)
        } else {
            available - 330.0
        };
        let percent = self
            .task_split_draft
            .or_else(|| self.state.preferences.task_panel_percent.map(f32::from));
        let tasks = percent
            .map_or(default, |percent| available * percent / 100.0)
            .clamp(300.0, available - 330.0);
        let (left, right) = if tasks_left {
            (tasks, available - tasks)
        } else {
            (available - tasks, tasks)
        };
        let item_spacing = ui.spacing().item_spacing;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(left, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.spacing_mut().item_spacing = item_spacing;
                        self.tool(ui, if tasks_left { 1 } else { 2 }, height)
                    },
                );
                self.task_splitter(ui, height, available, tasks, tasks_left);
                ui.allocate_ui_with_layout(
                    egui::vec2(right, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.spacing_mut().item_spacing = item_spacing;
                        self.tool(ui, if tasks_left { 2 } else { 1 }, height)
                    },
                );
            });
        });
    }

    fn task_splitter(
        &mut self,
        ui: &mut Ui,
        height: f32,
        available: f32,
        tasks: f32,
        tasks_left: bool,
    ) {
        let palette = web_theme::colors(ui.ctx());
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(TASK_SPLIT_WIDTH, height), egui::Sense::hover());
        let response = ui
            .interact(rect, task_split_id(), egui::Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
            .on_hover_text(
                "Resize Tasks and timeline · Left/Right arrows · Shift for larger steps · Double-click to reset",
            );
        if response.clicked() || response.drag_started() {
            response.request_focus();
        }
        let direction = if tasks_left { 1.0 } else { -1.0 };
        let bounded_width = |width: f32| {
            width.clamp(
                (available * 0.2).max(300.0),
                (available * 0.8).min(available - 330.0),
            )
        };
        if response.double_clicked() {
            self.commit_task_split(None);
            ui.ctx().request_repaint();
        } else {
            if response.dragged() || response.drag_stopped() {
                // The release event can include the final pointer movement.
                let delta = ui.input(|input| input.pointer.delta().x);
                let next = bounded_width(tasks + direction * delta);
                if (next - tasks).abs() > 0.01 {
                    self.task_split_draft = Some(next / available * 100.0);
                    ui.ctx().request_repaint();
                }
            }
            if response.drag_stopped() {
                if let Some(percent) = self.task_split_draft {
                    self.commit_task_split(Some(percent.round().clamp(20.0, 80.0) as u8));
                    ui.ctx().request_repaint();
                }
            }
            if response.has_focus() {
                ui.memory_mut(|memory| {
                    memory.set_focus_lock_filter(
                        task_split_id(),
                        egui::EventFilter {
                            horizontal_arrows: true,
                            ..Default::default()
                        },
                    );
                });
                let keyboard = ui.input_mut(|input| {
                    // A pointer gesture owns its draft until release, including
                    // when an arrow key is pressed during that gesture.
                    if input.pointer.any_down() || response.drag_stopped() {
                        return 0.0;
                    }
                    let arrow = input.events.iter().find_map(|event| match event {
                        egui::Event::Key {
                            key,
                            pressed: true,
                            modifiers,
                            ..
                        } if matches!(key, egui::Key::ArrowLeft | egui::Key::ArrowRight)
                            && !modifiers.ctrl
                            && !modifiers.alt
                            && !modifiers.command =>
                        {
                            Some((*key, *modifiers))
                        }
                        _ => None,
                    });
                    let Some((key, modifiers)) = arrow else {
                        return 0.0;
                    };
                    if !input.consume_key(modifiers, key) {
                        return 0.0;
                    }
                    let step = if modifiers.shift { 5.0 } else { 1.0 };
                    if key == egui::Key::ArrowLeft {
                        -step
                    } else {
                        step
                    }
                });
                if keyboard != 0.0 {
                    let next = bounded_width(tasks + direction * keyboard * available / 100.0);
                    if (next - tasks).abs() > 0.01 {
                        self.commit_task_split(Some(
                            (next / available * 100.0).round().clamp(20.0, 80.0) as u8,
                        ));
                        ui.ctx().request_repaint();
                    }
                }
            }
        }
        response.widget_info(|| {
            egui::WidgetInfo::slider(
                ui.is_enabled(),
                f64::from(tasks / available * 100.0),
                "Tasks panel width",
            )
        });
        let active = response.hovered() || response.dragged() || response.has_focus();
        let color = if active { palette.accent } else { palette.rule };
        let center = rect.center();
        let line_x = center
            .x
            .round_to_pixel_center(ui.painter().pixels_per_point());
        ui.painter()
            .vline(line_x, rect.y_range(), Stroke::new(1.0_f32, color));
        let grip = egui::Rect::from_center_size(center, egui::vec2(TASK_SPLIT_WIDTH, 22.0));
        ui.painter().rect_filled(grip, 2.0, palette.surface);
        for sign in [-1.0, 1.0] {
            ui.painter().add(egui::Shape::line(
                vec![
                    center + egui::vec2(sign * 2.0, -3.0),
                    center + egui::vec2(sign * 5.0, 0.0),
                    center + egui::vec2(sign * 2.0, 3.0),
                ],
                Stroke::new(
                    1.2_f32,
                    if active {
                        palette.accent
                    } else {
                        palette.muted
                    },
                ),
            ));
        }
        if response.has_focus() {
            ui.painter().rect_stroke(
                grip,
                2.0,
                Stroke::new(1.0_f32, palette.accent),
                egui::StrokeKind::Inside,
            );
        }
    }

    fn apply_chart_action(&mut self, action: ChartAction) {
        if matches!(action, ChartAction::BackToGrid) {
            self.set_workspace(true, &self.ctx.clone());
            return;
        }
        let mut book = self.state.workbook.clone().unwrap_or_default();
        let result = match &action {
            ChartAction::Add {
                kind,
                source,
                title,
            } => book.add_chart(*kind, *source, title).map(|_| true),
            ChartAction::Rename { id, title } => book.rename_chart(*id, title),
            ChartAction::Remove(id) => book.remove_chart(*id),
            ChartAction::ControlAdd {
                label,
                target,
                kind,
            } => book.add_control(label, *target, *kind).map(|_| true),
            ChartAction::ControlEdit {
                id,
                label,
                target,
                kind,
            } => book.update_control(*id, label, *target, *kind),
            ChartAction::ControlRemove(id) => book.remove_control(*id),
            ChartAction::ControlMove { id, delta } => book.move_control(*id, *delta),
            ChartAction::ControlValue { id, value } => book.set_control_value(*id, *value),
            ChartAction::BackToGrid => Ok(false),
        };
        match result {
            Ok(changed) => {
                self.charts.accept_action(&action);
                if changed {
                    self.state.workbook = Some(book);
                    self.changed();
                }
            }
            Err(error) => {
                self.status = error.clone();
                self.charts.reject_action(&action, error);
            }
        }
    }

    fn cancel_document_import(&mut self) {
        self.pending_import = None;
        self.pending_csv_target = None;
        self.pending_paste_target = None;
        self.pdf_target = None;
        self.pdf_preview = None;
        self.pdf_documents.clear();
        self.pdf_errors.clear();
        self.ocr_original = None;
        self.ocr_running = false;
        browser_storage::cancel_documents();
    }

    fn import_rows(&mut self, sheet: u64, origin: CellAddr, rows: Vec<Vec<String>>) -> bool {
        self.import_formatted_rows(sheet, origin, rows, &[], &[])
    }

    fn import_formatted_rows(
        &mut self,
        sheet: u64,
        origin: CellAddr,
        rows: Vec<Vec<String>>,
        date_columns: &[usize],
        amount_columns: &[usize],
    ) -> bool {
        if !self.flush_notes_edit() {
            return false;
        }
        let apply = |book: &mut Workbook, target| -> Result<bool, String> {
            let mut changed = book.set_block(target, origin, &rows)?;
            if !rows.is_empty() {
                for (columns, format) in [
                    (date_columns, spreadsheet::NumberFormat::Date),
                    (
                        amount_columns,
                        spreadsheet::NumberFormat::Currency("USD".into()),
                    ),
                ] {
                    for column in columns {
                        let start = CellAddr::new(origin.row, origin.col + column);
                        let end = CellAddr::new(origin.row + rows.len() - 1, start.col);
                        changed |= book.patch_style(
                            target,
                            spreadsheet::CellRange::new(start, end),
                            spreadsheet::StylePatch {
                                number_format: Some(format.clone()),
                                ..Default::default()
                            },
                        )?;
                    }
                }
            }
            Ok(changed)
        };
        let mut book = self.state.workbook.clone().unwrap_or_default();
        match apply(&mut book, sheet) {
            Ok(changed) => {
                if changed {
                    self.state.workbook = Some(book);
                    self.changed();
                }
                true
            }
            Err(error) => {
                self.status = error;
                false
            }
        }
    }

    fn process_pdf_events(&mut self) {
        if self.pdf_target.is_none() || self.pdf_preview.is_some() {
            return;
        }
        if self.ocr_running {
            match browser_storage::take_ocr() {
                Ok(Some(json)) => {
                    self.ocr_running = false;
                    match jig::pdf_import::ExtractedPdf::from_ocr_json(&json) {
                        Ok(document) if document.has_text() => {
                            let document = match self.ocr_original.take() {
                                Some(original) => {
                                    jig::pdf_import::ExtractedPdf::merge_ocr(original, document)
                                }
                                None => document,
                            };
                            self.pdf_documents.push(document);
                        }
                        Ok(_) => self
                            .pdf_errors
                            .push("Recognition found no readable text.".into()),
                        Err(error) => self.pdf_errors.push(error),
                    }
                    self.ocr_original = None;
                    browser_storage::next_pdf();
                }
                Ok(None) => return,
                Err(error) => {
                    self.ocr_running = false;
                    self.ocr_original = None;
                    self.pdf_errors.push(error);
                    browser_storage::next_pdf();
                }
            }
        }
        if !self.ocr_running {
            match browser_storage::take_pdf() {
                Ok(Some(file)) => {
                    let pdf = file.mime == "application/pdf"
                        || file.name.to_ascii_lowercase().ends_with(".pdf")
                        || file.bytes.starts_with(b"%PDF-");
                    let extracted = if pdf {
                        jig::pdf_import::extract(&file.bytes)
                    } else {
                        Err("Image requires local text recognition".into())
                    }
                    .map(|mut document| {
                        document.set_name(&file.name);
                        document
                    });
                    match extracted {
                        Ok(mut document)
                            if document.has_text()
                                && document
                                    .pages
                                    .iter()
                                    .all(|page| !page.text.trim().is_empty()) =>
                        {
                            document.set_name(&file.name);
                            self.pdf_documents.push(document);
                            browser_storage::next_pdf();
                        }
                        original => match browser_storage::start_ocr(&file) {
                            Ok(()) => {
                                self.ocr_original = original.ok();
                                self.ocr_running = true;
                                self.status = format!(
                                    "Recognizing {} locally. Review amounts before importing.",
                                    file.name
                                );
                            }
                            Err(error) => {
                                self.pdf_errors.push(format!("{}: {error}", file.name));
                                browser_storage::next_pdf();
                            }
                        },
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    self.pdf_errors.push(error);
                    browser_storage::next_pdf();
                }
            }
        }
        if !self.ocr_running && browser_storage::remaining_pdf() == 0 {
            if !self.pdf_documents.is_empty() {
                let document =
                    jig::pdf_import::ExtractedPdf::combine(std::mem::take(&mut self.pdf_documents));
                let (sheet, origin) = self.pdf_target.unwrap();
                self.pdf_preview = Some(PdfImportPanel::new(document, sheet, origin));
                self.status =
                    "Documents read locally. Correct and select rows before importing.".into();
            } else if !self.pdf_errors.is_empty() {
                self.status = format!("No rows imported: {}", self.pdf_errors.join(" · "));
                self.pdf_target = None;
                self.pdf_errors.clear();
            }
        }
    }

    fn document_import_panel(&mut self, ui: &mut Ui) -> bool {
        let palette = web_theme::colors(ui.ctx());
        if self.pdf_target.is_none() {
            return false;
        }
        if self.pdf_preview.is_none() {
            let mut cancel = false;
            ui.horizontal_wrapped(|ui| {
                if self.ocr_running {
                    let progress = browser_storage::ocr_progress();
                    let status: serde_json::Value =
                        serde_json::from_str(&progress).unwrap_or_default();
                    let value = status["progress"].as_f64().unwrap_or(0.0).clamp(0.0, 1.0) as f32;
                    let stage = status["stage"]
                        .as_str()
                        .unwrap_or("Recognizing text locally");
                    ui.add(
                        egui::ProgressBar::new(value)
                            .desired_width(180.0)
                            .text(stage),
                    );
                } else {
                    ui.label("Choose PDF documents or transaction screenshots to review locally.");
                }
                cancel = ui.button("Cancel import").clicked();
            });
            if cancel {
                self.cancel_document_import();
            }
            return self.ocr_running || browser_storage::remaining_pdf() > 0;
        }
        for error in &self.pdf_errors {
            ui.colored_label(palette.error, format!("File skipped: {error}"));
        }
        let action = self.pdf_preview.as_mut().and_then(|preview| preview.ui(ui));
        match action {
            Some(PdfImportAction::Cancel) => self.cancel_document_import(),
            Some(PdfImportAction::Apply {
                sheet,
                origin,
                rows,
                date_columns,
                amount_columns,
            }) => {
                let count = rows.len();
                if self.import_formatted_rows(sheet, origin, rows, &date_columns, &amount_columns) {
                    self.cancel_document_import();
                    self.sheet.request_focus();
                    self.status = format!(
                        "Imported {count} reviewed rows locally. Undo restores the previous sheet."
                    );
                }
            }
            None => {}
        }
        true
    }

    fn process_document_events(&mut self) {
        match browser_storage::take_csv() {
            Ok(Some(text)) => {
                if let Some((sheet, origin)) = self.pending_csv_target.take() {
                    match spreadsheet::parse_delimited(&text, ',') {
                        Ok(rows) => {
                            self.import_rows(sheet, origin, rows);
                        }
                        Err(error) => self.status = format!("CSV import rejected: {error}"),
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                self.pending_csv_target = None;
                self.status = error;
            }
        }
        match browser_storage::take_clipboard() {
            Ok(Some(text)) => {
                if let Some((sheet, origin)) = self.pending_paste_target.take() {
                    match self.sheet.paste_at(sheet, origin, &text) {
                        Ok(action) => {
                            self.apply_sheet_action(action);
                        }
                        Err(error) => self.status = error,
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                self.pending_paste_target = None;
                self.status = error;
            }
        }
        match browser_storage::take_ods() {
            Ok(Some(bytes)) => match jig::documents::read_ods(&bytes) {
                Ok(import) => {
                    let state = match import.embedded_state {
                        Some(state) => state,
                        None => {
                            let mut state = self.state.clone();
                            state.workbook = Some(import.workbook);
                            state
                        }
                    };
                    self.pending_import = Some(state);
                    self.status = "ODS read locally. Review the import summary before replacing the current document.".into();
                }
                Err(error) => {
                    self.status = format!("ODS import rejected; current data is unchanged: {error}")
                }
            },
            Ok(None) => {}
            Err(error) => self.status = error,
        }
        self.process_pdf_events();
    }

    fn focus_after_resize(&mut self, ctx: &egui::Context) {
        let editing = ctx
            .memory(|memory| memory.focused())
            .is_some_and(|id| TextEdit::load_state(ctx, id).is_some());
        if editing {
            return;
        }
        if self.focus == 2
            && self.timeline_view
            && self.task_date_edit.is_none()
            && self.task_date_legacy.is_none()
        {
            self.timeline.request_focus();
        } else if self.focus == 2 {
            self.calendar_panel.request_focus();
        }
        self.scroll_selected = true;
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
        // This path is reached only after explicit replacement confirmation.
        self.storage_error = false;
        self.reset_editors();
        self.reset_document_session();
        self.save_current();
    }

    fn open_local(&mut self) {
        if self.pending_chart_input || self.charts.has_draft() {
            self.status =
                "Save or cancel chart/interface edits in Charts before opening data.".into();
            return;
        }
        if self.pending_task_input.is_some() || self.has_task_draft() || self.has_schedule_draft() {
            self.status = "Save or clear task edits before opening another dataset.".into();
            return;
        }
        if !self.flush_sheet_edit() || !self.flush_notes_edit() {
            return;
        }
        if self.dirty {
            self.status =
                "Save your changes before opening another dataset. If saving is unavailable, export JSON, then reload to reopen the saved copy."
                    .into();
        } else {
            match browser_storage::load() {
                Ok(state) => {
                    let has_saved_personal = state.is_some();
                    self.state = state.unwrap_or_default();
                    self.restore_weather();
                    self.storage_error = false;
                    self.reset_editors();
                    self.reset_document_session();
                    self.initial_backup_pending = self.settings.auto_backup && has_saved_personal;
                    self.status = "Opened your browser-local data.".into();
                }
                Err(error) => {
                    self.storage_error = true;
                    self.status = format!("Stored data is preserved: {error}");
                }
            }
        }
    }

    fn info_button(&mut self, ui: &mut Ui, page: InfoPage) {
        if ui
            .add(egui::Button::new(page.label()).selected(self.info_page == Some(page)))
            .clicked()
        {
            self.deferred_view = Some(DeferredView::Info(page));
        }
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        let palette = web_theme::colors(ui.ctx());
        ui.horizontal_wrapped(|ui| {
            ui.set_min_height(28.0);
            self.info_button(ui, InfoPage::About);
            if ui
                .add(
                    egui::Button::new("Data")
                        .selected(self.data_open)
                        .corner_radius(0.0)
                        .stroke(Stroke::new(
                            1.0_f32,
                            if self.data_open {
                                palette.accent
                            } else {
                                palette.rule
                            },
                        )),
                )
                .clicked()
            {
                self.settings_panel.cancel_capture();
                crate::settings_storage::set_capture(false);
                self.data_open = !self.data_open;
            }
            self.workspace_buttons(ui);
            self.settings_button(ui);
            self.info_button(ui, InfoPage::Help);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.calculator_button(ui);
            });
        });
        if self.data_open {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui.button("Start empty").clicked() {
                    self.confirm = Some(Confirm::Empty);
                }
                if ui.button("Open local data").clicked() {
                    self.open_local();
                }
                if ui.button("Import JSON").clicked()
                    && self.flush_sheet_edit()
                    && self.flush_notes_edit()
                {
                    self.cancel_document_import();
                    if let Err(error) = browser_storage::request_import() {
                        self.status = error;
                    }
                }
                if ui.button("Export JSON").clicked()
                    && self.flush_sheet_edit()
                    && self.flush_notes_edit()
                {
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
                ui.label(RichText::new(match confirm { Confirm::Empty => "Replace this browser's saved data with an empty document? Export first if needed.", Confirm::Clear => "Delete this browser's saved Jig data? Export a backup first if needed." }).color(palette.error));
                if ui.button("Confirm").clicked() {
                    self.confirm = None;
                    match confirm {
                        Confirm::Empty => self.replace_personal(State::default()),
                        Confirm::Clear => match browser_storage::clear() {
                            Ok(()) => {
                                self.weather.disable();
                                self.state = State::default();
                                self.dirty = false;
                                self.storage_error = false;
                                self.reset_editors();
                                self.reset_document_session();
                                self.status = "Browser-local data cleared. Start a new task, note or sheet to save new work.".into();
                            }
                            Err(error) => self.status = error,
                        },
                    }
                }
                if ui.button("Cancel").clicked() { self.confirm = None; }
            });
        }
        if let Some(import) = &self.pending_import {
            let sheets = import.workbook.as_ref().map_or(0, |book| book.sheets.len());
            let cells = import.workbook.as_ref().map_or(0, |book| {
                book.sheets.iter().map(|sheet| sheet.cells.len()).sum()
            });
            let summary = format!("Import {sheets} sheets / {cells} cells, {} tasks, {} notes, {} deadlines and {} calculations? This replaces the browser's saved Jig data.", import.tasks.len(), import.notes.len(), import.deadlines.len(), import.history.len());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(summary).color(palette.accent));
                if ui.button("Confirm import").clicked() && self.flush_notes_edit() {
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
        let palette = web_theme::colors(ui.ctx());
        let selected = self.focus == index;
        let frame = egui::Frame::default()
            .fill(if selected {
                palette.surface
            } else {
                palette.bg
            })
            .inner_margin(egui::Margin::same(9))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height((height - 18.0).max(45.0));
                match index {
                    0 => self.deadlines(ui),
                    1 => {
                        let drop_rect = egui::Rect::from_min_size(
                            ui.cursor().min,
                            egui::vec2(ui.available_width(), (height - 18.0).max(180.0)),
                        )
                        .intersect(ui.clip_rect());
                        let scroll = egui::ScrollArea::vertical()
                            .id_salt("checklist")
                            .max_height((height - 18.0).max(180.0))
                            .show(ui, |ui| self.tasks(ui));
                        self.task_drop_zone(ui, drop_rect, &scroll.inner);
                    }
                    2 => self.calendar(ui, (height - 18.0).max(230.0)),
                    3 => self.calculator(ui, (height - 18.0).max(120.0)),
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
        let color = if self.focus == index {
            palette.accent
        } else {
            palette.rule
        };
        if selected != (self.focus == index) {
            ui.ctx().request_repaint();
        }
        ui.painter().hline(
            frame.response.rect.x_range(),
            frame.response.rect.top(),
            Stroke::new(1.0_f32, color),
        );
    }

    fn open_calculator(&mut self, ctx: &egui::Context) {
        // Open after the current editors have processed this frame's input.
        self.deferred_view = Some(DeferredView::Calculator);
        ctx.request_repaint();
    }

    fn show_calendar(&mut self, ctx: &egui::Context, month: bool) {
        if self.has_pending_editor_input() {
            self.deferred_view = Some(DeferredView::CalendarMonth(month));
            ctx.request_repaint();
            return;
        }
        // The key that opens this view must not also choose a task date.
        crate::text_input::consume_opening_keys(ctx);
        if self.timeline_view {
            self.toggle_calendar(ctx);
        }
        if self.calendar_panel.month_view() != month {
            self.calendar_panel
                .set_month_view(month, self.selected_date);
        }
        self.calendar_panel.request_focus();
        ctx.request_repaint();
    }

    fn toggle_calendar(&mut self, ctx: &egui::Context) {
        if self.has_pending_editor_input() {
            self.deferred_view = Some(DeferredView::Calendar);
            ctx.request_repaint();
            return;
        }
        if !self.tasks_workspace && !self.show_tasks(ctx) {
            return;
        }
        self.close_settings();
        self.info_page = None;
        ctx.memory_mut(|memory| {
            memory.close_popup();
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        self.cancel_task_note_drag(ctx);
        self.focus = 2;
        self.focus_calculator = false;
        self.focus_task_date = false;
        self.scroll_selected = true;
        self.timeline_view = !self.timeline_view;
        self.timeline.cancel_gesture();
        if self.timeline_view {
            self.timeline.reveal(self.selected_date);
            self.timeline.request_focus();
        } else {
            self.calendar_panel.reveal(self.selected_date);
            self.calendar_panel.request_focus();
        }
        ctx.request_repaint();
    }

    fn calculator_button(&mut self, ui: &mut Ui) {
        let palette = web_theme::colors(ui.ctx());
        let popup = egui::Id::new("calculator-popup");
        let (rect, response) = ui.allocate_exact_size(egui::vec2(30.0, 28.0), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Calculator")
        });
        let visuals = ui.style().interact(&response);
        ui.painter().rect(
            rect,
            2.0,
            visuals.bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        let icon = rect.shrink2(egui::vec2(7.0, 4.0));
        ui.painter().rect_stroke(
            icon,
            1.0,
            Stroke::new(1.0_f32, palette.text),
            egui::StrokeKind::Inside,
        );
        ui.painter().hline(
            (icon.left() + 3.0)..=(icon.right() - 3.0),
            icon.top() + 5.0,
            Stroke::new(1.0_f32, palette.text),
        );
        for y in [icon.top() + 10.0, icon.top() + 14.0] {
            for x in [icon.left() + 4.0, icon.right() - 4.0] {
                ui.painter()
                    .circle_filled(egui::pos2(x, y), 1.0, palette.text);
            }
        }
        if response.clicked() {
            if ui.memory(|memory| memory.is_popup_open(popup)) {
                ui.memory_mut(|memory| memory.close_popup());
            } else {
                self.open_calculator(ui.ctx());
            }
        }
        let response = response.on_hover_text(format!(
            "Calculator and unit conversion · {}",
            self.shortcut_label(AppAction::Calculator)
        ));
        egui::popup::popup_below_widget(
            ui,
            popup,
            &response,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                ui.set_width((ui.ctx().screen_rect().width() - 28.0).clamp(220.0, 380.0));
                self.calculator(ui, 260.0);
            },
        );
    }

    fn calculator(&mut self, ui: &mut Ui, height: f32) {
        let top = ui.cursor().top();
        self.calculator_input(ui);
        ui.separator();
        let used = ui.cursor().top() - top;
        egui::ScrollArea::vertical()
            .id_salt("math-history")
            .max_height((height - used).max(70.0))
            .show(ui, |ui| self.calculator_history(ui));
    }

    fn calculator_input(&mut self, ui: &mut Ui) {
        let palette = web_theme::colors(ui.ctx());
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
                    .color(palette.text),
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
            if response.changed() {
                self.changed();
            }
        });
    }

    fn calculator_history(&mut self, ui: &mut Ui) {
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

    fn edit_task_target(&mut self, target: TaskEditorTarget, ctx: &egui::Context) {
        if self.pending_task_input.is_some() {
            self.deferred_task_edit = Some(target);
            ctx.request_repaint();
            return;
        }
        let existing = match target {
            TaskEditorTarget::Task(id) => Some(id),
            TaskEditorTarget::Deadline(id) => self
                .state
                .tasks
                .iter()
                .find(|task| task.deadline == Some(id))
                .map(|task| task.id),
        };
        if existing.is_some() && self.task_edit == existing {
            self.focus_task_input = true;
            ctx.request_repaint();
            return;
        }
        if self.has_task_draft() {
            self.status = "Save or clear the current task edit before editing another task.".into();
            return;
        }
        let id = match (existing, target) {
            (Some(id), _) => Ok(id),
            (None, TaskEditorTarget::Deadline(id)) => self
                .state
                .ensure_task_for_deadline(id)
                .inspect(|_| self.changed()),
            (None, TaskEditorTarget::Task(_)) => Err("Task not found".into()),
        };
        match id {
            Ok(id) => {
                if let Some(task) = self.state.tasks.iter().find(|task| task.id == id) {
                    self.task_edit = Some(id);
                    self.task_edit_original = serde_json::to_string(task).ok();
                    self.task_text = task.text.clone();
                    self.task_details = task.details.clone().unwrap_or_default();
                    self.task_edit_list = Some(task.list_id);
                }
            }
            Err(error) => self.status = error,
        }
    }

    fn task_input_processed(&mut self, id: egui::Id) {
        if self.pending_task_input == Some(id) {
            self.pending_task_input = None;
        }
    }

    fn begin_task_date(&mut self, id: u64, feature_after: bool) -> bool {
        if self.pending_task_input.is_some() {
            self.status = "Finish the current task input before opening a date editor.".into();
            return false;
        }
        if self.has_schedule_draft() {
            if self.task_date_edit == Some(id) {
                self.feature_after_date |= feature_after;
                self.focus_task_date = true;
                return true;
            }
            self.status =
                "Save or cancel the current date edit before editing another date.".into();
            return false;
        }
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
        self.capture_schedule_draft();
        true
    }

    fn begin_legacy_date(&mut self, id: u64, timeline: bool) -> bool {
        if self.pending_task_input.is_some() {
            self.status = "Finish the current task input before opening a date editor.".into();
            return false;
        }
        if self.has_schedule_draft() {
            self.status =
                "Save or cancel the current date edit before editing another date.".into();
            return false;
        }
        let Some(deadline) = self.state.deadlines.iter().find(|item| item.id == id) else {
            return false;
        };
        let date = deadline.at.with_timezone(&Local).date_naive();
        self.task_date = date.to_string();
        self.selected_date = date;
        self.calendar_weather_date = None;
        self.calendar_panel.reveal(date);
        self.task_start.clear();
        self.task_date_edit = None;
        self.task_date_legacy = Some(id);
        self.task_date_timeline = timeline;
        self.task_date_error = None;
        self.feature_after_date = false;
        self.focus_task_date = true;
        self.capture_schedule_draft();
        true
    }

    fn task_editor_state(&self, id: u64) -> Result<State, String> {
        let mut state = self.state.clone();
        state.edit_task(id, &self.task_text)?;
        state.set_task_list(id, self.task_edit_list.unwrap_or(self.active_task_list))?;
        state.set_task_details(
            id,
            (!self.task_details.trim().is_empty()).then_some(self.task_details.as_str()),
        )?;
        Ok(state)
    }

    fn task_drag_source(&self, response: &egui::Response, id: u64) {
        if response.hovered() {
            response.ctx.set_cursor_icon(egui::CursorIcon::Grab);
        }
        if response.drag_started() {
            egui::DragAndDrop::set_payload(
                &response.ctx,
                TaskDragPayload {
                    id,
                    order: self
                        .state
                        .tasks
                        .iter()
                        .map(|task| (task.id, task.list_id))
                        .collect(),
                    lists: self.selected_task_lists.clone(),
                },
            );
        }
    }

    fn cancel_task_note_drag(&self, ctx: &egui::Context) {
        if egui::DragAndDrop::has_payload_of_type::<TaskDragPayload>(ctx)
            || egui::DragAndDrop::has_payload_of_type::<NoteDragPayload>(ctx)
        {
            egui::DragAndDrop::clear_payload(ctx);
            ctx.stop_dragging();
        }
    }

    fn drag_auto_scroll(&self, ui: &mut Ui) {
        if !(egui::DragAndDrop::has_payload_of_type::<TaskDragPayload>(ui.ctx())
            || egui::DragAndDrop::has_payload_of_type::<NoteDragPayload>(ui.ctx()))
            || !ui.input(|input| input.pointer.any_down())
        {
            return;
        }
        let clip = ui.clip_rect();
        if let Some(pointer) = ui
            .input(|input| input.pointer.interact_pos())
            .filter(|position| clip.contains(*position))
        {
            let edge = if pointer.y < clip.top() + 22.0 {
                1.0
            } else if pointer.y > clip.bottom() - 22.0 {
                -1.0
            } else {
                0.0
            };
            if edge != 0.0 {
                ui.scroll_with_delta(egui::vec2(
                    0.0,
                    edge * 260.0 * ui.input(|input| input.stable_dt.min(0.05)),
                ));
                ui.ctx().request_repaint();
            }
        }
    }

    fn task_drop_zone(&mut self, ui: &mut Ui, rect: egui::Rect, rows: &[TaskDropRow]) {
        let palette = web_theme::colors(ui.ctx());
        if rect.width() < 3.0 || rect.height() < 3.0 {
            return;
        }
        let cancelled = ui.input(|input| {
            !input.focused
                || input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Touch {
                            phase: egui::TouchPhase::Cancel,
                            ..
                        }
                    )
                })
        });
        if cancelled {
            self.cancel_task_note_drag(ui.ctx());
            return;
        }
        let response = ui.interact(rect, ui.id().with("task-drop-zone"), egui::Sense::hover());
        let task = response.dnd_hover_payload::<TaskDragPayload>();
        let note = response.dnd_hover_payload::<NoteDragPayload>();
        if task.is_none() && note.is_none() {
            return;
        }
        if task.as_ref().is_some_and(|drag| {
            drag.lists != self.selected_task_lists
                || !self
                    .state
                    .tasks
                    .iter()
                    .map(|task| (task.id, task.list_id))
                    .eq(drag.order.iter().copied())
        }) || note.as_ref().is_some_and(|drag| {
            !self.notes.is_dragging(drag.id)
                || self.notes.has_draft()
                || !self.state.notes.iter().any(|note| note.id == drag.id)
        }) {
            self.cancel_task_note_drag(ui.ctx());
            return;
        }
        let Some(pointer) = ui.input(|input| input.pointer.interact_pos()) else {
            return;
        };
        let hovered = rows.iter().find(|row| row.rect.contains(pointer));
        let list_id = if note.is_some() {
            hovered.map_or(self.active_task_list, |row| row.list_id)
        } else {
            self.active_task_list
        };
        let eligible: Vec<_> = rows
            .iter()
            .filter(|row| note.is_none() || row.list_id == list_id)
            .collect();
        let target = hovered
            .map(|row| (row.id, pointer.y >= row.rect.center().y))
            .or_else(|| {
                eligible
                    .iter()
                    .find(|row| pointer.y < row.rect.center().y)
                    .map(|row| (row.id, false))
            })
            .or_else(|| eligible.last().map(|row| (row.id, true)));
        let y = target
            .and_then(|(id, after)| {
                rows.iter().find(|row| row.id == id).map(|row| {
                    if after {
                        row.rect.bottom()
                    } else {
                        row.rect.top()
                    }
                })
            })
            .unwrap_or(rect.top() + 8.0)
            .clamp(rect.top() + 1.0, rect.bottom() - 1.0);
        let painter = ui.painter().with_clip_rect(rect);
        painter.hline(rect.x_range(), y, Stroke::new(2.0_f32, palette.accent));
        if note.is_some() {
            let name = self
                .state
                .task_list(list_id)
                .map_or("Tasks", |list| list.name.as_str());
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(1.0_f32, palette.accent),
                egui::StrokeKind::Inside,
            );
            painter.text(
                egui::pos2(rect.left() + 8.0, (y - 16.0).max(rect.top() + 3.0)),
                egui::Align2::LEFT_TOP,
                format!("Add to {name}"),
                FontId::monospace(11.0),
                palette.accent,
            );
        }
        ui.ctx().request_repaint();
        if !ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary)) {
            return;
        }
        if task.is_some() {
            if let Some(drag) = response.dnd_release_payload::<TaskDragPayload>() {
                if let Some((target, after)) = target {
                    let before: Vec<_> = self.state.tasks.iter().map(|task| task.id).collect();
                    match self.state.move_task_relative(drag.id, target, after) {
                        Ok(()) if !self.state.tasks.iter().map(|task| task.id).eq(before) => {
                            self.changed()
                        }
                        Ok(()) => {}
                        Err(error) => self.status = error,
                    }
                }
            }
        } else if let Some(drag) = response.dnd_release_payload::<NoteDragPayload>() {
            let mut state = self.state.clone();
            let result = state.note_to_task(drag.id, list_id).and_then(|id| {
                if let Some((target, after)) = target {
                    state.move_task_relative(id, target, after)?;
                }
                Ok(())
            });
            match result {
                Ok(()) => {
                    self.state = state;
                    self.changed();
                }
                Err(error) => self.status = error,
            }
        }
    }

    fn sort_visible_tasks(&mut self, ctx: &egui::Context) {
        self.cancel_task_note_drag(ctx);
        self.timeline.cancel_gesture();
        let mut next = self.state.clone();
        // Sorting explicitly acts on older deadline-only rows too. Reuse their
        // deadlines and commit together, so a failed conversion leaves work intact.
        if self.selected_task_lists.contains(&next.default_list_id()) {
            let legacy: Vec<_> = next
                .deadlines
                .iter()
                .filter(|deadline| {
                    !next
                        .tasks
                        .iter()
                        .any(|task| task.deadline == Some(deadline.id))
                })
                .map(|deadline| deadline.id)
                .collect();
            for id in legacy {
                if let Err(error) = next.ensure_task_for_deadline(id) {
                    self.status = error;
                    return;
                }
            }
        }
        let reordered = next.sort_tasks_in_lists(&self.selected_task_lists);
        if reordered || next.tasks.len() != self.state.tasks.len() {
            self.state = next;
            self.changed();
            ctx.request_repaint();
        }
    }

    fn tasks(&mut self, ui: &mut Ui) -> Vec<TaskDropRow> {
        let palette = web_theme::colors(ui.ctx());
        let mut targets = Vec::new();
        let mut rows: Vec<_> = self
            .state
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, task)| self.selected_task_lists.contains(&task.list_id))
            .map(|(index, task)| ChecklistRow {
                task: Some(task.id),
                deadline: task.deadline,
                title: task.text.clone(),
                complete: task.complete,
                color: task.color,
                position: Some(index),
                list_id: task.list_id,
            })
            .collect();
        // Older files keep their independent deadlines until a row is acted on.
        for deadline in &self.state.deadlines {
            if self
                .selected_task_lists
                .contains(&self.state.default_list_id())
                && !self
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
                    list_id: self.state.default_list_id(),
                });
            }
        }
        for row in rows {
            let previous = row.position.and_then(|position| {
                self.state.tasks[..position]
                    .iter()
                    .rposition(|task| task.list_id == row.list_id)
            });
            let next = row.position.and_then(|position| {
                self.state
                    .tasks
                    .iter()
                    .enumerate()
                    .skip(position + 1)
                    .find(|(_, task)| task.list_id == row.list_id)
                    .map(|(index, _)| index)
            });
            let combined = self.selected_task_lists.len() > 1;
            let list_index = self
                .state
                .task_lists
                .iter()
                .position(|list| list.id == row.list_id)
                .unwrap_or(0);
            let fill = if combined {
                web_theme::task_list_fill_for(ui.ctx(), list_index)
            } else {
                Color32::TRANSPARENT
            };
            let frame = egui::Frame::default()
                .fill(fill)
                .inner_margin(egui::Margin::same(if combined { 3 } else { 0 }))
                .show(ui, |ui| {
                    ui.push_id(("checklist-row", row.task, row.deadline), |ui| {
                        let mut color_control = None;
                        ui.horizontal_wrapped(|ui| {
                            let sense = egui::Sense::drag()
                                .difference(egui::Sense::focusable_noninteractive());
                            if let Some(id) = row.task {
                                let (rect, grip) =
                                    ui.allocate_exact_size(egui::vec2(20.0, 18.0), sense);
                                for x in [-2.0, 2.0] {
                                    for y in [-4.0, 0.0, 4.0] {
                                        ui.painter().circle_filled(
                                            rect.center() + egui::vec2(x, y),
                                            1.0,
                                            palette.muted,
                                        );
                                    }
                                }
                                self.task_drag_source(&grip, id);
                            } else {
                                ui.allocate_space(egui::vec2(20.0, 18.0));
                            }
                            let featured =
                                row.deadline.is_some() && row.deadline == self.state.featured;
                            if ui
                                .add_enabled(
                                    !row.complete,
                                    egui::Button::new(
                                        RichText::new(if featured { "★" } else { "☆" }).color(
                                            if featured {
                                                palette.accent
                                            } else {
                                                palette.muted
                                            },
                                        ),
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
                                        Ok(id) => {
                                            self.begin_task_date(id, true);
                                        }
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
                                let label = format!(
                                    "Color: {}",
                                    row.color.map_or("None", TaskColor::label)
                                );
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
                                    self.task_color_edit = if self.task_color_edit == Some(deadline)
                                    {
                                        None
                                    } else {
                                        Some(deadline)
                                    };
                                }
                            }
                            let text = RichText::new(&row.title).color(if row.complete {
                                palette.muted
                            } else {
                                palette.text
                            });
                            ui.label(if row.complete {
                                text.strikethrough()
                            } else {
                                text
                            });
                            if combined {
                                if let Some(list) = self.state.task_list(row.list_id) {
                                    ui.label(
                                        RichText::new(&list.name).small().color(palette.muted),
                                    );
                                }
                            }
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
                                if let Some(id) = row.task {
                                    self.begin_task_date(id, false);
                                } else if let Some(id) = row.deadline {
                                    self.begin_legacy_date(id, false);
                                }
                            }
                            if ui.small_button("edit").clicked() {
                                if let Some(id) = row.task {
                                    self.edit_task_target(TaskEditorTarget::Task(id), ui.ctx());
                                } else if let Some(id) = row.deadline {
                                    self.edit_task_target(TaskEditorTarget::Deadline(id), ui.ctx());
                                }
                            }
                            if ui
                                .add_enabled(previous.is_some(), egui::Button::new("↑").small())
                                .clicked()
                            {
                                if let (Some(id), Some(position)) = (row.task, previous) {
                                    let result = self.state.move_task(id, position + 1);
                                    self.apply(result);
                                }
                            }
                            if ui
                                .add_enabled(next.is_some(), egui::Button::new("↓").small())
                                .clicked()
                            {
                                if let (Some(id), Some(position)) = (row.task, next) {
                                    let result = self.state.move_task(id, position + 1);
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
                                    self.task_edit_original = None;
                                    self.task_edit_list = None;
                                    self.task_text.clear();
                                    self.task_details.clear();
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
                                    let text =
                                        RichText::new(color.map_or("None", TaskColor::label))
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
                        if !self.task_date_timeline {
                            let target = row_id
                                .filter(|id| self.task_date_edit == Some(*id))
                                .or_else(|| {
                                    row.deadline.filter(|id| self.task_date_legacy == Some(*id))
                                });
                            if let Some(id) = target {
                                self.task_date_editor(ui, id);
                            }
                        }
                    });
                });
            if let Some(id) = row.task {
                targets.push(TaskDropRow {
                    id,
                    list_id: row.list_id,
                    rect: frame.response.rect,
                });
            }
        }
        if self.task_edit.is_some() && self.state.task_lists.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                ui.label("List");
                for list in &self.state.task_lists {
                    ui.selectable_value(&mut self.task_edit_list, Some(list.id), &list.name);
                }
            });
        }
        ui.add_space(6.0);
        let mut save_task = false;
        ui.horizontal(|ui| {
            let id = egui::Id::new("task-input");
            let response = ui.add(
                TextEdit::singleline(&mut self.task_text)
                    .id(id)
                    .return_key(None)
                    .hint_text("Add new task")
                    .desired_width((ui.available_width() - 55.0).max(80.0)),
            );
            self.task_input_processed(id);
            response.clone().on_hover_text(format!(
                "New tasks go to {}",
                self.state
                    .task_list(self.active_task_list)
                    .map_or("Tasks", |list| list.name.as_str())
            ));
            if response.has_focus() {
                self.focus = 1;
            }
            if response.gained_focus() {
                response.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
            }
            if self.focus_task_input {
                response.request_focus();
                response.scroll_to_me(Some(egui::Align::Center));
                self.focus_task_input = false;
                ui.ctx().request_repaint();
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
                save_task = true;
            }
        });
        if !self.task_details.is_empty()
            || self.task_edit.is_some_and(|id| {
                self.state
                    .tasks
                    .iter()
                    .any(|task| task.id == id && task.details.is_some())
            })
        {
            ui.label(RichText::new("Details").small().color(palette.muted));
            let id = egui::Id::new("task-details-input");
            ui.add(
                TextEdit::multiline(&mut self.task_details)
                    .id(id)
                    .desired_width(f32::INFINITY)
                    .desired_rows(4)
                    .char_limit(jig::model::MAX_NOTE_BYTES),
            );
            self.task_input_processed(id);
        }
        if self.task_edit.is_some() && ui.small_button("Cancel edit").clicked() {
            save_task = false;
            self.task_edit = None;
            self.task_edit_original = None;
            self.task_edit_list = None;
            self.task_text.clear();
            self.task_details.clear();
        }
        if let Some(id) = self.task_edit {
            if ui
                .add_enabled(
                    self.state.notes.len() < jig::model::MAX_NOTES,
                    egui::Button::new("To note").small(),
                )
                .on_hover_text(
                    "Move this task to Notes; its dates and list are kept for conversion back",
                )
                .clicked()
            {
                save_task = false;
                let converted = self.task_editor_state(id).and_then(|mut state| {
                    state.task_to_note(id)?;
                    Ok(state)
                });
                match converted {
                    Ok(state) => {
                        self.state = state;
                        self.task_edit = None;
                        self.task_edit_original = None;
                        self.task_edit_list = None;
                        self.task_text.clear();
                        self.task_details.clear();
                        if self.task_date_edit == Some(id) {
                            self.close_task_dates();
                        }
                        self.changed();
                        ui.ctx().request_repaint();
                    }
                    Err(error) => self.status = error,
                }
            }
        }
        // Process the optional multiline editor before committing this frame.
        if save_task {
            let result = if let Some(id) = self.task_edit {
                self.task_editor_state(id).map(|state| self.state = state)
            } else {
                if self.task_details.is_empty() {
                    self.state
                        .add_task_to_list(self.active_task_list, &self.task_text)
                        .map(|_| ())
                } else {
                    let mut state = self.state.clone();
                    state
                        .add_task_to_list(self.active_task_list, &self.task_text)
                        .and_then(|id| {
                            state.set_task_details(id, Some(&self.task_details))?;
                            self.state = state;
                            Ok(())
                        })
                }
            };
            if result.is_ok() {
                self.task_text.clear();
                self.task_details.clear();
                self.task_edit = None;
                self.task_edit_original = None;
                self.task_edit_list = None;
            }
            self.apply(result);
        }
        let completed = self
            .state
            .tasks
            .iter()
            .filter(|task| task.complete && self.selected_task_lists.contains(&task.list_id))
            .count();
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if completed > 0 {
                if self.confirm_clear_completed {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("Delete {completed} completed tasks?"));
                        if ui.small_button("Confirm").clicked() {
                            self.state
                                .remove_completed_tasks_in_lists(&self.selected_task_lists);
                            if self.task_edit.is_some_and(|id| {
                                !self.state.tasks.iter().any(|task| task.id == id)
                            }) {
                                self.task_edit = None;
                                self.task_edit_original = None;
                                self.task_edit_list = None;
                                self.task_text.clear();
                                self.task_details.clear();
                            }
                            if self.task_date_edit.is_some_and(|id| {
                                !self.state.tasks.iter().any(|task| task.id == id)
                            }) {
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
            let sort = ui.small_button("Sort").on_hover_text(concat!(
                "Sort selected lists: earliest due first, undated tasks in their ",
                "current order, completed tasks last. Undo restores the previous order."
            ));
            if sort.gained_focus() {
                sort.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
            }
            if sort.clicked() {
                self.sort_visible_tasks(ui.ctx());
            }
        });
        self.drag_auto_scroll(ui);
        targets
    }

    fn close_task_dates(&mut self) {
        self.ctx.memory_mut(|memory| memory.close_popup());
        self.calendar_task_date_target = None;
        self.task_date_edit = None;
        self.task_date_legacy = None;
        self.task_date_timeline = false;
        self.task_date_original = None;
        self.task_date_initial = None;
        self.task_date_error = None;
        self.feature_after_date = false;
        self.focus_task_date = false;
    }

    fn resize_task_dates(
        &mut self,
        id: u64,
        edge: timeline::ResizeEdge,
        date: NaiveDate,
    ) -> Result<(), String> {
        if self.task_date_edit == Some(id) && self.has_schedule_draft() {
            return Err("Save or cancel the date edit before resizing its timeline bar.".into());
        }
        let (start, due) = timeline::resize_schedule(&self.state, id, edge, date, &Local)?;
        let old_start = self
            .state
            .tasks
            .iter()
            .find(|task| task.id == id)
            .and_then(|task| task.start);
        let old_due = self.state.task_deadline(id).map(|deadline| deadline.at);
        if old_start == Some(start) && old_due == Some(due) {
            return Ok(());
        }
        let mut next = self.state.clone();
        next.set_task_schedule(id, Some(&start.to_rfc3339()), Some(&due.to_rfc3339()))?;
        self.state = next;
        if self.task_date_edit == Some(id) {
            self.close_task_dates();
        }
        self.changed();
        Ok(())
    }

    fn save_task_dates(&mut self, id: u64) -> Result<u64, String> {
        let existing_due = match self.task_date_legacy {
            Some(deadline) => self.state.deadlines.iter().find(|item| item.id == deadline),
            None => self.state.task_deadline(id),
        }
        .map(|deadline| deadline.at);
        let due = jig::task_dates::resolve(
            &self.task_date,
            existing_due,
            chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            &Local,
        )?
        .to_rfc3339();
        let existing_start = self
            .state
            .tasks
            .iter()
            .find(|task| task.id == id)
            .and_then(|task| task.start);
        let start = if self.task_start.trim().is_empty() {
            None
        } else {
            Some(
                jig::task_dates::resolve(
                    &self.task_start,
                    existing_start,
                    chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
                    &Local,
                )?
                .to_rfc3339(),
            )
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
        let palette = web_theme::colors(ui.ctx());
        ui.indent(("task-date-editor", id), |ui| {
            if self.focus_task_date {
                // Opening an editor with Enter/Space must not also submit its
                // newly focused field in this same frame.
                crate::text_input::consume_opening_keys(ui.ctx());
            }
            let owner = if self.task_date_timeline { 2 } else { 1 };
            let field = egui::Id::new(("task-date-input", id));
            ui.label(RichText::new("Due").color(palette.muted).size(11.0));
            if self.focus_task_date {
                ui.memory_mut(|memory| memory.request_focus(field));
            }
            let existing_due = match self.task_date_legacy {
                Some(deadline) => self.state.deadlines.iter().find(|item| item.id == deadline),
                None => self.state.task_deadline(id),
            }
            .map(|deadline| deadline.at);
            let response = ui
                .horizontal(|ui| {
                    let response = ui.add(
                        TextEdit::singleline(&mut self.task_date)
                            .id(field)
                            .return_key(None)
                            .hint_text("YYYY-MM-DD HH:MM")
                            .desired_width((ui.available_width() - 36.0).max(100.0)),
                    );
                    if crate::date_picker_ui::button(
                        ui,
                        egui::Id::new(("task-due-picker", id)),
                        &mut self.task_date,
                        existing_due,
                        chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                    ) {
                        self.task_date_error = None;
                        response.request_focus();
                        ui.input_mut(|input| {
                            input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                        });
                    }
                    response
                })
                .inner
                .on_hover_text(concat!(
                    "Enter a local date and time, or a timestamp with a timezone. ",
                    "Date-only edits retain the existing time; new deadlines use 09:00 local time."
                ));
            self.task_input_processed(field);
            if response.has_focus() {
                self.focus = owner;
                self.calendar_task_date_target = Some(TaskDateField::Due);
            }
            if self.focus_task_date {
                response.scroll_to_me(None);
                self.focus_task_date = !response.has_focus();
                ui.ctx().request_repaint();
            }
            let mut enter = response.has_focus()
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            ui.label(
                RichText::new("Start · optional")
                    .color(palette.muted)
                    .size(11.0),
            );
            let start_field = egui::Id::new(("task-start-input", id));
            let existing_start = self
                .state
                .tasks
                .iter()
                .find(|task| task.id == id)
                .and_then(|task| task.start);
            let start = ui
                .horizontal(|ui| {
                    let response = ui.add(
                        TextEdit::singleline(&mut self.task_start)
                            .id(start_field)
                            .return_key(None)
                            .hint_text("YYYY-MM-DD HH:MM")
                            .desired_width((ui.available_width() - 36.0).max(100.0)),
                    );
                    if crate::date_picker_ui::button(
                        ui,
                        egui::Id::new(("task-start-picker", id)),
                        &mut self.task_start,
                        existing_start,
                        chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
                    ) {
                        self.task_date_error = None;
                        response.request_focus();
                        ui.input_mut(|input| {
                            input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                        });
                    }
                    response
                })
                .inner;
            self.task_input_processed(start_field);
            if start.has_focus() {
                self.focus = owner;
                self.calendar_task_date_target = Some(TaskDateField::Start);
                if start.gained_focus() {
                    start.scroll_to_me(None);
                    ui.ctx().request_repaint();
                }
                enter |= ui
                    .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
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
                let has_date =
                    self.task_date_legacy.is_some() || self.state.task_deadline(id).is_some();
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
                ui.label(RichText::new(error).color(palette.error).size(11.0));
            }
        });
    }

    fn deadlines(&mut self, ui: &mut Ui) {
        let palette = web_theme::colors(ui.ctx());
        let featured = self.state.deadlines.iter().find(|deadline| {
            Some(deadline.id) == self.state.featured && self.state.is_deadline_active(deadline.id)
        });
        let Some(deadline) = featured else {
            ui.label(RichText::new("No starred deadline").color(palette.muted));
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
            ui.label(
                RichText::new(&title)
                    .color(palette.text)
                    .strong()
                    .size(18.0),
            );
            ui.label(RichText::new(&date).color(palette.muted));
            if seconds < 0 {
                ui.label(RichText::new("OVERDUE").color(palette.error));
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
                                .color(palette.text),
                        );
                        ui.label(
                            RichText::new(["DAYS", "HOURS", "MINUTES", "SECONDS"][index])
                                .small()
                                .color(palette.accent),
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
            self.calendar_weather_date = None;
            let date = deadline.at.with_timezone(&Local).date_naive();
            self.selected_date = date;
            self.calendar_panel.reveal(date);
            self.timeline.reveal(date);
        }
    }

    fn calendar(&mut self, ui: &mut Ui, height: f32) {
        let top = ui.cursor().top();
        ui.horizontal(|ui| {
            if ui
                .selectable_label(
                    !self.timeline_view && self.calendar_panel.month_view(),
                    "Calendar",
                )
                .clicked()
            {
                self.show_calendar(ui.ctx(), true);
            }
            if ui
                .selectable_label(
                    !self.timeline_view && !self.calendar_panel.month_view(),
                    "Dates",
                )
                .clicked()
            {
                self.show_calendar(ui.ctx(), false);
            }
            if ui
                .selectable_label(self.timeline_view, "Timeline")
                .clicked()
            {
                if !self.timeline_view {
                    self.toggle_calendar(ui.ctx());
                } else {
                    self.timeline.request_focus();
                }
            }
        });
        let today = Local::now().date_naive();
        let weather_date = self.calendar_weather_date.unwrap_or(today);
        if self.weather.header(ui, weather_date) {
            self.state.preferences.weather = self.weather.config().cloned();
            self.changed();
        }
        if !self.timeline_view {
            self.timeline.cancel_gesture();
            self.calendar_days(ui);
            self.calendar_date_editor(ui);
            return;
        }
        // Give the chart the panel's remaining height as the task list grows.
        // Weather stays above it and never occupies the space below the rows.
        let rows_height = (height - (ui.cursor().top() - top) - 60.0).max(180.0);
        let action = self.timeline.ui(
            ui,
            &self.state,
            today,
            rows_height,
            &self.selected_task_lists,
        );
        if self.timeline.focused {
            self.focus = 2;
        }
        match action {
            Some(TimelineAction::LabelWidth(percent)) => {
                if self.state.preferences.timeline_label_percent != percent {
                    self.state.preferences.timeline_label_percent = percent;
                    self.changed();
                }
            }
            Some(TimelineAction::ResizeTask { task, edge, date }) => {
                if let Err(error) = self.resize_task_dates(task, edge, date) {
                    self.status = error;
                }
            }
            Some(TimelineAction::EditTask(id)) => {
                self.calendar_weather_date = None;
                if self.begin_task_date(id, false) {
                    self.task_date_timeline = true;
                    if let Some(deadline) = self.state.task_deadline(id) {
                        self.selected_date = deadline.at.with_timezone(&Local).date_naive();
                        self.calendar_panel.reveal(self.selected_date);
                    }
                }
            }
            Some(TimelineAction::EditDeadline(id)) => {
                self.begin_legacy_date(id, true);
            }
            None => {}
        }
        self.calendar_date_editor(ui);
    }

    fn calendar_date_editor(&mut self, ui: &mut Ui) {
        if self.task_date_timeline {
            let target = self.task_date_edit.or(self.task_date_legacy);
            if let Some(id) = target {
                ui.add_space(6.0);
                self.task_date_editor(ui, id);
            }
        }
    }

    fn pick_task_date(&mut self, field: TaskDateField, date: NaiveDate) -> Result<(), String> {
        let (draft, existing, hour) = match field {
            TaskDateField::Start => (
                &self.task_start,
                self.state
                    .tasks
                    .iter()
                    .find(|task| Some(task.id) == self.task_date_edit)
                    .and_then(|task| task.start),
                0,
            ),
            TaskDateField::Due => (
                &self.task_date,
                match self.task_date_legacy {
                    Some(id) => self
                        .state
                        .deadlines
                        .iter()
                        .find(|deadline| deadline.id == id),
                    None => self
                        .task_date_edit
                        .and_then(|id| self.state.task_deadline(id)),
                }
                .map(|deadline| deadline.at),
                9,
            ),
        };
        let default_time = chrono::NaiveTime::from_hms_opt(hour, 0, 0).unwrap();
        let current = jig::task_dates::resolve(draft, existing, default_time, &Local)
            .ok()
            .or(existing);
        let chosen = jig::task_dates::resolve(&date.to_string(), current, default_time, &Local)?
            .to_rfc3339();
        match field {
            TaskDateField::Start => self.task_start = chosen,
            TaskDateField::Due => self.task_date = chosen,
        }
        self.task_date_error = None;
        Ok(())
    }

    fn calendar_days(&mut self, ui: &mut Ui) {
        let palette = web_theme::colors(ui.ctx());
        let picking = self
            .calendar_task_date_target
            .filter(|_| self.task_date_edit.is_some() || self.task_date_legacy.is_some());
        if let Some(field) = picking {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(match field {
                        TaskDateField::Start => "Click a day to set the start date",
                        TaskDateField::Due => "Click a day to set the due date",
                    })
                    .small()
                    .color(palette.accent),
                );
                if ui.small_button("Done picking").clicked() {
                    self.calendar_task_date_target = None;
                }
            });
        }
        if let Some(date) = self.calendar_panel.ui(
            ui,
            &self.state,
            Local::now().date_naive(),
            self.selected_date,
            &self.selected_task_lists,
        ) {
            self.selected_date = date;
            self.calendar_weather_date = Some(date);
            if let Some(field) = picking {
                if let Err(error) = self.pick_task_date(field, date) {
                    self.status = error.clone();
                    self.task_date_error = Some(error);
                }
            }
            ui.ctx().request_repaint();
        }
        if self.calendar_panel.focused {
            self.focus = 2;
        }
        self.calendar_tasks(ui);
    }

    fn calendar_tasks(&mut self, ui: &mut Ui) {
        let palette = web_theme::colors(ui.ctx());
        let tasks: Vec<_> = self
            .state
            .tasks_on_date(self.selected_date, &Local)
            .into_iter()
            .filter(|task| self.selected_task_lists.contains(&task.list_id))
            .collect();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(self.selected_date.format("%a %d %b").to_string())
                    .color(palette.muted)
                    .size(12.0),
            );
            if !tasks.is_empty() {
                ui.label(
                    RichText::new(format!("{} due", tasks.len()))
                        .color(palette.muted)
                        .size(12.0),
                );
            }
        });
        if tasks.is_empty() {
            ui.label(
                RichText::new("No tasks due")
                    .color(palette.muted)
                    .size(12.0),
            );
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
                                    .color(palette.muted)
                                    .size(11.0),
                            );
                            let text = RichText::new(task.title)
                                .color(if task.complete {
                                    palette.muted
                                } else {
                                    palette.text
                                })
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
                                Stroke::new(0.5_f32, palette.muted),
                            );
                        }
                    });
                }
            });
    }
}

impl eframe::App for BrowserApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        let schedule = self.task_date_edit.or(self.task_date_legacy);
        let sheet_input = self.sheet_workspace
            && !self.charts_view
            && !self.settings_open
            && self.info_page.is_none()
            && self.sheet.owns_text_input(ctx);
        let sheet_commands = sheet_input
            || (self.sheet_workspace
                && !self.charts_view
                && !self.settings_open
                && self.info_page.is_none()
                && self.sheet.owns_grid_input(ctx));
        let chart_input = self.sheet_workspace
            && self.charts_view
            && !self.settings_open
            && self.info_page.is_none()
            && ctx
                .memory(|memory| memory.focused())
                .is_some_and(|id| TextEdit::load_state(ctx, id).is_some());
        let protect = sheet_input
            || chart_input
            || self.settings_open
            || ctx.memory(|memory| memory.focused()).is_some_and(|id| {
                id == egui::Id::new("task-input")
                    || id == egui::Id::new("task-details-input")
                    || id == egui::Id::new("notes-text-input")
                    || schedule.is_some_and(|target| {
                        id == egui::Id::new(("task-date-input", target))
                            || id == egui::Id::new(("task-start-input", target))
                            || id == egui::Id::new(("task-due-picker", target)).with("time")
                            || id == egui::Id::new(("task-start-picker", target)).with("time")
                    })
            });
        let sheet_navigation = [
            AppAction::ToggleWorkspace,
            AppAction::Tasks,
            AppAction::Notes,
            AppAction::Calendar,
            AppAction::OpenLocal,
        ]
        .map(|action| self.settings.binding(action));
        self.ordered_input
            .process(ctx, input, protect, sheet_commands, &sheet_navigation);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stl.destroy();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        web_theme::apply(ctx, self.settings.theme);
        let palette = web_theme::colors(ctx);
        self.process_settings_events();
        if self.ordered_input.has_pending() {
            ctx.request_repaint();
        }
        let text_input = crate::text_input::has_edit_events(ctx);
        let focused = ctx.memory(|memory| memory.focused());
        let schedule_id = self.task_date_edit.or(self.task_date_legacy);
        let mut inputs = vec![
            egui::Id::new("task-input"),
            egui::Id::new("task-details-input"),
        ];
        if let Some(target) = schedule_id {
            inputs.push(egui::Id::new(("task-date-input", target)));
            inputs.push(egui::Id::new(("task-start-input", target)));
        }
        self.pending_task_input = if text_input {
            focused.filter(|id| inputs.contains(id)).or_else(|| {
                inputs
                    .into_iter()
                    .find(|id| ctx.memory(|memory| memory.had_focus_last_frame(*id)))
            })
        } else {
            None
        };
        self.pending_note_input =
            self.notes.has_draft() && self.notes.owns_keyboard(ctx) && text_input;
        self.pending_chart_input = self.sheet_workspace
            && self.charts_view
            && !self.settings_open
            && self.info_page.is_none()
            && text_input
            && focused.is_some_and(|id| TextEdit::load_state(ctx, id).is_some());
        self.sanitize_task_lists();
        ctx.request_repaint_after(Duration::from_secs(1));
        self.process_document_events();
        self.weather.update();
        if self.stl.update() {
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
        let settings_capture = self.settings_open && self.settings_panel.owns_keyboard();
        crate::settings_storage::set_capture(settings_capture);
        if !self.settings_open && self.info_page.is_none() && !settings_capture {
            self.navigation_shortcuts(ctx);
            if self.sheet_workspace || self.tasks_workspace {
                for (key, action) in [
                    (jig::app_settings::ShortcutKey::S, SheetAction::SaveOds),
                    (jig::app_settings::ShortcutKey::O, SheetAction::OpenOds),
                    (
                        jig::app_settings::ShortcutKey::P,
                        SheetAction::ExportPdf(self.sheet.active_sheet()),
                    ),
                ] {
                    if Self::consume_chord(ctx, jig::app_settings::Chord::ctrl(key))
                        && self.flush_sheet_edit()
                    {
                        self.apply_sheet_action(action);
                    }
                }
            }
            if self.notes_workspace
                && Self::consume_chord(
                    ctx,
                    jig::app_settings::Chord::ctrl(jig::app_settings::ShortcutKey::S),
                )
                && self.flush_notes_edit()
                && self.dirty
            {
                self.save_current();
            }
            if !self.sheet_workspace
                && !self.notes_workspace
                && !self.notes.owns_keyboard(ctx)
                && !ctx.memory(|memory| memory.any_popup_open())
                && self.consume_shortcut(ctx, AppAction::Maximize)
            {
                self.cancel_task_note_drag(ctx);
                self.timeline.cancel_gesture();
                self.expanded = !self.expanded;
                self.focus_after_resize(ctx);
                ctx.request_repaint();
            }
            if !self.sheet_workspace
                && !self.notes_workspace
                && !self.notes.owns_keyboard(ctx)
                && !ctx.memory(|memory| memory.any_popup_open())
                && self.consume_shortcut(ctx, AppAction::RenameTaskList)
            {
                self.cancel_task_note_drag(ctx);
                let focused = ctx.memory(|memory| memory.focused());
                if let Some((_, list)) = self
                    .task_list_tab_ids
                    .iter()
                    .find(|(id, _)| Some(*id) == focused)
                {
                    self.begin_task_list_rename(*list, ctx);
                } else {
                    self.timeline.cancel_gesture();
                    self.expanded = !self.expanded;
                    self.focus_after_resize(ctx);
                }
            }
            if self.sheet_workspace && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                self.confirm = None;
            }
            if !self.sheet_workspace
                && !self.notes_workspace
                && !self.notes.owns_keyboard(ctx)
                && !ctx.memory(|memory| memory.any_popup_open())
                && ctx
                    .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                let list_editor = self.task_list_rename.take().is_some()
                    || self.task_list_remove.take().is_some();
                if !list_editor && !self.timeline.cancel_gesture() {
                    self.expanded = false;
                }
                self.confirm = None;
                self.focus_calculator = false;
                self.focus_task_date = false;
                self.task_color_edit = None;
                self.calendar_task_date_target = None;
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
                let no_popup = !ctx.memory(|memory| memory.any_popup_open());
                if no_popup && self.consume_shortcut(ctx, AppAction::Redo) {
                    self.redo_last(ctx);
                } else if no_popup && self.consume_shortcut(ctx, AppAction::Undo) {
                    self.undo_last(ctx);
                }
                let divider_focused = ctx.memory(|memory| memory.has_focus(task_split_id()))
                    || self.timeline.owns_keyboard(ctx)
                    || self.calendar_panel.owns_keyboard(ctx)
                    || ctx.memory(|memory| memory.any_popup_open())
                    || self.notes.owns_keyboard(ctx);
                let previous = !self.sheet_workspace
                    && !self.notes_workspace
                    && !divider_focused
                    && ctx.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft)
                            || input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                    });
                let next = !self.sheet_workspace
                    && !self.notes_workspace
                    && !divider_focused
                    && ctx.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)
                            || input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
                    });
                if previous || next {
                    self.cancel_task_note_drag(ctx);
                    let order: &[usize] = &[1, 2];
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
        }
        // Static label selection runs after app UI and can overwrite a cell's
        // clipboard output. Sheet owns selection; TextEdit keeps normal copy.
        let selectable_labels =
            !(self.sheet_workspace || self.notes_workspace || self.tasks_workspace);
        if ctx.style().interaction.selectable_labels != selectable_labels {
            ctx.style_mut(|style| style.interaction.selectable_labels = selectable_labels);
        }
        if self.sheet_workspace
            || self.notes_workspace
            || (!self.tasks_workspace && self.expanded && !matches!(self.focus, 1 | 2))
        {
            self.last_task_list_click = None;
        }
        let settings_was_open = self.settings_open;
        let info_was_open = self.info_page.is_some();
        let popup_was_open = ctx.memory(|memory| memory.any_popup_open());
        egui::TopBottomPanel::top("jig-header").show(ctx, |ui| self.toolbar(ui));
        egui::TopBottomPanel::bottom("jig-status").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&self.status).size(11.0).color(
                    if self.storage_error || self.dirty {
                        palette.error
                    } else {
                        palette.muted
                    },
                ));
                ui.hyperlink_to("Source", "https://github.com/biraz-dev/Jig");
                if let Some(error) = &self.draft_backup_error {
                    ui.label(
                        RichText::new(format!("Draft backup: {error}"))
                            .small()
                            .color(palette.error),
                    );
                }
                ui.label(
                    RichText::new(if self.info_page.is_some() {
                        "Esc return to workspace".to_owned()
                    } else if self.settings_open {
                        "Preferences save in this browser · Esc return".to_owned()
                    } else if self.sheet_workspace {
                        format!(
                            "Arrows cells | Enter edit | Ctrl+G go to | {} tasks",
                            self.shortcut_label(AppAction::ToggleWorkspace)
                        )
                    } else if self.notes_workspace {
                        format!(
                            "Ctrl+Enter save note | Esc cancel | {} tasks",
                            self.shortcut_label(AppAction::Tasks)
                        )
                    } else {
                        format!(
                            "Arrows select | {} maximize | Esc return",
                            self.shortcut_label(AppAction::Maximize)
                        )
                    })
                    .size(11.0)
                    .color(palette.muted),
                );
            });
            if !self.settings_open {
                let backup = crate::settings_storage::status();
                if let Some(action) = crate::settings_ui::backup_recovery(
                    ui,
                    &backup,
                    !self.dirty && !self.storage_error,
                    true,
                ) {
                    self.backup_recovery_action(action, ui.ctx());
                }
            }
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(page) = self.info_page {
                match info::show(ui, page, &self.settings, &mut self.focus_info) {
                    Some(info::InfoAction::Done) => self.info_page = None,
                    Some(info::InfoAction::Settings) => self.toggle_settings(ui.ctx()),
                    None => {}
                }
                return;
            }
            if self.settings_open {
                self.settings_workspace(ui);
                return;
            }
            if self.sheet_workspace {
                self.spreadsheet(ui);
                return;
            }
            if self.notes_workspace {
                egui::ScrollArea::vertical()
                    .id_salt("notes-workspace")
                    .show(ui, |ui| {
                        self.notes_board(ui);
                        self.drag_auto_scroll(ui);
                    });
                return;
            }
            let height = ui.available_height().max(500.0);
            egui::ScrollArea::vertical()
                .id_salt("dashboard")
                .show(ui, |ui| {
                    self.tasks_main(ui, height - 8.0);
                    self.drag_auto_scroll(ui);
                });
        });
        // Let Settings consume a newly captured chord before global commands.
        if !self.settings_panel.owns_keyboard() {
            if self.consume_shortcut(ctx, AppAction::Settings) {
                self.toggle_settings(ctx);
            }
            if info_was_open && self.info_page.is_some() {
                if !popup_was_open
                    && !ctx.memory(|memory| memory.any_popup_open())
                    && ctx.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                    })
                {
                    self.info_page = None;
                } else {
                    self.navigation_shortcuts(ctx);
                }
            }
            if settings_was_open && self.settings_open {
                if !popup_was_open
                    && !ctx.memory(|memory| memory.any_popup_open())
                    && ctx.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                    })
                {
                    self.close_settings();
                } else {
                    self.navigation_shortcuts(ctx);
                }
            }
        }
        if self.collect_viewer_undo() {
            ctx.request_repaint();
        }
        if self.pending_task_input.is_none() {
            if let Some((id, combine)) = self.deferred_task_list.take() {
                self.select_task_list(id, combine, ctx);
            }
            if let Some(target) = self.deferred_task_edit.take() {
                self.edit_task_target(target, ctx);
            }
        }
        if let Some(workspace) = self.deferred_view.take() {
            match workspace {
                DeferredView::Sheet => {
                    self.set_workspace(true, ctx);
                }
                DeferredView::Charts(source) => {
                    self.show_charts(ctx, source);
                }
                DeferredView::Calendar => self.toggle_calendar(ctx),
                DeferredView::CalendarMonth(month) => self.show_calendar(ctx, month),
                DeferredView::Calculator => {
                    self.charts.cancel_gesture();
                    self.settings_panel.cancel_capture();
                    crate::settings_storage::set_capture(false);
                    if self.has_pending_editor_input() {
                        self.open_calculator(ctx);
                    } else {
                        ctx.memory_mut(|memory| {
                            memory.open_popup(egui::Id::new("calculator-popup"))
                        });
                        self.focus_calculator = true;
                        ctx.request_repaint();
                    }
                }
                DeferredView::Settings => {
                    if self.has_pending_editor_input() {
                        self.toggle_settings(ctx);
                    } else if self.flush_sheet_edit() {
                        self.charts.cancel_gesture();
                        self.cancel_task_note_drag(ctx);
                        self.timeline.cancel_gesture();
                        ctx.memory_mut(|memory| memory.close_popup());
                        if self.settings_open {
                            self.close_settings();
                        } else {
                            self.info_page = None;
                            self.settings_open = true;
                            self.settings_panel.open();
                        }
                    }
                }
                DeferredView::Info(page) => {
                    if self.has_pending_editor_input() {
                        self.deferred_view = Some(DeferredView::Info(page));
                    } else if self.flush_sheet_edit() {
                        self.charts.cancel_gesture();
                        self.cancel_task_note_drag(ctx);
                        self.timeline.cancel_gesture();
                        self.close_settings();
                        self.info_page = Some(page);
                        self.focus_info = true;
                    }
                }
                DeferredView::Tasks => {
                    self.show_tasks(ctx);
                }
                DeferredView::Notes => {
                    self.show_notes(ctx);
                }
            }
            ctx.request_repaint();
        }
        self.finish_backup_action(ctx);
        self.run_automatic_backup(ctx);
        let task_draft = self.backup_working_drafts();
        browser_storage::set_unsaved(
            self.dirty
                || self.charts.has_draft()
                || self.notes.has_unsaved_changes()
                || task_draft
                || (self.draft_backup_blocked && self.draft_backup_payload.is_some()),
        );
    }
}
