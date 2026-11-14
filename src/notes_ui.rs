//! A small board of plain-text notes. The application owns every saved record.
use crate::web_theme::{ACCENT, BG, MUTED, RULE, SURFACE, TEXT};
use eframe::egui::{self, RichText, Sense, Stroke, TextEdit, Ui};
use jig::model::{note_text, Note, State, MAX_NOTES, MAX_NOTE_BYTES};
use serde::{Deserialize, Serialize};

const CARD_WIDTH: f32 = 150.0;
const CARD_HEIGHT: f32 = 156.0;
const GAP: f32 = 8.0;

/// Recovery may retain a working draft larger than the saved-note limit, so a
/// failed Save can still be corrected or copied after a refresh.
pub const MAX_NOTE_DRAFT_BYTES: usize = 128 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NoteDraftSnapshot {
    pub id: Option<u64>,
    pub text: String,
    pub original: String,
}

#[derive(Clone, Debug)]
pub enum NoteAction {
    Add { text: String },
    Edit { id: u64, text: String },
    Remove(u64),
    Pin { id: u64, pinned: bool },
    Move { id: u64, target: u64, after: bool },
    ToTask { id: u64, list_id: u64 },
}

struct Draft {
    id: Option<u64>,
    text: String,
    original: String,
    focus: bool,
}

struct Drag {
    id: u64,
    pinned: bool,
    order: Vec<(u64, bool)>,
    columns: usize,
}

#[derive(Default)]
pub struct NotesPanel {
    draft: Option<Draft>,
    pending_edit: Option<(Option<u64>, String)>,
    convert: Option<(u64, u64)>,
    drag: Option<Drag>,
    reveal: Option<u64>,
    error: Option<String>,
    keyboard_ids: Vec<egui::Id>,
    pub focused: bool,
}

