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
    assert_eq!(
        state.task_deadline(task).unwrap().title,
        "Pack installation"
    );
    assert_eq!(state.tasks.len(), 1);
    assert_eq!(state.deadlines.len(), 1);
    assert_eq!(state.featured, Some(deadline));
    let unchanged = json(&state);
    for (start, due) in [
        (Some("not a date"), Some("2026-11-10T18:00:00Z")),
        (Some("2026-11-03T00:00:00Z"), Some("not a date")),
        (Some("2026-11-10T18:00:00Z"), Some("2026-11-09T18:00:00Z")),
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
    invalid["tasks"][0]
        .as_object_mut()
        .unwrap()
        .remove("deadline");
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

mod resizing {
    use super::{day, json};
    use chrono::{
        DateTime, FixedOffset, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc,
    };
    use jig::{
        model::{parse_deadline, State, TaskColor},
        timeline::{self, ResizeEdge},
    };

    fn scheduled(start: &str, due: &str) -> (State, u64) {
        let mut state = State::default();
        let task = state.add_task("Prepare installation").unwrap();
        state
            .set_task_schedule(task, Some(start), Some(due))
            .unwrap();
        (state, task)
    }

    fn schedule(state: &State, task: u64) -> (DateTime<Utc>, DateTime<Utc>) {
        (
            state
                .tasks
                .iter()
                .find(|item| item.id == task)
                .unwrap()
                .start
                .unwrap(),
            state.task_deadline(task).unwrap().at,
        )
    }

    #[test]
    fn either_edge_can_lengthen_shorten_or_collapse_without_moving_the_other_instant() {
        let start = "2026-11-03T18:15:30.123456789+05:30";
        let due = "2026-11-07T10:45:12.987654321+05:30";
        let (state, task) = scheduled(start, due);
        let before = json(&state);
        let timezone = FixedOffset::east_opt(5 * 3600 + 30 * 60).unwrap();
        for (edge, target, expected_start, expected_due) in [
            (
                ResizeEdge::Start,
                "2026-11-01",
                "2026-11-01T18:15:30.123456789+05:30",
                due,
            ),
            (
                ResizeEdge::Start,
                "2026-11-05",
                "2026-11-05T18:15:30.123456789+05:30",
                due,
            ),
            (ResizeEdge::Start, "2026-11-09", due, due),
            (ResizeEdge::Start, "2026-11-03", start, due),
            (
                ResizeEdge::Due,
                "2026-11-09",
                start,
                "2026-11-09T10:45:12.987654321+05:30",
            ),
            (
                ResizeEdge::Due,
                "2026-11-05",
                start,
                "2026-11-05T10:45:12.987654321+05:30",
            ),
            (ResizeEdge::Due, "2026-11-01", start, start),
            (ResizeEdge::Due, "2026-11-07", start, due),
        ] {
            assert_eq!(
                timeline::resize_schedule(&state, task, edge, day(target), &timezone).unwrap(),
                (
                    parse_deadline(expected_start).unwrap(),
                    parse_deadline(expected_due).unwrap()
                ),
                "{edge:?} to {target}"
            );
            assert_eq!(
                json(&state),
                before,
                "A resize proposal must not change state"
            );
        }
        // Crossing dates still preserves the moved edge's clock when it fits
        // before/after the opposite clock on that final shared day.
        let (state, task) = scheduled("2026-11-03T09:00:00Z", "2026-11-07T17:00:00Z");
        assert_eq!(
            timeline::resize_schedule(&state, task, ResizeEdge::Start, day("2026-11-09"), &Utc)
                .unwrap(),
            (
                parse_deadline("2026-11-07T09:00:00Z").unwrap(),
                schedule(&state, task).1
            )
        );
        assert_eq!(
            timeline::resize_schedule(&state, task, ResizeEdge::Due, day("2026-11-01"), &Utc)
                .unwrap(),
            (
                schedule(&state, task).0,
                parse_deadline("2026-11-03T17:00:00Z").unwrap()
            )
        );
    }

    #[test]
    fn applying_a_resize_reuses_records_and_preserves_task_metadata() {
        for complete in [false, true] {
            let (mut state, task) = scheduled("2026-11-03T09:00:00Z", "2026-11-07T17:00:00Z");
            state.add_task("Keep the checklist order").unwrap();
            state.calculate("1 / 3").unwrap();
            state.set_task_color(task, Some(TaskColor::Lilac)).unwrap();
            state.feature_task(task).unwrap();
            if complete {
                state.toggle_task(task).unwrap();
            }
            let deadline = state.task_deadline(task).unwrap().id;
            let before = json(&state);
            let (start, due) =
                timeline::resize_schedule(&state, task, ResizeEdge::Start, day("2026-11-01"), &Utc)
                    .unwrap();
            assert_eq!(json(&state), before);
            let mut expected = state.clone();
            expected.tasks[0].start = Some(start);
            expected.deadlines[0].at = due;
            state
                .set_task_schedule(task, Some(&start.to_rfc3339()), Some(&due.to_rfc3339()))
                .unwrap();
            state.validate().unwrap();
            assert_eq!(json(&state), json(&expected));
            assert_eq!(state.task_deadline(task).unwrap().id, deadline);
            assert_eq!(state.tasks[0].complete, complete);
            assert_eq!(state.tasks[0].color, Some(TaskColor::Lilac));
            assert_eq!(state.featured, (!complete).then_some(deadline));
            assert_eq!(timeline::rows(&state, &Utc).len(), 2);
            assert_eq!(state.deadlines.len(), 1);
        }
    }

    // Deterministic 2026 US Eastern transitions avoid changing process-wide TZ
    // and exercise gap/fold handling without another timezone dependency.
    #[derive(Clone, Copy, Debug)]
    struct Eastern2026;

    fn eastern_offset(daylight: bool) -> FixedOffset {
        FixedOffset::west_opt(if daylight { 4 * 3600 } else { 5 * 3600 }).unwrap()
    }

    impl TimeZone for Eastern2026 {
        type Offset = FixedOffset;

        fn from_offset(_: &Self::Offset) -> Self {
            Self
        }

        fn offset_from_local_date(&self, date: &NaiveDate) -> LocalResult<Self::Offset> {
            self.offset_from_local_datetime(&date.and_hms_opt(12, 0, 0).unwrap())
        }

        fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<Self::Offset> {
            if local.date() == day("2026-03-08") && local.hour() == 2 {
                return LocalResult::None;
            }
            if local.date() == day("2026-11-01") && local.hour() == 1 {
                return LocalResult::Ambiguous(eastern_offset(true), eastern_offset(false));
            }
            let daylight = *local >= day("2026-03-08").and_hms_opt(3, 0, 0).unwrap()
                && *local < day("2026-11-01").and_hms_opt(1, 0, 0).unwrap();
            LocalResult::Single(eastern_offset(daylight))
        }

        fn offset_from_utc_date(&self, date: &NaiveDate) -> Self::Offset {
            self.offset_from_utc_datetime(&date.and_hms_opt(12, 0, 0).unwrap())
        }

        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> Self::Offset {
            let daylight = *utc >= day("2026-03-08").and_hms_opt(7, 0, 0).unwrap()
                && *utc < day("2026-11-01").and_hms_opt(6, 0, 0).unwrap();
            eastern_offset(daylight)
        }
    }

    #[test]
    fn resizing_across_dst_preserves_each_edges_local_clock() {
        let (state, task) = scheduled("2026-03-06T09:30:00-05:00", "2026-03-10T17:45:00-04:00");
        let original = schedule(&state, task);
        let start = timeline::resize_schedule(
            &state,
            task,
            ResizeEdge::Start,
            day("2026-03-08"),
            &Eastern2026,
        )
        .unwrap();
        assert_eq!(
            start,
            (
                parse_deadline("2026-03-08T09:30:00-04:00").unwrap(),
                original.1
            )
        );
        assert_eq!((start.0 - original.0).num_hours(), 47);
        let due = timeline::resize_schedule(
            &state,
            task,
            ResizeEdge::Due,
            day("2026-03-07"),
            &Eastern2026,
        )
        .unwrap();
        assert_eq!(
            due,
            (
                original.0,
                parse_deadline("2026-03-07T17:45:00-05:00").unwrap()
            )
        );
        assert_eq!((original.1 - due.1).num_hours(), 71);
    }

    #[test]
    fn ambiguous_days_preserve_existing_instants_and_choose_earliest_for_new_dates() {
        let (state, task) = scheduled("2026-11-01T01:30:00-05:00", "2026-11-01T01:45:00-05:00");
        for edge in [ResizeEdge::Start, ResizeEdge::Due] {
            assert_eq!(
                timeline::resize_schedule(&state, task, edge, day("2026-11-01"), &Eastern2026)
                    .unwrap(),
                schedule(&state, task),
                "No-op must retain the second occurrence of the local clock"
            );
        }
        let (state, task) = scheduled("2026-10-30T01:30:00-04:00", "2026-11-04T01:45:00-05:00");
        assert_eq!(
            timeline::resize_schedule(
                &state,
                task,
                ResizeEdge::Start,
                day("2026-11-01"),
                &Eastern2026
            )
            .unwrap(),
            (
                parse_deadline("2026-11-01T01:30:00-04:00").unwrap(),
                schedule(&state, task).1
            )
        );
        assert_eq!(
            timeline::resize_schedule(
                &state,
                task,
                ResizeEdge::Due,
                day("2026-11-01"),
                &Eastern2026
            )
            .unwrap(),
            (
                schedule(&state, task).0,
                parse_deadline("2026-11-01T01:45:00-04:00").unwrap()
            )
        );
    }

    #[test]
    fn nonexistent_local_clock_rejects_the_edit_without_changing_either_endpoint() {
        let (state, task) = scheduled("2026-03-07T02:30:00-05:00", "2026-03-10T02:30:00-04:00");
        let before = json(&state);
        for edge in [ResizeEdge::Start, ResizeEdge::Due] {
            assert!(
                timeline::resize_schedule(&state, task, edge, day("2026-03-08"), &Eastern2026)
                    .is_err()
            );
            assert_eq!(json(&state), before);
        }
    }

    #[test]
    fn resizing_requires_an_existing_ranged_task_and_supported_calendar_dates() {
        let (mut state, task) = scheduled("2026-11-03T09:00:00Z", "2026-11-07T17:00:00Z");
        let undated = state.add_task("Undated task").unwrap();
        let milestone = state.add_task("Deadline-only task").unwrap();
        state
            .set_task_deadline(milestone, Some("2026-11-07T17:00:00Z"))
            .unwrap();
        let legacy = state
            .set_deadline(None, "Legacy deadline", "2026-11-07T17:00:00Z")
            .unwrap();
        let before = json(&state);
        for task_id in [undated, milestone, legacy, 9999] {
            for edge in [ResizeEdge::Start, ResizeEdge::Due] {
                assert!(
                    timeline::resize_schedule(&state, task_id, edge, day("2026-11-05"), &Utc)
                        .is_err()
                );
                assert_eq!(json(&state), before);
            }
        }
        for date in [
            NaiveDate::MIN,
            NaiveDate::MAX,
            day("0000-01-01"),
            NaiveDate::from_ymd_opt(10000, 1, 1).unwrap(),
        ] {
            for edge in [ResizeEdge::Start, ResizeEdge::Due] {
                assert!(timeline::resize_schedule(&state, task, edge, date, &Utc).is_err());
                assert_eq!(json(&state), before);
            }
        }
        let mut invalid = state.clone();
        invalid.tasks[0].start = Some(parse_deadline("2026-11-08T09:00:00Z").unwrap());
        assert!(timeline::resize_schedule(
            &invalid,
            task,
            ResizeEdge::Start,
            day("2026-11-05"),
            &Utc
        )
        .is_err());
        invalid.tasks[0].start = Some(DateTime::<Utc>::MIN_UTC);
        assert!(timeline::resize_schedule(
            &invalid,
            task,
            ResizeEdge::Start,
            day("2026-11-05"),
            &FixedOffset::west_opt(3600).unwrap()
        )
        .is_err());

        let (state, task) = scheduled("0001-01-02T09:00:00Z", "9999-12-30T17:00:00Z");
        assert_eq!(
            timeline::resize_schedule(&state, task, ResizeEdge::Start, day("0001-01-01"), &Utc)
                .unwrap()
                .0,
            parse_deadline("0001-01-01T09:00:00Z").unwrap()
        );
        assert_eq!(
            timeline::resize_schedule(&state, task, ResizeEdge::Due, day("9999-12-31"), &Utc)
                .unwrap()
                .1,
            parse_deadline("9999-12-31T17:00:00Z").unwrap()
        );
        // A supported local date must also produce portable UTC schedule
        // inputs at the bounds; UTC year 10000 cannot be parsed as RFC3339.
        for (start, due, edge, date, offset) in [
            (
                "0001-01-02T00:30:00+01:00",
                "0001-01-03T17:00:00+01:00",
                ResizeEdge::Start,
                "0001-01-01",
                3600,
            ),
            (
                "9999-12-29T09:00:00-01:00",
                "9999-12-30T23:30:00-01:00",
                ResizeEdge::Due,
                "9999-12-31",
                -3600,
            ),
        ] {
            let (state, task) = scheduled(start, due);
            let before = json(&state);
            assert!(timeline::resize_schedule(
                &state,
                task,
                edge,
                day(date),
                &FixedOffset::east_opt(offset).unwrap()
            )
            .is_err());
            assert_eq!(json(&state), before);
        }
    }
}
