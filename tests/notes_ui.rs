//! Escape regressions for the actual Notes editor and its parent keyboard guard.
extern crate self as eframe;
pub use egui;
#[allow(dead_code)]
#[path = "../src/notes_ui.rs"]
mod notes_ui;
#[allow(dead_code)]
#[path = "../src/web_theme.rs"]
mod web_theme;

use egui::{Context, Event, Id, Key, Modifiers, Pos2, RawInput, Rect, TextEdit};
use jig::model::State;
use notes_ui::{NoteAction, NoteDraftSnapshot, NotesPanel};

struct Harness {
    ctx: Context,
    panel: NotesPanel,
    state: State,
    time: f64,
    other: String,
    escape_owned: bool,
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
        };
        h.frame(vec![]);
        h.frame(vec![]);
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(editor_id()));
        h
    }

    fn frame(&mut self, events: Vec<Event>) -> Vec<NoteAction> {
        self.time += 0.1;
        let mut actions = Vec::new();
        let _ = self.ctx.run(
            RawInput {
                focused: true,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 800.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ctx| {
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