impl NotesPanel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_draft(&self) -> bool {
        self.draft.is_some()
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.draft.as_ref().is_some_and(|draft| match draft.id {
            Some(_) => draft.text != draft.original,
            None => !draft.text.trim().is_empty(),
        })
    }

    /// A per-tab recovery record, separate from committed application state.
    pub fn draft_snapshot(&self) -> Option<NoteDraftSnapshot> {
        if !self.has_unsaved_changes() {
            return None;
        }
        let draft = self.draft.as_ref()?;
        Some(NoteDraftSnapshot {
            id: draft.id,
            text: draft.text.clone(),
            original: draft.original.clone(),
        })
    }

    /// Restore an editor without changing State or its Undo journal. Returns
    /// true when a missing/changed original requires recovery as a new note.
    pub fn restore_draft(
        &mut self,
        snapshot: NoteDraftSnapshot,
        state: &State,
    ) -> jig::Result<bool> {
        if snapshot.text.len() > MAX_NOTE_DRAFT_BYTES {
            return Err("The recovered note draft exceeds 128 KiB".into());
        }
        if snapshot
            .text
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
            || snapshot
                .text
                .as_bytes()
                .iter()
                .enumerate()
                .any(|(index, byte)| {
                    *byte == b'\r' && snapshot.text.as_bytes().get(index + 1) != Some(&b'\n')
                })
        {
            return Err("The recovered note draft contains unsupported control characters".into());
        }
        if snapshot.original.len() > MAX_NOTE_BYTES {
            return Err("The recovered note's original text exceeds 16 KiB".into());
        }
        if !snapshot.original.is_empty() {
            note_text(&snapshot.original)?;
        }
        let matched = snapshot.id.filter(|id| {
            state
                .notes
                .iter()
                .any(|note| note.id == *id && note.text == snapshot.original)
        });
        let recovered_as_new = snapshot.id.is_some() && matched.is_none();
        self.reset();
        self.draft = Some(Draft {
            id: matched,
            text: snapshot.text,
            original: if matched.is_some() {
                snapshot.original
            } else {
                String::new()
            },
            focus: true,
        });
        Ok(recovered_as_new)
    }

    /// Finish a valid edit as one application action. An invalid draft stays
    /// available for correction; an unchanged edit closes without an action.
    pub fn finish_edit(&mut self) -> Option<NoteAction> {
        if self
            .draft
            .as_ref()
            .is_some_and(|draft| draft.id.is_none() && draft.text.trim().is_empty())
        {
            self.cancel_edit();
            return None;
        }
        let draft = self.draft.as_ref()?;
        let text = match note_text(&draft.text) {
            Ok(text) => text,
            Err(error) => {
                self.error = Some(error);
                return None;
            }
        };
        let draft = self.draft.take()?;
        self.error = None;
        match draft.id {
            Some(id) if draft.original != text => Some(NoteAction::Edit { id, text }),
            Some(_) => None,
            None => Some(NoteAction::Add { text }),
        }
    }

    pub fn cancel_edit(&mut self) {
        self.draft = None;
        self.error = None;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Preserve text if the application rejects a command after the draft was
    /// finished, for example when the note limit changed in another action.
    pub fn reject_action(&mut self, action: &NoteAction, error: String) {
        match action {
            NoteAction::Add { text } => {
                self.draft = Some(Draft {
                    id: None,
                    text: text.clone(),
                    original: String::new(),
                    focus: true,
                });
            }
            NoteAction::Edit { id, text } => {
                self.draft = Some(Draft {
                    id: Some(*id),
                    text: text.clone(),
                    original: String::new(),
                    focus: true,
                });
            }
            _ => {}
        }
        self.error = Some(error);
    }

    pub fn owns_keyboard(&self, ctx: &egui::Context) -> bool {
        ctx.memory(|memory| {
            memory
                .focused()
                .is_some_and(|id| id == draft_input_id() || self.keyboard_ids.contains(&id))
        })
    }

    fn begin_edit(&mut self, id: Option<u64>, text: &str) {
        if self.draft.as_ref().is_some_and(|draft| draft.id == id) {
            return;
        }
        // A header or earlier card can be clicked in the same input frame as
        // the last text/paste event for a later card. Let its editor consume
        // that input before finishing the old draft or replacing its target.
        self.pending_edit = Some((id, text.to_owned()));
    }

    fn finish_pending_edit(&mut self, state: &State, actions: &mut Vec<NoteAction>) {
        let Some((id, text)) = self.pending_edit.take() else {
            return;
        };
        if id.is_some_and(|id| {
            !state.notes.iter().any(|note| note.id == id)
                || actions.iter().any(|action| {
                    matches!(action, NoteAction::Remove(removed) | NoteAction::ToTask { id: removed, .. } if *removed == id)
                })
        }) {
            return;
        }
        if let Some(action) = self.finish_edit() {
            actions.push(action);
        }
        if self.has_draft() {
            return;
        }
        self.convert = None;
        self.drag = None;
        self.error = None;
        self.draft = Some(Draft {
            id,
            text: text.clone(),
            original: text,
            focus: true,
        });
    }

    /// The parent supplies scrolling, so the same board can appear in Notes
    /// and below the Tasks workspace without creating another set of records.
    pub fn ui(&mut self, ui: &mut Ui, state: &State, active_list: u64) -> Vec<NoteAction> {
        self.focused = false;
        self.keyboard_ids.clear();
        let reveal = self.reveal.take();
        let mut actions = Vec::new();
        let columns =
            (((ui.available_width() + GAP) / (CARD_WIDTH + GAP)).floor() as usize).clamp(1, 9);
        let width =
            ((ui.available_width() - GAP * (columns - 1) as f32) / columns as f32).max(80.0);
        let mut notes: Vec<_> = state.notes.iter().collect();
        notes.sort_by_key(|note| !note.pinned);
        let order: Vec<_> = notes.iter().map(|note| (note.id, note.pinned)).collect();
        if self.drag.as_ref().is_some_and(|drag| {
            drag.order != order || drag.columns != columns || ui.input(|input| !input.focused)
        }) || ui.input(|input| {
            input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Touch {
                        phase: egui::TouchPhase::Cancel,
                        ..
                    }
                )
            })
        }) {
            self.drag = None;
        }
        if self.draft.as_ref().is_some_and(|draft| {
            draft
                .id
                .is_some_and(|id| !state.notes.iter().any(|note| note.id == id))
        }) {
            self.cancel_edit();
        }
        if self
            .convert
            .is_some_and(|(id, _)| !state.notes.iter().any(|note| note.id == id))
        {
            self.convert = None;
        }

        ui.horizontal_wrapped(|ui| {
            let add = ui.add_enabled(state.notes.len() < MAX_NOTES, egui::Button::new("Add note"));
            self.focused |= add.has_focus();
            if add
                .on_hover_text("Plain text · Up to 81 notes · Ctrl-Enter saves an edit")
                .clicked()
            {
                self.begin_edit(None, "");
            }
            ui.label(
                RichText::new(format!("{}/{}", state.notes.len(), MAX_NOTES))
                    .small()
                    .color(MUTED),
            );
        });
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).small().color(crate::web_theme::ERROR));
        }
        let mut cards = Vec::new();
        egui::Grid::new("notes-board")
            .num_columns(columns)
            .spacing([GAP, GAP])
            .show(ui, |ui| {
                let mut count = 0;
                for note in &notes {
                    let rect = ui
                        .push_id(("note", note.id), |ui| {
                            self.card(
                                ui,
                                state,
                                note,
                                width,
                                columns,
                                active_list,
                                &order,
                                &mut actions,
                            )
                        })
                        .inner;
                    cards.push((note.id, note.pinned, rect));
                    count += 1;
                    if count % columns == 0 {
                        ui.end_row();
                    }
                }
                if self.draft.as_ref().is_some_and(|draft| draft.id.is_none()) {
                    ui.push_id("new-note", |ui| {
                        self.editor_card(ui, width, &mut actions);
                    });
                }
            });

        if let Some(id) = reveal {
            if let Some((_, _, rect)) = cards.iter().find(|(note_id, _, _)| *note_id == id) {
                ui.scroll_to_rect(*rect, Some(egui::Align::Center));
            }
        }

        if let Some(drag) = &self.drag {
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                self.drag = None;
            } else {
                let target = ui
                    .input(|input| input.pointer.interact_pos())
                    .and_then(|pointer| {
                        cards
                            .iter()
                            .find(|(id, pinned, rect)| {
                                *id != drag.id && *pinned == drag.pinned && rect.contains(pointer)
                            })
                            .map(|(id, _, rect)| (*id, pointer.x >= rect.center().x, *rect))
                    });
                if let Some((_, _, rect)) = target {
                    ui.painter().rect_stroke(
                        rect,
                        3.0,
                        Stroke::new(1.5_f32, ACCENT),
                        egui::StrokeKind::Inside,
                    );
                }
                if ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary)) {
                    if let Some((target, after, _)) = target {
                        self.reveal = Some(drag.id);
                        actions.push(NoteAction::Move {
                            id: drag.id,
                            target,
                            after,
                        });
                    }
                    self.drag = None;
                } else if !ui.input(|input| input.pointer.primary_down()) {
                    self.drag = None;
                }
            }
        }
        if self.pending_edit.is_some() {
            self.finish_pending_edit(state, &mut actions);
            ui.ctx().request_repaint();
        }
        actions
    }

    #[allow(clippy::too_many_arguments)]
    fn card(
        &mut self,
        ui: &mut Ui,
        state: &State,
        note: &Note,
        width: f32,
        columns: usize,
        active_list: u64,
        order: &[(u64, bool)],
        actions: &mut Vec<NoteAction>,
    ) -> egui::Rect {
        let editing = self
            .draft
            .as_ref()
            .is_some_and(|draft| draft.id == Some(note.id));
        let converting = self.convert.is_some_and(|(id, _)| id == note.id);
        let height = if editing || converting {
            228.0
        } else {
            CARD_HEIGHT
        };
        let response = egui::Frame::default()
            .fill(if note.pinned { egui::Color32::from_rgb(48, 42, 36) } else { SURFACE })
            .stroke(Stroke::new(1.0_f32, if editing { ACCENT } else { RULE.gamma_multiply(0.6) }))
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.set_width((width - 12.0).max(68.0));
                ui.set_min_height(height - 12.0);
                let mut remove = false;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    let grip_id = ui.id().with("grip");
                    self.keyboard_ids.push(grip_id);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 20.0), Sense::hover());
                    let grip = ui.interact(rect, grip_id, if self.has_draft() { Sense::hover() } else { Sense::click_and_drag() })
                        .on_hover_cursor(if self.has_draft() { egui::CursorIcon::Default } else if self.drag.as_ref().is_some_and(|drag| drag.id == note.id) { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
                    let grip_color = if grip.has_focus() || grip.hovered() { ACCENT } else { MUTED };
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "↕", egui::FontId::monospace(14.0), grip_color);
                    if grip.has_focus() {
                        ui.painter().rect_stroke(rect.shrink(1.0), 1.0, Stroke::new(1.0_f32, ACCENT), egui::StrokeKind::Inside);
                    }
                    grip.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, !self.has_draft(), "Reorder note"));
                    self.focused |= grip.has_focus();
                    if grip.gained_focus() { grip.scroll_to_me(None); }
                    if grip.clicked() || grip.drag_started() { grip.request_focus(); }
                    if grip.drag_started() {
                        self.drag = Some(Drag { id: note.id, pinned: note.pinned, order: order.to_vec(), columns });
                    }
                    if grip.has_focus() && self.drag.is_none() && !self.has_draft() {
                        ui.memory_mut(|memory| memory.set_focus_lock_filter(grip_id, egui::EventFilter {
                            horizontal_arrows: true, vertical_arrows: true, ..Default::default()
                        }));
                        let delta = ui.input_mut(|input| {
                            for (key, delta) in [
                                (egui::Key::ArrowLeft, -1), (egui::Key::ArrowRight, 1),
                                (egui::Key::ArrowUp, -(columns as isize)),
                                (egui::Key::ArrowDown, columns as isize),
                            ] {
                                if input.consume_key(egui::Modifiers::ALT, key) { return delta; }
                            }
                            0
                        });
                        if delta != 0 {
                            let group: Vec<_> = order.iter().filter(|(_, pinned)| *pinned == note.pinned).map(|(id, _)| *id).collect();
                            if let Some(index) = group.iter().position(|id| *id == note.id) {
                                let next = (index as isize + delta).clamp(0, group.len() as isize - 1) as usize;
                                if next != index {
                                    self.reveal = Some(note.id);
                                    actions.push(NoteAction::Move { id: note.id, target: group[next], after: delta > 0 });
                                }
                            }
                        }
                    }
                    grip.on_hover_text("Drag to reorder · Alt+arrows on the focused handle · Pinned notes stay first");
                    let idle = !self.has_draft();
                    let pin = ui.add_enabled(idle, egui::Button::new(if note.pinned { "◆" } else { "◇" }).small());
                    pin.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, idle, if note.pinned { "Unpin note" } else { "Pin note" }));
                    if pin.on_hover_text(if note.pinned { "Unpin note" } else { "Pin note" }).clicked() {
                        self.reveal = Some(note.id);
                        actions.push(NoteAction::Pin { id: note.id, pinned: !note.pinned });
                    }
                    if ui.small_button("copy").on_hover_text("Copy the complete plain text").clicked() {
                        let text = self.draft.as_ref().filter(|draft| draft.id == Some(note.id)).map_or(note.text.as_str(), |draft| draft.text.as_str());
                        ui.ctx().copy_text(text.to_owned());
                    }
                    if ui.add_enabled(idle, egui::Button::new("task").small()).on_hover_text("Move this note into a task list").clicked() {
                        let list = note.task_origin.as_ref().map(|origin| origin.list_id)
                            .filter(|id| state.task_list(*id).is_some())
                            .unwrap_or(active_list);
                        self.convert = Some((note.id, if state.task_list(list).is_some() { list } else { state.default_list_id() }));
                        ui.ctx().request_repaint();
                    }
                    remove = ui.small_button("del").on_hover_text("Delete this note · Undo restores it").clicked();
                });
                if editing {
                    self.editor(ui, actions);
                } else {
                    let (body_rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 100.0), Sense::hover());
                    let painter = ui.painter().with_clip_rect(body_rect);
                    let galley = painter.layout(note.text.clone(), egui::FontId::monospace(12.0), TEXT, body_rect.width());
                    painter.galley(body_rect.min, galley, TEXT);
                    // Keep even a very long note inside a fixed card. Opening
                    // the editor exposes all text without enlarging its preview.
                    let body = ui.interact(body_rect, ui.id().with("body"), Sense::click());
                    body.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), format!("Edit note: {}", note.text)));
                    self.focused |= body.has_focus();
                    if body.gained_focus() { body.scroll_to_me(None); }
                    if body.has_focus() {
                        ui.painter().rect_stroke(body_rect, 1.0, Stroke::new(1.0_f32, ACCENT), egui::StrokeKind::Inside);
                    }
                    if body.clicked() { self.begin_edit(Some(note.id), &note.text); ui.ctx().request_repaint(); }
                }
                if converting { self.conversion(ui, state, note.id, actions); }
                if remove {
                    if editing { self.cancel_edit(); }
                    if converting { self.convert = None; }
                    actions.push(NoteAction::Remove(note.id));
                }
            });
        response.response.rect
    }

    fn editor_card(&mut self, ui: &mut Ui, width: f32, actions: &mut Vec<NoteAction>) {
        egui::Frame::default()
            .fill(BG)
            .stroke(Stroke::new(1.0_f32, ACCENT))
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.set_width((width - 12.0).max(68.0));
                ui.set_min_height(216.0);
                self.editor(ui, actions);
            });
    }

    fn editor(&mut self, ui: &mut Ui, actions: &mut Vec<NoteAction>) {
        if self.pending_edit.is_some() {
            // The clicked switch target may have taken keyboard focus before
            // this card was rendered. Retain the old input long enough to
            // consume this frame's final text/paste events before committing.
            ui.memory_mut(|memory| memory.request_focus(draft_input_id()));
        }
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        let focused = draft.focus || ui.memory(|memory| memory.has_focus(draft_input_id()));
        let save_key = focused
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Enter));
        let cancel_key = focused
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let response = egui::ScrollArea::vertical()
            .id_salt("note-editor-scroll")
            .max_height(150.0)
            .show(ui, |ui| {
                ui.add(
                    TextEdit::multiline(&mut draft.text)
                        .id(draft_input_id())
                        .hint_text("Write a note…")
                        .desired_width(ui.available_width())
                        .desired_rows(7)
                        .font(egui::TextStyle::Monospace),
                )
            })
            .inner;
        self.focused |= response.has_focus();
        if draft.focus {
            response.request_focus();
            response.scroll_to_me(Some(egui::Align::Center));
            draft.focus = false;
            ui.ctx().request_repaint();
        }
        let mut save = save_key;
        let mut cancel = cancel_key;
        ui.horizontal_wrapped(|ui| {
            save |= ui
                .small_button("Save")
                .on_hover_text("Ctrl-Enter")
                .clicked();
            cancel |= ui.small_button("Cancel").on_hover_text("Escape").clicked();
        });
        if cancel {
            self.cancel_edit();
            ui.ctx().request_repaint();
        } else if save {
            if let Some(action) = self.finish_edit() {
                actions.push(action);
            }
            ui.ctx().request_repaint();
        }
    }

    fn conversion(&mut self, ui: &mut Ui, state: &State, id: u64, actions: &mut Vec<NoteAction>) {
        let Some((_, mut list_id)) = self.convert else {
            return;
        };
        if state.task_list(list_id).is_none() {
            list_id = state.default_list_id();
        }
        ui.horizontal_wrapped(|ui| {
            for list in &state.task_lists {
                if ui
                    .selectable_label(list_id == list.id, &list.name)
                    .clicked()
                {
                    list_id = list.id;
                    ui.ctx().request_repaint();
                }
            }
        });
        self.convert = Some((id, list_id));
        ui.horizontal_wrapped(|ui| {
            if ui
                .small_button("Convert")
                .on_hover_text("Move the entire note into this task list; Undo restores the note")
                .clicked()
            {
                actions.push(NoteAction::ToTask { id, list_id });
                self.convert = None;
            }
            if ui.small_button("Cancel").clicked() {
                self.convert = None;
                ui.ctx().request_repaint();
            }
        });
    }
}

fn draft_input_id() -> egui::Id {
    egui::Id::new("notes-text-input")
}
