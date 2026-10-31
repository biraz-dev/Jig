use chrono::{FixedOffset, NaiveDate, Utc};
use jig::{
    model::{parse_deadline, State, TaskColor},
    timeline::{self, DateRange, Scale, TimelineRow, VisibleSpan},
};

fn day(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

fn json(state: &State) -> String {
    serde_json::to_string(state).unwrap()
}

#[test]
fn schedule_edits_are_atomic_and_reuse_the_existing_records() {
    let mut state = State::default();
    let task = state.add_task("Prepare installation").unwrap();
    let unchanged = json(&state);
    assert!(state
        .set_task_schedule(task, Some("2026-11-01T00:00:00-04:00"), None)
        .is_err());
    assert_eq!(json(&state), unchanged);
    state
        .set_task_schedule(
            task,
            Some("2026-11-01T00:00:00-04:00"),
            Some("2026-11-08T18:00:00-05:00"),
        )
        .unwrap();
    let deadline = state.task_deadline(task).unwrap().id;
    assert_eq!(state.featured, None, "Scheduling must not choose a star");
    state.set_task_color(task, Some(TaskColor::Sage)).unwrap();
    state.feature_task(task).unwrap();
    state.edit_task(task, "Pack installation").unwrap();
    state
        .set_task_schedule(
            task,
            Some("2026-11-02T00:00:00-05:00"),
            Some("2026-11-09T18:00:00-05:00"),
        )
        .unwrap();
    assert_eq!(state.task_deadline(task).unwrap().id, deadline);
    assert_eq!(state.task_deadline(task).unwrap().title, "Pack installation");
    assert_eq!(state.tasks.len(), 1);
    assert_eq!(state.deadlines.len(), 1);
    assert_eq!(state.featured, Some(deadline));
    let unchanged = json(&state);
    for (start, due) in [
        (Some("not a date"), Some("2026-11-10T18:00:00Z")),
        (Some("2026-11-03T00:00:00Z"), Some("not a date")),
        (
            Some("2026-11-10T18:00:00Z"),
            Some("2026-11-09T18:00:00Z"),
        ),
        (Some("2026-11-03T00:00:00Z"), None),
    ] {
        assert!(state.set_task_schedule(task, start, due).is_err());
        assert_eq!(
            json(&state),
            unchanged,
            "A rejected edit must change nothing"
        );
    }
    assert!(state.set_task_schedule(9999, None, None).is_err());
    assert_eq!(json(&state), unchanged);
    state.validate().unwrap();
}

#[test]
fn existing_due_editing_paths_preserve_and_validate_starts() {
    let mut state = State::default();
    let task = state.add_task("Dry work").unwrap();
    state
        .set_task_deadline(task, Some("2026-11-08T18:00:00Z"))
        .unwrap();
    state
        .set_task_start(task, Some("2026-11-03T00:00:00Z"))
        .unwrap();
    let deadline = state.task_deadline(task).unwrap().id;
    let unchanged = json(&state);
    assert!(state
        .set_task_deadline(task, Some("2026-11-02T18:00:00Z"))
        .is_err());
    assert_eq!(json(&state), unchanged);
    assert!(state
        .set_deadline(Some(deadline), "Bad rename", "2026-11-02T18:00:00Z")
        .is_err());
    assert_eq!(
        json(&state),
        unchanged,
        "A rejected date must not rename a task"
    );
    assert!(state
        .set_task_start(task, Some("2026-11-09T00:00:00Z"))
        .is_err());
    assert_eq!(json(&state), unchanged);
    state
        .set_deadline(Some(deadline), "Dry sculpture", "2026-11-10T18:00:00Z")
        .unwrap();
    assert_eq!(state.tasks[0].text, "Dry sculpture");
    assert_eq!(
        state.tasks[0].start,
        Some(parse_deadline("2026-11-03T00:00:00Z").unwrap())
    );
    state.set_task_start(task, None).unwrap();
    assert_eq!(state.tasks[0].start, None);
    assert_eq!(state.task_deadline(task).unwrap().id, deadline);
    state
        .set_task_start(task, Some("2026-11-10T18:00:00Z"))
        .unwrap();
    assert_eq!(state.tasks[0].start, Some(state.deadlines[0].at));
}

#[test]
fn removing_dates_clears_their_start_and_completion_retains_it() {
    let mut state = State::default();
    let task = state.add_task("Fire test tiles").unwrap();
    state
        .set_task_schedule(
            task,
            Some("2026-11-01T00:00:00Z"),
            Some("2026-11-02T18:00:00Z"),
        )
        .unwrap();
    state.set_task_color(task, Some(TaskColor::Rose)).unwrap();
    state.feature_task(task).unwrap();
    state.toggle_task(task).unwrap();
    assert!(state.tasks[0].start.is_some());
    assert_eq!(state.tasks[0].color, Some(TaskColor::Rose));
    assert_eq!(state.featured, None);
    state.toggle_task(task).unwrap();
    state.feature_task(task).unwrap();
    let deadline = state.task_deadline(task).unwrap().id;
    state.remove_deadline(deadline).unwrap();
    assert_eq!(state.tasks[0].start, None);
    assert_eq!(state.tasks[0].deadline, None);
    assert_eq!(state.featured, None);
    state.validate().unwrap();
    state
        .set_task_schedule(
            task,
            Some("2026-11-03T00:00:00Z"),
            Some("2026-11-04T18:00:00Z"),
        )
        .unwrap();
    state.set_task_deadline(task, None).unwrap();
    assert_eq!(state.tasks[0].start, None);
    assert_eq!(state.tasks[0].deadline, None);
    assert_eq!(state.tasks[0].color, Some(TaskColor::Rose));
    state.validate().unwrap();
}

#[test]
fn starts_roundtrip_in_version_one_json_and_old_files_still_load() {
    let mut state = State::default();
    let task = state.add_task("Photograph work").unwrap();
    state
        .set_task_deadline(task, Some("2026-11-08T18:00:00-05:00"))
        .unwrap();
    let old = json(&state);
    assert!(!old.contains("\"start\""));
    let restored = State::from_json(&old).unwrap();
    assert_eq!(json(&restored), old);
    state
        .set_task_start(task, Some("2026-11-05T00:00:00-05:00"))
        .unwrap();
    state.set_task_color(task, Some(TaskColor::Lilac)).unwrap();
    state.feature_task(task).unwrap();
    let exported = json(&state);
    let restored = State::from_json(&exported).unwrap();
    assert_eq!(json(&restored), exported);
    assert_eq!(restored.version, 1);
    assert_eq!(restored.tasks.len(), 1);
    assert_eq!(restored.deadlines.len(), 1);
    let mut invalid: serde_json::Value = serde_json::from_str(&exported).unwrap();
    invalid["tasks"][0]["start"] = "2026-11-09T00:00:00Z".into();
    assert!(State::from_json(&invalid.to_string()).is_err());
    invalid["tasks"][0]["start"] = "2026-11-05T00:00:00Z".into();
    invalid["tasks"][0].as_object_mut().unwrap().remove("deadline");
    assert!(State::from_json(&invalid.to_string()).is_err());
    invalid["tasks"][0]["start"] = "not a date".into();
    assert!(State::from_json(&invalid.to_string()).is_err());
}

#[test]
fn rows_are_a_read_only_projection_of_tasks_and_legacy_deadlines() {
    let mut state = State::default();
    let first = state.add_task("Make mold").unwrap();
    let undated = state.add_task("Choose glaze").unwrap();
    let third = state.add_task("Cast work").unwrap();
    for task in [first, third] {
        state
            .set_task_deadline(task, Some("2026-11-08T18:00:00Z"))
            .unwrap();
    }
    state.set_task_color(first, Some(TaskColor::Amber)).unwrap();
    state.set_task_color(third, Some(TaskColor::Sage)).unwrap();
    state
        .set_task_start(first, Some("2026-11-01T00:00:00Z"))
        .unwrap();
    state.toggle_task(third).unwrap();
    let legacy = state
        .set_deadline(None, "Existing opening", "2026-11-08T18:00:00Z")
        .unwrap();
    state.feature(legacy).unwrap();
    let unchanged = json(&state);
    let rows = timeline::rows(&state, &Utc);
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].task_id, Some(first));
    assert_eq!(rows[1].task_id, Some(undated));
    assert_eq!(rows[1].due, None);
    assert_eq!(rows[2].task_id, Some(third));
    assert!(rows[2].complete);
    assert_eq!(rows[2].color, Some(TaskColor::Sage));
    assert_eq!(rows[3].task_id, None);
    assert_eq!(rows[3].deadline_id, Some(legacy));
    assert_eq!(rows[3].title, "Existing opening");
    assert_eq!(
        rows.iter()
            .filter(|row| row.due == Some(day("2026-11-08")))
            .count(),
        3,
        "Rows on one date must remain separate"
    );
    assert!(state.tasks_on_date(day("2026-11-03"), &Utc).is_empty());
    assert_eq!(json(&state), unchanged);
}

