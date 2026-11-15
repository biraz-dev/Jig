use chrono::{Datelike, NaiveDate};
use jig::calendar_strip::{Viewport, MAX_DAY_WIDTH, MIN_DAY_WIDTH};

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
