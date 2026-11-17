//! History replacement must cancel pointer gestures before they can write cells.
extern crate self as eframe;
pub use egui;
#[allow(dead_code)]
#[path = "../src/chart_ui.rs"]
mod chart_ui;
#[allow(dead_code)]
#[path = "../src/sheet_controls_ui.rs"]
mod sheet_controls_ui;
#[allow(dead_code)]
#[path = "../src/text_input.rs"]
mod text_input;
#[allow(dead_code)]
#[path = "../src/web_theme.rs"]
mod web_theme;

use chart_ui::{ChartAction, ChartPanel};
use egui::{Context, Event, Id, Key, Modifiers, PointerButton, Pos2, RawInput, Rect};
use jig::{
    model::State,
    sheet_controls::{ControlKind, ControlTarget},
    spreadsheet::{CellAddr, Workbook},
};

struct Harness {
    ctx: Context,
    panel: ChartPanel,
    state: State,
    time: f64,
}

impl Harness {
    fn new() -> Self {
        let mut workbook = Workbook::default();
        workbook.set_cell(1, CellAddr::new(0, 0), "50").unwrap();
        workbook
            .add_control(
                "Scale",
                Some(ControlTarget {
                    sheet: 1,
                    cell: CellAddr::new(0, 0),
                }),
                ControlKind::Slider {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                },
            )
            .unwrap();
        Self {
            ctx: Context::default(),
            panel: ChartPanel::new(),
            state: State {
                workbook: Some(workbook),
                ..State::default()
            },
            time: 0.0,
        }
    }

    fn frame(&mut self, events: Vec<Event>, mut restore: bool) -> Vec<ChartAction> {
        self.time += 0.1;
        let mut actions = Vec::new();
        let _ = self.ctx.run(
            RawInput {
                focused: true,
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(620.0, 850.0))),
                events,
                ..Default::default()
            },
            |ctx| {
                if restore {
                    // Match the application: history replaces data and resets
                    // the panel after egui has received this frame's events.
                    self.state
                        .workbook
                        .as_mut()
                        .unwrap()
                        .set_cell(1, CellAddr::new(0, 0), "10")
                        .unwrap();
                    ctx.stop_dragging();
                    self.panel.cancel_gesture();
                    self.panel.reset();
                    restore = false;
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    actions.extend(self.panel.ui(ui, &self.state));
                });
            },
        );
        actions
    }

    fn slider(&mut self) -> Rect {
        self.frame(vec![], false);
        self.ctx.memory_mut(|memory| {
            memory.request_focus(Id::new(("control-number-value", 1_u64)));
        });
        self.frame(vec![], false);
        // Follow the actual Number field, Apply button, Slider focus order.
        for _ in 0..2 {
            self.frame(
                [true, false]
                    .into_iter()
                    .map(|pressed| Event::Key {
                        key: Key::Tab,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: Modifiers::NONE,
                    })
                    .collect(),
                false,
            );
        }
        let focused = self.ctx.memory(|memory| memory.focused()).unwrap();
        let rect = self.ctx.read_response(focused).unwrap().rect;
        assert!(rect.width() > 80.0 && rect.height() < 40.0);
        rect
    }

    fn pointer(pos: Pos2, pressed: bool) -> Vec<Event> {
        vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            },
        ]
    }

    fn fresh_drag_commits_once(&mut self, rect: Rect) {
        let end = rect.right_center() - egui::vec2(8.0, 0.0);
        assert!(self
            .frame(Self::pointer(rect.center(), true), false)
            .is_empty());
        assert!(self.frame(vec![Event::PointerMoved(end)], false).is_empty());
        let actions = self.frame(Self::pointer(end, false), false);
        assert!(
            matches!(
                actions.as_slice(),
                [ChartAction::ControlValue { id: 1, value }] if *value > 50.0
            ),
            "Fresh drag should commit once: {actions:?}"
        );
        assert!(self.frame(vec![], false).is_empty());
    }
}

fn reset_drag(release_with_reset: bool) {
    // A thumb press can own a drag before changing the value. Exercise both
    // that case and an existing changed preview before history replacement.
    for moved in [false, true] {
        let mut h = Harness::new();
        let rect = h.slider();
        assert!(h
            .frame(Harness::pointer(rect.center(), true), false)
            .is_empty());
        let end = if moved {
            rect.right_center() - egui::vec2(8.0, 0.0)
        } else {
            rect.center()
        };
        assert!(h.frame(vec![Event::PointerMoved(end)], false).is_empty());
        assert!(h.ctx.dragged_id().is_some());
        let events = if release_with_reset {
            Harness::pointer(end, false)
        } else {
            vec![Event::PointerMoved(end)]
        };
        let actions = h.frame(events, true);
        assert!(actions.is_empty(), "Reset must not write: {actions:?}");
        if !release_with_reset {
            assert!(h
                .frame(vec![Event::PointerMoved(rect.center())], false)
                .is_empty());
            assert!(h
                .frame(Harness::pointer(rect.center(), false), false)
                .is_empty());
        }
        assert_eq!(
            h.state
                .workbook
                .as_ref()
                .unwrap()
                .raw(1, CellAddr::new(0, 0)),
            "10"
        );
        h.fresh_drag_commits_once(rect);
    }
}

#[test]
fn history_reset_while_held_cancels_the_slider_until_release() {
    reset_drag(false);
}

#[test]
fn history_reset_in_the_pointer_release_frame_cannot_write_a_stale_value() {
    reset_drag(true);
}
