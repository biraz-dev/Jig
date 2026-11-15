//! Preserve native editor event order when typing and clicking share a frame.
use eframe::egui::{Context, Event, Key, PointerButton, RawInput, TextEdit, TouchPhase};

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
    /// Also prime an idle mouse position before a first touch so its location
    /// jump cannot scroll a parent panel. The original tail runs next paint.
    pub fn process(&mut self, ctx: &Context, input: &mut RawInput, protect_editor: bool) {
        let resuming = !self.deferred.is_empty();
        if resuming {
            let mut events = std::mem::take(&mut self.deferred);
            events.append(&mut input.events);
            input.events = events;
        }
        let editor_focused = protect_editor
            && ctx
                .memory(|memory| memory.focused())
                .is_some_and(|id| TextEdit::load_state(ctx, id).is_some());
        let mut idle_pointer = ctx.input(|input| {
            (!input.pointer.any_down() && !input.any_touches())
                .then(|| input.pointer.latest_pos())
                .flatten()
        });
        let single_touch = input
            .events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    Event::Touch {
                        phase: TouchPhase::Start,
                        ..
                    }
                )
            })
            .take(2)
            .count()
            == 1;
        let mut finish_before_press = false;
        let mut touch_press_origin = None;
        for (index, event) in input.events.iter().enumerate() {
            if let Event::PointerMoved(pos) = event {
                if touch_press_origin.is_some_and(|origin| origin != *pos) {
                    // A resumed touch must hit-test its grip before later motion
                    // can move the pointer over a different widget.
                    self.deferred = input.events.split_off(index);
                    ctx.request_repaint();
                    break;
                }
            }
            if let Event::PointerButton {
                pos,
                button,
                pressed: true,
                ..
            } = event
            {
                let pos = *pos;
                // eframe emits this down immediately before its Touch::Start,
                // without a preceding move. Moving in the same pass is too late
                // to clear egui's delta from the previous idle mouse location.
                let touch_press = *button == PointerButton::Primary
                    && single_touch
                    && matches!(input.events.get(index + 1), Some(Event::Touch { phase: TouchPhase::Start, pos: touch, .. }) if *touch == pos);
                let prime_touch =
                    touch_press && idle_pointer.is_some_and(|previous| previous != pos);
                if finish_before_press || prime_touch {
                    self.deferred = input.events.split_off(index);
                    if prime_touch {
                        input.events.push(Event::PointerMoved(pos));
                    }
                    ctx.request_repaint();
                    break;
                }
                if resuming && touch_press && idle_pointer == Some(pos) {
                    touch_press_origin = Some(pos);
                }
                idle_pointer = None;
            }
            if matches!(
                event,
                Event::PointerButton { pressed: false, .. }
                    | Event::Touch {
                        phase: TouchPhase::End | TouchPhase::Cancel,
                        ..
                    }
            ) {
                touch_press_origin = None;
            }
            if matches!(
                event,
                Event::Touch {
                    phase: TouchPhase::Start | TouchPhase::Move,
                    ..
                }
            ) {
                idle_pointer = None;
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
