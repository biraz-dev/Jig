//! Preserve native editor event order when typing and clicking share a frame.
use eframe::egui::{Context, Event, Key, RawInput, TextEdit};

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

    /// Called before egui begins its pass. A focused editor consumes its
    /// earlier keyboard events before a later pointer press can blur it.
    /// The untouched pointer tail is delivered on the next requested paint.
    pub fn process(&mut self, ctx: &Context, input: &mut RawInput, protect_editor: bool) {
        if !self.deferred.is_empty() {
            let mut events = std::mem::take(&mut self.deferred);
            events.append(&mut input.events);
            input.events = events;
        }
        if !protect_editor
            || !ctx
                .memory(|memory| memory.focused())
                .is_some_and(|id| TextEdit::load_state(ctx, id).is_some())
        {
            return;
        }
        let mut editor_event = false;
        for (index, event) in input.events.iter().enumerate() {
            if editor_event && matches!(event, Event::PointerButton { pressed: true, .. }) {
                self.deferred = input.events.split_off(index);
                ctx.request_repaint();
                break;
            }
            editor_event |= matches!(
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