#[test]
fn projection_uses_calendar_days_in_the_requested_timezone_across_offset_changes() {
    let mut state = State::default();
    let task = state.add_task("Prepare kiln").unwrap();
    state
        .set_task_schedule(
            task,
            Some("2026-11-01T00:00:00-04:00"),
            Some("2026-11-01T01:30:00-05:00"),
        )
        .unwrap();
    let eastern_standard = FixedOffset::west_opt(5 * 3600).unwrap();
    let rows = timeline::rows(&state, &eastern_standard);
    assert_eq!(rows[0].start, Some(day("2026-10-31")));
    assert_eq!(rows[0].due, Some(day("2026-11-01")));
    let range = timeline::window(day("2026-11-01"), Scale::Week).unwrap();
    let span = timeline::clipped(&rows[0], range).unwrap();
    assert_eq!(
        span.days, 2,
        "The timeline counts local dates, not 24-hour periods"
    );
    let utc_rows = timeline::rows(&state, &Utc);
    assert_eq!(utc_rows[0].start, Some(day("2026-11-01")));
    assert_eq!(utc_rows[0].due, Some(day("2026-11-01")));
    assert_eq!(timeline::clipped(&utc_rows[0], range).unwrap().days, 1);
}

fn row(start: Option<&str>, due: Option<&str>) -> TimelineRow<'static> {
    TimelineRow {
        task_id: Some(1),
        deadline_id: due.map(|_| 2),
        title: "Test task",
        complete: false,
        color: None,
        start: start.map(day),
        due: due.map(day),
    }
}

