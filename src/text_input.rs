//! Preserve native editor event order when text and navigation share a frame.
use eframe::egui::{Context, Event, Key, PointerButton, RawInput, TextEdit, TouchPhase};
use jig::app_settings::Chord;

pub fn has_edit_events(ctx: &Context) -> bool {
    ctx.input(|input| input.events.iter().any(edits_text))
}

/// A key that opens a control must not type into the editor it just focused.
/// eframe pairs Space with a text event, before or after its key event.
pub fn consume_opening_keys(ctx: &Context) {
    ctx.input_mut(|input| {
        let space = input.events.iter().position(|event| {
            matches!(event, Event::Key { key: Key::Space, pressed: true, modifiers, .. }
                if modifiers.is_none())
        });
        let paired_text = space.and_then(|index| {
            [Some(index + 1), index.checked_sub(1)]
                .into_iter()
                .flatten()
                .find(|&index| matches!(input.events.get(index), Some(Event::Text(text)) if text == " "))
        });
        let mut index = 0;
        input.events.retain(|event| {
            let keep = paired_text != Some(index)
                && !matches!(event, Event::Key { key: Key::Enter | Key::Space, pressed: true, modifiers, .. }
                    if modifiers.is_none());
            index += 1;
            keep
        });
    });
}

fn edits_text(event: &Event) -> bool {
    match event {
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
    }
}

fn commits_sheet_edit(event: &Event, navigation: &[Option<Chord>]) -> bool {
    let Event::Key {
        key,
        pressed: true,
        modifiers,
        ..
    } = event
    else {
        return false;
    };
    (modifiers.ctrl
        && !modifiers.shift
        && !modifiers.alt
        && !modifiers.mac_cmd
        && matches!(key, Key::G | Key::S | Key::O | Key::P))
        || (modifiers.alt
            && !modifiers.ctrl
            && !modifiers.shift
            && !modifiers.mac_cmd
            && matches!(key, Key::PageUp | Key::PageDown))
        || (modifiers.shift
            && !modifiers.ctrl
            && !modifiers.alt
            && !modifiers.mac_cmd
            && *key == Key::F11)
        || navigation.iter().flatten().any(|chord| {
            Key::from_name(chord.key.name()) == Some(*key)
                && modifiers.ctrl == chord.ctrl
                && modifiers.shift == chord.shift
                && modifiers.alt == chord.alt
                && modifiers.mac_cmd == chord.meta
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
    /// Sheet commands also wait for earlier edits before committing the draft.
    /// Prime an idle pointer before a press whose location changed without a
    /// prior paint, so that jump cannot scroll a parent panel.
    pub fn process(
        &mut self,
        ctx: &Context,
        input: &mut RawInput,
        protect_editor: bool,
        protect_sheet_commands: bool,
        sheet_navigation: &[Option<Chord>],
    ) {
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
        let mut sheet_edit_pending = false;
        let mut press_origin = None;
        for (index, event) in input.events.iter().enumerate() {
            if sheet_edit_pending && commits_sheet_edit(event, sheet_navigation) {
                // Sheet navigation and file commands commit before TextEdit
                // runs. Let earlier text reach the current draft first.
                self.deferred = input.events.split_off(index);
                ctx.request_repaint();
                break;
            }
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
            sheet_edit_pending |= protect_sheet_commands && edits_text(event);
            finish_before_press |= sheet_edit_pending;
        }
    }
}
