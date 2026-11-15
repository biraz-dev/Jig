//! Preserve native editor event order when typing and clicking share a frame.
use eframe::egui::{Context, Event, Key, RawInput, TextEdit, TouchPhase};

pub fn has_edit_events(ctx: &Context) -> bool {
    ctx.input(|input| {
        input.events.iter().any(|event| match event {
            Event::Text(_) | Event::Paste(_) | Event::Cut | Event::Ime(_) => true,
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                matches!(key, Key::Backspace | Key::Delete)
                    || (*key == Key::Enter && !modifiers.ctrl && !modifiers.command)
                    || (matches!(key, Key::Z | Key::Y) && (modifiers.ctrl || modifiers.command))
            }
            _ => false,
        })
    })
}

#[derive(Default)]
pub struct OrderedInput {
    deferred: Vec<Event>,
}

impl OrderedInput {
    pub fn has_pending(&self) -> bool {
        !self.deferred.is_empty()
    }

    /// Called before egui begins its pass. Finish earlier editor input or a
    /// cancelled touch before a later pointer press starts another interaction.
    /// The untouched pointer tail is delivered on the next requested paint.
    pub fn process(&mut self, ctx: &Context, input: &mut RawInput, protect_editor: bool) {
        if !self.deferred.is_empty() {
            let mut events = std::mem::take(&mut self.deferred);
            events.append(&mut input.events);
            input.events = events;
        }
        let editor_focused = protect_editor
            && ctx
                .memory(|memory| memory.focused())
                .is_some_and(|id| TextEdit::load_state(ctx, id).is_some());
        let mut finish_before_press = false;
        for (index, event) in input.events.iter().enumerate() {
            if finish_before_press && matches!(event, Event::PointerButton { pressed: true, .. }) {
                self.deferred = input.events.split_off(index);
                ctx.request_repaint();
                break;
            }
            // eframe does not repaint or release its pointer on TouchCancel.
            // The next Start may arrive in the same batch; let applets cancel
            // their old native drag before that new press reaches egui.
            finish_before_press |= matches!(
                event,
                Event::Touch {
                    phase: TouchPhase::Cancel,
                    ..
                }
            );
            finish_before_press |= editor_focused
                && matches!(
                    event,
                    Event::Text(_)
                        | Event::Paste(_)
                        | Event::Cut
                        | Event::Copy
                        | Event::Ime(_)
                        | Event::Key { pressed: true, .. }
                );
        }
    }
}
