//! Note draft cancellation and explicit saving through the actual egui editor.
extern crate self as eframe;
pub use egui;
#[allow(dead_code)]
#[path = "../src/notes_ui.rs"]
mod notes_ui;
#[allow(dead_code)]
#[path = "../src/web_theme.rs"]
mod web_theme;

use egui::{Context, Event, Id, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, TextEdit};
use jig::model::State;
use notes_ui::{NoteAction, NoteDraftSnapshot, NotesPanel};

struct Harness {
    ctx: Context,
    panel: NotesPanel,
    state: State,
    time: f64,
    other: String,
    escape_owned: bool,
    early_cancel: bool,
    window_focused: bool,
    labels: std::collections::HashMap<String, Pos2>,
}

impl Harness {
    fn new(existing: bool, text: &str) -> Self {
        let mut state = State::default();
        let id = existing.then(|| state.add_note("Original note").unwrap());
        let mut panel = NotesPanel::new();
        panel
            .restore_draft(
                NoteDraftSnapshot {
                    id,
                    text: text.into(),
                    original: if existing {
                        "Original note".into()
                    } else {
                        String::new()
                    },
                },
                &state,
            )
            .unwrap();
        let mut h = Self {
            ctx: Context::default(),
            panel,
            state,
            time: 0.0,
            other: String::new(),
            escape_owned: false,
            early_cancel: false,
            window_focused: true,
            labels: Default::default(),
        };
        h.frame(vec![]);
        h.frame(vec![]);
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(editor_id()));
        h
    }

    fn frame(&mut self, events: Vec<Event>) -> Vec<NoteAction> {
        self.time += 0.1;
        let mut actions = Vec::new();
        let output = self.ctx.run(
            RawInput {
                focused: self.window_focused,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 800.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ctx| {
                if self.early_cancel {
                    self.panel.cancel_if_clicked_outside(ctx);
                }
                // The application checks this before rendering Notes. egui has
                // already processed Escape's ordinary focus cancellation.
                if ctx.input(|input| input.key_pressed(Key::Escape)) {
                    self.escape_owned = self.panel.owns_keyboard(ctx);
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add(TextEdit::singleline(&mut self.other).id(other_id()));
                    actions = self.panel.ui(ui, &self.state, self.state.default_list_id());
                });
            },
        );
        self.labels.clear();
        for shape in output.shapes {
            if let egui::Shape::Text(text) = shape.shape {
                self.labels.insert(
                    text.galley.job.text.clone(),
                    text.pos + text.galley.size() * 0.5,
                );
            }
        }
        actions
    }

    fn press(&mut self, key: Key) -> Vec<NoteAction> {
        self.frame(
            [true, false]
                .into_iter()
                .map(|pressed| Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                })
                .collect(),
        )
    }

    fn click(&mut self, position: Pos2) -> Vec<NoteAction> {
        let mut actions = self.frame(vec![
            Event::PointerMoved(position),
            Event::PointerButton {
                pos: position,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ]);
        actions.extend(self.frame(vec![Event::PointerButton {
            pos: position,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]));
        actions
    }

    fn input_rect(&self) -> Rect {
        self.ctx.read_response(editor_id()).unwrap().rect
    }
}

fn editor_id() -> Id {
    Id::new("notes-text-input")
}

fn other_id() -> Id {
    Id::new("unrelated-editor")
}

#[test]
fn escape_cancels_settled_new_and_existing_note_edits_without_saving() {
    for (existing, text) in [
        (false, ""),
        (false, "Unfinished note"),
        (true, ""),
        (true, "Unfinished note"),
    ] {
        let mut h = Harness::new(existing, text);
        let original = h.state.clone();
        assert_eq!(h.panel.has_unsaved_changes(), existing || !text.is_empty());
        assert!(h.press(Key::Escape).is_empty());
        assert!(h.escape_owned, "The parent must let Notes handle Escape");
        assert!(!h.panel.has_draft(), "Escape must close the editor");
        assert!(!h.panel.has_unsaved_changes());
        assert_eq!(
            serde_json::to_value(&h.state).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
    }
}

#[test]
fn escape_cancels_from_either_note_editor_button() {
    for tabs in [1, 2] {
        let mut h = Harness::new(false, "Unfinished note");
        for _ in 0..tabs {
            assert!(h.press(Key::Tab).is_empty());
        }
        let focused = h.ctx.memory(|m| m.focused()).unwrap();
        assert_ne!(focused, editor_id());
        assert_ne!(focused, other_id());
        assert!(h.press(Key::Escape).is_empty());
        assert!(h.escape_owned, "Editor buttons must retain Notes ownership");
        assert!(!h.panel.has_draft(), "Escape must cancel from a button");
        assert!(h.state.notes.is_empty());
    }
}

#[test]
fn escape_in_an_unrelated_editor_preserves_the_note_draft() {
    let mut h = Harness::new(true, "Unfinished note");
    h.ctx.memory_mut(|m| m.request_focus(other_id()));
    assert!(h.frame(vec![]).is_empty());
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(other_id()));
    assert!(h.press(Key::Escape).is_empty());
    assert!(
        !h.escape_owned,
        "Notes must not capture another editor's Escape"
    );
    assert!(h.panel.has_unsaved_changes());
    assert_eq!(h.panel.draft_snapshot().unwrap().text, "Unfinished note");
    assert_eq!(h.state.notes[0].text, "Original note");
}

#[test]
fn clicking_outside_cancels_new_existing_and_invalid_drafts_without_actions() {
    for early_cancel in [false, true] {
        for (existing, text) in [
            (false, "Unsent new note"),
            (true, "Changed note"),
            (true, ""),
        ] {
            let mut h = Harness::new(existing, text);
            h.early_cancel = early_cancel;
            let original = serde_json::to_value(&h.state).unwrap();
            let outside = h.ctx.read_response(other_id()).unwrap().rect.center();
            assert!(
                h.click(outside).is_empty(),
                "Cancelling must not emit a save"
            );
            assert!(!h.panel.has_draft());
            assert!(h.panel.draft_snapshot().is_none());
            assert_eq!(serde_json::to_value(&h.state).unwrap(), original);
        }
    }
}

#[test]
fn clicks_inside_the_card_and_keyboard_focus_changes_preserve_the_draft() {
    let mut h = Harness::new(true, "Working text");
    h.early_cancel = true;
    let input = h.input_rect();
    for point in [
        input.center(),
        egui::pos2(input.left() - 3.0, input.top() + 3.0),
    ] {
        assert!(h.click(point).is_empty());
        assert_eq!(h.panel.draft_snapshot().unwrap().text, "Working text");
    }
    assert!(h.press(Key::Tab).is_empty());
    assert!(h.panel.has_draft(), "Tab to a card button must not cancel");
    h.window_focused = false;
    assert!(h.frame(vec![]).is_empty());
    assert!(h.panel.has_draft(), "Window blur must retain recovery text");
}

#[test]
fn explicit_save_button_and_queued_save_keep_the_last_input() {
    let mut button = Harness::new(true, "Working text");
    button.early_cancel = true;
    let save = button.labels["Save"];
    assert!(
        matches!(button.click(save).as_slice(), [NoteAction::Edit { text, .. }] if text == "Working text")
    );
    assert!(!button.panel.has_draft());

    let mut queued = Harness::new(true, "Working text");
    queued.press(Key::End);
    queued.panel.request_save();
    let actions = queued.frame(vec![Event::Text("!".into())]);
    assert!(
        matches!(actions.as_slice(), [NoteAction::Edit { text, .. }] if text == "Working text!")
    );
    assert!(!queued.panel.has_draft());
}

#[test]
fn switching_to_another_note_or_add_cancels_instead_of_saving() {
    for existing_target in [false, true] {
        let mut h = Harness::new(true, "Discard this draft");
        let second = h.state.add_note("Second note").unwrap();
        h.frame(vec![]);
        h.frame(vec![]);
        let target = h.labels[if existing_target {
            "Second note"
        } else {
            "Add note"
        }];
        assert!(h.click(target).is_empty());
        h.frame(vec![]);
        h.frame(vec![]);
        assert!(h.panel.has_draft());
        assert_eq!(h.ctx.memory(|memory| memory.focused()), Some(editor_id()));
        assert!(h.frame(vec![Event::Text("Fresh input".into())]).is_empty());
        let draft = h.panel.draft_snapshot().unwrap();
        assert_eq!(draft.id, existing_target.then_some(second));
        assert_eq!(
            draft.original,
            if existing_target { "Second note" } else { "" }
        );
        assert!(!draft.text.contains("Discard this draft"));
        assert_eq!(h.state.notes[0].text, "Original note");
        assert_eq!(h.state.notes[1].text, "Second note");
    }
}

#[test]
fn touch_tap_outside_cancels_but_dragging_out_does_not() {
    let outside = egui::pos2(800.0, 650.0);
    for phase in [egui::TouchPhase::End, egui::TouchPhase::Cancel] {
        let mut tap = Harness::new(true, "Working text");
        let touch = |phase| Event::Touch {
            device_id: egui::TouchDeviceId(1),
            id: egui::TouchId(1),
            phase,
            pos: outside,
            force: None,
        };
        tap.frame(vec![
            touch(egui::TouchPhase::Start),
            Event::PointerMoved(outside),
            Event::PointerButton {
                pos: outside,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ]);
        assert!(tap
            .frame(vec![
                touch(phase),
                Event::PointerButton {
                    pos: outside,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                },
                Event::PointerGone,
            ])
            .is_empty());
        assert_eq!(tap.panel.has_draft(), phase == egui::TouchPhase::Cancel);
    }

    let mut drag = Harness::new(true, "Working text");
    let inside = drag.input_rect().center();
    drag.frame(vec![
        Event::PointerMoved(inside),
        Event::PointerButton {
            pos: inside,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    ]);
    drag.frame(vec![Event::PointerMoved(outside)]);
    assert!(drag
        .frame(vec![Event::PointerButton {
            pos: outside,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }])
        .is_empty());
    assert!(
        drag.panel.has_draft(),
        "A selection drag must not discard text"
    );
}
