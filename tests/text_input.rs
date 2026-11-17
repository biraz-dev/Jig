//! Native widget regressions for the production input adapter, without a window.
extern crate self as eframe;
pub use egui;
#[path = "../src/text_input.rs"]
mod text_input;

use egui::{Context, Event, Id, Key, Modifiers, RawInput, Rect, TextEdit};

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        focused: true,
        screen_rect: Some(Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(480.0, 240.0),
        )),
        events,
        ..Default::default()
    }
}

fn key(key: Key, modifiers: Modifiers) -> Vec<Event> {
    [true, false]
        .into_iter()
        .map(|pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        })
        .collect()
}

#[test]
fn queued_text_and_paste_reach_the_editor_before_forward_or_reverse_tab() {
    for reverse in [false, true] {
        for text in [Event::Text("12".into()), Event::Paste("12".into())] {
            let ctx = Context::default();
            let mut ordered = text_input::OrderedInput::default();
            let mut draft = String::new();
            let mut applied = None;
            let mut saw_edit = false;
            let field = Id::new("number-input");
            let mut frame = |events| {
                let mut raw = input(events);
                ordered.process(&ctx, &mut raw, true, false, &[]);
                let _ = ctx.run(raw, |ctx| {
                    saw_edit |= text_input::has_edit_events(ctx);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let before = ui.button("Apply before").clicked();
                        ui.add(TextEdit::singleline(&mut draft).id(field).return_key(None));
                        if ui.button("Apply after").clicked() || before {
                            applied = Some(draft.clone());
                        }
                    });
                });
            };
            frame(vec![]);
            ctx.memory_mut(|memory| memory.request_focus(field));
            frame(vec![]);
            let mut events = vec![text];
            events.extend(key(
                Key::Tab,
                Modifiers {
                    shift: reverse,
                    ..Modifiers::NONE
                },
            ));
            frame(events);
            frame(vec![]); // Process the deferred focus change.
            frame(key(Key::Space, Modifiers::NONE));
            assert_eq!(draft, "12", "reverse Tab: {reverse}");
            assert_eq!(applied.as_deref(), Some("12"));
            assert!(saw_edit);
            assert!(!ordered.has_pending());
        }
    }
}

fn open_editor(
    ctx: &Context,
    events: Vec<Event>,
    open: &mut bool,
    text: &mut String,
    focus_button: bool,
) {
    let _ = ctx.run(input(events), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let button = ui.button("Open date");
            if focus_button {
                button.request_focus();
            }
            if button.clicked() {
                *open = true;
                text_input::consume_opening_keys(ctx);
                ui.memory_mut(|memory| memory.request_focus(Id::new("date-input")));
            }
            if *open {
                ui.add(
                    TextEdit::singleline(text)
                        .id(Id::new("date-input"))
                        .return_key(None),
                );
            }
        });
    });
}

#[test]
fn space_opens_the_real_button_without_typing_into_its_new_editor() {
    for text_first in [false, true] {
        let ctx = Context::default();
        let mut open = false;
        let mut text = String::new();
        open_editor(&ctx, vec![], &mut open, &mut text, true);
        let mut activation = key(Key::Space, Modifiers::NONE);
        activation.insert(usize::from(!text_first), Event::Text(" ".into()));
        open_editor(&ctx, activation, &mut open, &mut text, false);
        assert!(open);
        assert_eq!(text, "");

        open_editor(
            &ctx,
            vec![Event::Text("2030-01-17".into())],
            &mut open,
            &mut text,
            false,
        );
        let mut ordinary = key(Key::Space, Modifiers::NONE);
        ordinary.insert(1, Event::Text(" ".into()));
        ordinary.push(Event::Text("09:30".into()));
        open_editor(&ctx, ordinary, &mut open, &mut text, false);
        assert_eq!(
            text, "2030-01-17 09:30",
            "Later date separators remain ordinary input"
        );
    }
}

#[test]
fn opening_guard_removes_one_paired_space_without_discarding_other_edits() {
    let ctx = Context::default();
    let mut events = vec![Event::Text(" ".into())];
    events.extend(key(Key::Space, Modifiers::NONE).into_iter().take(1));
    events.extend([
        Event::Text(" ".into()),
        Event::Text("09:30".into()),
        Event::Paste("pasted".into()),
        Event::Ime(egui::ImeEvent::Commit("composed".into())),
    ]);
    let _ = ctx.run(input(events), |ctx| {
        text_input::consume_opening_keys(ctx);
        ctx.input(|input| {
            assert_eq!(input.events.len(), 4);
            assert!(matches!(&input.events[0], Event::Text(text) if text == " "));
            assert!(matches!(&input.events[1], Event::Text(text) if text == "09:30"));
            assert!(matches!(&input.events[2], Event::Paste(text) if text == "pasted"));
            assert!(matches!(&input.events[3], Event::Ime(egui::ImeEvent::Commit(text)) if text == "composed"));
        });
    });
}