#[test]
fn spans_clip_inclusively_and_deadline_only_dates_are_milestones() {
    let range = DateRange {
        start: day("2026-11-02"),
        end: day("2026-11-08"),
    };
    assert_eq!(range.days(), 7);
    assert_eq!(
        timeline::clipped(&row(Some("2026-11-01"), Some("2026-11-10")), range),
        Some(VisibleSpan {
            offset: 0,
            days: 7,
            milestone: false,
            clipped_start: true,
            clipped_end: true,
        })
    );
    assert_eq!(
        timeline::clipped(&row(Some("2026-11-03"), Some("2026-11-05")), range),
        Some(VisibleSpan {
            offset: 1,
            days: 3,
            milestone: false,
            clipped_start: false,
            clipped_end: false,
        })
    );
    for date in ["2026-11-02", "2026-11-08"] {
        let milestone = timeline::clipped(&row(None, Some(date)), range).unwrap();
        assert!(milestone.milestone);
        assert_eq!(milestone.days, 1);
        let one_day = timeline::clipped(&row(Some(date), Some(date)), range).unwrap();
        assert!(
            !one_day.milestone,
            "An explicit one-day task is still a bar"
        );
        assert_eq!(one_day.days, 1);
    }
    assert_eq!(timeline::clipped(&row(None, None), range), None);
    assert_eq!(
        timeline::clipped(&row(None, Some("2026-11-09")), range),
        None
    );
    assert_eq!(
        timeline::clipped(&row(Some("2026-10-01"), Some("2026-11-01")), range),
        None
    );
    assert_eq!(
        timeline::clipped(&row(Some("2026-11-10"), Some("2026-11-01")), range),
        None
    );
    assert_eq!(
        timeline::clipped(
            &row(None, Some("2026-11-05")),
            DateRange {
                start: range.end,
                end: range.start,
            }
        ),
        None
    );
}

#[test]
fn windows_and_navigation_handle_leap_months_years_and_bounds() {
    assert_eq!(
        timeline::window(day("2024-02-15"), Scale::Month).unwrap(),
        DateRange {
            start: day("2024-02-01"),
            end: day("2024-02-29"),
        }
    );
    assert_eq!(
        timeline::window(day("2026-01-01"), Scale::Week).unwrap(),
        DateRange {
            start: day("2025-12-29"),
            end: day("2026-01-04"),
        }
    );
    assert_eq!(
        timeline::shift(day("2026-12-31"), Scale::Month, 1).unwrap(),
        day("2027-01-01")
    );
    assert_eq!(
        timeline::shift(day("2026-01-01"), Scale::Month, -1).unwrap(),
        day("2025-12-01")
    );
    assert_eq!(
        timeline::shift(day("2026-01-01"), Scale::Week, 1).unwrap(),
        day("2026-01-05")
    );
    assert_eq!(
        timeline::shift(day("2026-01-01"), Scale::Week, -1).unwrap(),
        day("2025-12-22")
    );
    for scale in [Scale::Week, Scale::Month] {
        assert!(timeline::shift(day("2026-01-01"), scale, i32::MAX).is_err());
        assert!(timeline::shift(day("2026-01-01"), scale, i32::MIN).is_err());
    }
}
