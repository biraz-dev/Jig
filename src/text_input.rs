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
    /// Prime an idle pointer before a press whose location changed without a
    /// prior paint, so that jump cannot scroll a parent panel.
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
        let single_pointer = input
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
            <= 1;
        let mut finish_before_press = false;
        let mut press_origin = None;
        for (index, event) in input.events.iter().enumerate() {
            if let Event::PointerMoved(pos) = event {
                if press_origin.is_some_and(|origin| origin != *pos) {
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
                // eframe runs pointerdown logic before touchstart identifies its
                // device. Normalize idle primary presses, including mouse/pen;
                // moving within this same pass cannot clear egui's old delta.
                let idle_press = *button == PointerButton::Primary && single_pointer;
                let prime_press =
                    idle_press && idle_pointer.is_some_and(|previous| previous != pos);
                if finish_before_press || prime_press {
                    self.deferred = input.events.split_off(index);
                    if prime_press {
                        input.events.push(Event::PointerMoved(pos));
                    }
                    ctx.request_repaint();
                    break;
                }
                if resuming && idle_press && idle_pointer == Some(pos) {
                    press_origin = Some(pos);
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
                press_origin = None;
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
