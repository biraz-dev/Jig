//! One formatted holding document, using Jig's existing save and Undo paths.
use super::*;
use crate::draft_storage::{self, Event};

impl BrowserApp {
    pub(super) fn sync_draft(&mut self, force: bool) {
        if let Err(error) = draft_storage::sync(
            &self.state.draft,
            force,
            !force && !self.dirty && !self.storage_error,
        ) {
            self.status = error;
        }
    }

    pub(super) fn process_draft_events(&mut self, ctx: &egui::Context) {
        let events = match draft_storage::take() {
            Ok(events) => events,
            Err(error) => {
                draft_storage::show_error(&error);
                self.status = error;
                return;
            }
        };
        for event in events {
            match event {
                Event::Change { draft, coalesce } => {
                    if let Err(error) = draft.validate() {
                        self.status = error;
                        draft_storage::show_error(&self.status);
                        self.draft_undo_group = false;
                        break;
                    }
                    if draft == self.state.draft {
                        self.sync_draft(false);
                        continue;
                    }
                    self.state.draft = draft;
                    if coalesce && self.draft_undo_group {
                        // Continuous typing is one history action, but each
                        // accepted snapshot still saves immediately.
                        self.redo_history.clear();
                        self.undo_baseline = self.state.clone();
                        self.undo_baseline_json =
                            serde_json::to_vec(&self.state).unwrap_or_default();
                        self.save_current();
                    } else {
                        self.changed();
                    }
                    self.draft_undo_group = true;
                }
                Event::Undo => {
                    self.undo_last(ctx);
                    self.sync_draft(false);
                }
                Event::Redo => {
                    self.redo_last(ctx);
                    self.sync_draft(false);
                }
                Event::Note { text } => self.copy_draft_text(&text, false),
                Event::Task { text } => self.copy_draft_text(&text, true),
            }
        }
    }

    fn copy_draft_text(&mut self, text: &str, task: bool) {
        let mut state = self.state.clone();
        let result = if task {
            jig::model::note_text(text).and_then(|body| {
                let title = body
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("");
                let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
                let id = state.add_task_to_list(self.active_task_list, &title)?;
                state
                    .tasks
                    .iter_mut()
                    .find(|task| task.id == id)
                    .unwrap()
                    .details = Some(body);
                Ok(())
            })
        } else {
            state.add_note(text).map(|_| ())
        };
        match result {
            Ok(()) => {
                self.state = state;
                self.changed();
                if !self.dirty && !self.storage_error {
                    self.status = if task {
                        "Copied to the active task list; Draft is unchanged."
                    } else {
                        "Copied to Notes; Draft is unchanged."
                    }
                    .into();
                }
            }
            Err(error) => {
                draft_storage::show_error(&error);
                self.status = error;
            }
        }
    }

    pub(super) fn show_draft(&mut self, ctx: &egui::Context) -> bool {
        if self.has_pending_editor_input() {
            self.deferred_view = Some(DeferredView::Draft);
            return false;
        }
        if !self.flush_sheet_edit() {
            return false;
        }
        self.cancel_notes_edit();
        self.charts.cancel_gesture();
        self.cancel_task_note_drag(ctx);
        self.timeline.cancel_gesture();
        self.calendar_task_date_target = None;
        self.close_settings();
        self.info_page = None;
        self.sheet_workspace = false;
        self.tasks_workspace = false;
        self.notes_workspace = false;
        self.draft_workspace = true;
        self.draft_undo_group = false;
        self.expanded = false;
        self.task_split_draft = None;
        self.focus_calculator = false;
        self.focus_task_date = false;
        browser_storage::set_workspace("draft");
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        draft_storage::focus();
        ctx.request_repaint();
        true
    }
}
