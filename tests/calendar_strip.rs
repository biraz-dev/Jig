use chrono::{Datelike, NaiveDate};
use jig::calendar_strip::{Viewport, MAX_DAY_WIDTH, MIN_DAY_WIDTH};

extern crate self as eframe;
pub use egui;
#[allow(dead_code)]
#[path = "../src/calendar_ui.rs"]
mod calendar_ui;
#[allow(dead_code)]
#[path = "../src/web_theme.rs"]
mod web_theme;

// The empty calendar fixture has no task marks; only the color adapter is needed.
mod web {
    pub fn task_color(_: Option<jig::model::TaskColor>, _: bool) -> egui::Color32 {
        egui::Color32::WHITE
    }
}

fn day(year: i32, month: u32, date: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, date).unwrap()
}

#[test]
fn widening_exposes_more_dates_across_month_and_year_boundaries() {
    let view = Viewport::new(day(2026, 12, 30));
    let narrow = view.visible(280.0);
    let wide = view.visible(560.0);
    assert_eq!(narrow[0].0, day(2026, 12, 27));
    assert_eq!(narrow[0], wide[0]);
    assert!(narrow.iter().any(|(date, _)| *date == day(2027, 1, 1)));
    assert!(wide.last().unwrap().0 > narrow.last().unwrap().0);
    assert_eq!(view.date_at(200.0, 280.0), Some(day(2027, 1, 1)));
    assert_eq!(view.date_at(280.0, 280.0), None);
}

#[test]
fn dragging_pans_across_leap_days_and_zoom_keeps_the_pointer_date() {
    let mut view = Viewport::new(day(2024, 3, 1));
    let before = view.date_at(120.0, 480.0).unwrap();
    assert_eq!(before, day(2024, 3, 1));
    view.pan(40.0, 480.0);
    assert_eq!(view.date_at(120.0, 480.0), Some(day(2024, 2, 29)));
    view.pan(-40.0, 480.0);
    assert_eq!(view.date_at(120.0, 480.0), Some(before));

    let anchor_date = view.date_at(173.0, 480.0).unwrap();
    let fraction = (173.0 - view.x_for(anchor_date)) / view.day_width();
    view.zoom(1.6, 173.0, 480.0);
    assert_eq!(view.date_at(173.0, 480.0), Some(anchor_date));
    let after_fraction = (173.0 - view.x_for(anchor_date)) / view.day_width();
    assert!((fraction - after_fraction).abs() < 0.00001);
    assert_eq!(view.day_width(), 64.0);
    view.zoom(f32::MAX, 173.0, 480.0);
    assert_eq!(view.day_width(), MAX_DAY_WIDTH);
    view.zoom(f32::MIN_POSITIVE, 173.0, 480.0);
    assert_eq!(view.day_width(), MIN_DAY_WIDTH);
}

#[test]
fn reveal_and_pan_remain_within_portable_dates_and_reject_invalid_motion() {
    let mut view = Viewport::new(day(1, 1, 1));
    view.pan(f32::MAX, 320.0);
    assert_eq!(view.visible(320.0)[0].0, day(1, 1, 1));
    view.pan(-f32::MAX, 320.0);
    assert_eq!(view.visible(320.0).last().unwrap().0, day(9999, 12, 31));
    assert!(view
        .visible(320.0)
        .iter()
        .all(|(date, _)| date.year() == 9999));
    view.reveal(day(2026, 11, 15), 320.0);
    assert_eq!(view.date_at(160.0, 320.0), Some(day(2026, 11, 15)));
    view.ensure_visible(day(2026, 12, 5), 320.0);
    assert!(view
        .visible(320.0)
        .iter()
        .any(|(date, _)| *date == day(2026, 12, 5)));
    let before = view;
    view.pan(f32::NAN, 320.0);
    view.zoom(f32::INFINITY, 0.0, 320.0);
    view.zoom(0.0, 0.0, 320.0);
    assert_eq!(view, before);
    assert!(view.visible(f32::INFINITY).is_empty());
    assert!(view.visible(-1.0).is_empty());
}

#[test]
fn dates_keep_keyboard_navigation_and_zoom_inside_a_scrolling_pane() {
    use egui::{epaint::Shape, Context, Event, Key, Modifiers, Pos2, RawInput, Rect};

    fn selected_width(shape: &Shape, color: egui::Color32) -> Option<f32> {
        match shape {
            Shape::Rect(rect) if rect.fill == color && rect.rect.height() > 50.0 => {
                Some(rect.rect.width())
            }
            Shape::Vec(shapes) => shapes.iter().find_map(|shape| selected_width(shape, color)),
            _ => None,
        }
    }

    for changing_weather in [false, true] {
        let ctx = Context::default();
        web_theme::apply(&ctx, &jig::app_settings::AppSettings::default(), None);
        let state = jig::model::State::default();
        let today = day(2026, 10, 30);
        let mut selected = today;
        let mut calendar = calendar_ui::CalendarPanel::new(today);
        calendar.request_focus();
        let mut normal_width = 0.0;
        for (frame, key) in [
            None,
            None,
            Some(Key::Home),
            Some(Key::ArrowRight),
            Some(Key::Home),
            Some(Key::Equals),
            Some(Key::Minus),
        ]
        .into_iter()
        .enumerate()
        {
            let events = key.map_or_else(Vec::new, |key| {
                [true, false]
                    .into_iter()
                    .map(|pressed| Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: Modifiers::NONE,
                    })
                    .collect()
            });
            let output = ctx.run(
                RawInput {
                    focused: true,
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1440.0, 1000.0))),
                    time: Some(frame as f64 * 0.1),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("tasks-upper-area")
                            .drag_to_scroll(false)
                            .max_height(560.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_width(760.0);
                                ui.horizontal(|ui| {
                                    ui.label("Calendar");
                                    ui.label("Dates");
                                    ui.label("Timeline");
                                });
                                ui.label("Sample Harbor · °C · options");
                                if changing_weather && selected != today {
                                    ui.label("High 22°C · Low 14°C");
                                    ui.label("Rain 60% · Humidity 75%");
                                } else {
                                    ui.label("High 20°C · Low 12°C");
                                }
                                if let Some(date) = calendar.ui(ui, &state, today, selected, &[1]) {
                                    selected = date;
                                }
                                ui.add_space(400.0);
                            });
                    });
                },
            );
            assert!(calendar.owns_keyboard(&ctx), "Focus lost after {key:?}");
            let width = output
                .shapes
                .iter()
                .find_map(|shape| selected_width(&shape.shape, web_theme::colors(&ctx).selected))
                .expect("The selected date must remain visible");
            match key {
                Some(Key::ArrowRight) => assert_eq!(selected, today + chrono::Duration::days(1)),
                Some(Key::Home) => {
                    assert_eq!(selected, today);
                    normal_width = width;
                }
                Some(Key::Equals) => assert!(width > normal_width + 5.0),
                Some(Key::Minus) => assert!((width - normal_width).abs() < 2.0),
                _ => {}
            }
        }
    }
}
