use chrono::{Datelike, TimeZone, Timelike, Utc};
use jig::{
    calculator::evaluate,
    calendar,
    model::{countdown, State, TaskColor},
    units::convert,
};

#[test]
fn arithmetic_precedence_percent_and_reuse() {
    assert_eq!(evaluate("125 * 1.15", 0.0).unwrap(), 143.75);
    assert_eq!(evaluate("(12 + 8) * 50%", 0.0).unwrap(), 10.0);
    assert_eq!(evaluate("1 / 50%", 0.0).unwrap(), 2.0);
    assert_eq!(evaluate("-2 * -(3 + .5)", 0.0).unwrap(), 7.0);
    assert_eq!(evaluate("ans / 4", 10.0).unwrap(), 2.5);
    assert_eq!(evaluate("10 - 2 - 3", 0.0).unwrap(), 5.0);
    for bad in ["", "1/0", "(1+2", "1+2)", "abc", "1..2", "2**3", "2(3)"] {
        assert!(evaluate(bad, 0.0).is_err(), "{bad}");
    }
    assert!(evaluate(&"(".repeat(1000), 0.0).is_err());
}
#[test]
fn invalid_intermediate_arithmetic_cannot_be_masked_by_later_operations() {
    for expression in ["1 / (1 / 0)", "0 / (1 / 0)", "1 / (0 / 0)"] {
        assert!(evaluate(expression, 0.0).is_err(), "{expression}");
    }
    let overflowing_literal = "9".repeat(400);
    assert!(evaluate(&format!("1 / {overflowing_literal}"), 0.0).is_err());
    let finite_literal = "9".repeat(200);
    let overflowing_product = format!("1 / ({finite_literal} * {finite_literal})");
    assert!(evaluate(&overflowing_product, 0.0).is_err());
    assert_eq!(evaluate("1 / (1 / 2)", 0.0).unwrap(), 2.0);
    assert!(evaluate(&format!("1 / {finite_literal}"), 0.0).unwrap() > 0.0);
}

#[test]
fn conversions_are_bidirectional_and_fractional() {
    assert!((convert("12in", "mm").unwrap() - 304.8).abs() < 1e-10);
    assert!((convert("304.8mm", "ft").unwrap() - 1.0).abs() < 1e-10);
    assert!((convert("1 1/2in", "mm").unwrap() - 38.1).abs() < 1e-10);
    assert!((convert("-1 1/2in", "mm").unwrap() + 38.1).abs() < 1e-10);
    assert!((convert("3/4in", "cm").unwrap() - 1.905).abs() < 1e-10);
    for bad in ["12", "1/0in", "nanin", "1yd", "1 2 3in"] {
        assert!(convert(bad, "m").is_err());
    }
    for source in ["in", "ft", "mm", "cm", "m"] {
        let m = convert(&format!("3.125{source}"), "m").unwrap();
        assert!((convert(&format!("{m}m"), source).unwrap() - 3.125).abs() < 1e-10);
    }
}

#[test]
fn hybrid_input_recognizes_math_and_length_conversion_syntax() {
    use jig::workbench;

    for input in [
        "72 in mm",
        "72in mm",
        "72 in to mm",
        "72in->mm",
        "72 in → mm",
        "72 IN MILLIMETERS",
        "72 inches millimetres",
    ] {
        let result = workbench::evaluate(input, 0.0).unwrap();
        assert!((result.value - 1828.8).abs() < 1e-10, "{input}");
        assert_eq!(result.unit.as_deref(), Some("mm"));
    }
    for (input, expected) in [
        ("1 1/2 in cm", 3.81),
        ("-1 1/2 in cm", -3.81),
        ("3/4 in mm", 19.05),
        ("2 feet inches", 24.0),
        ("1828.8 mm in", 72.0),
        ("ans in mm", 50.8),
    ] {
        assert!(
            (workbench::evaluate(input, 2.0).unwrap().value - expected).abs() < 1e-10,
            "{input}"
        );
    }
    let arithmetic = workbench::evaluate("(125 * 1.15) + ans", 2.0).unwrap();
    assert_eq!(arithmetic.value, 145.75);
    assert_eq!(arithmetic.unit, None);
}

#[test]
fn hybrid_errors_do_not_change_history_or_last_answer() {
    let mut state = State::default();
    state.calculate("72 in mm").unwrap();
    for bad in [
        "",
        "72in",
        "72 yd mm",
        "72 in yards",
        "in mm",
        "1/0 in mm",
        "72 in mm extra",
        "12 + 2 in mm",
        "72 in to to mm",
        "1 / 0",
        "72\u{001b} in mm",
    ] {
        assert!(state.calculate(bad).is_err(), "{bad}");
        assert_eq!(state.history.len(), 1, "{bad}");
        assert_eq!(state.history[0].unit.as_deref(), Some("mm"));
    }
    assert!(state.calculate(&"1".repeat(4097)).is_err());
    let answer = state.calculate("ans / 2").unwrap();
    assert!((answer - 914.4).abs() < 1e-10);
    assert_eq!(state.history.last().unwrap().unit, None);
}

#[test]
fn hybrid_history_preserves_units_precision_and_old_state_files() {
    let mut state = State::default();
    state.calculate("1 / 3").unwrap();
    let old_text = serde_json::to_string(&state).unwrap();
    assert!(!old_text.contains("\"unit\""));
    let mut restored = State::from_json(&old_text).unwrap();
    assert_eq!(restored.history[0].unit, None);
    restored.calculate("ans in mm").unwrap();
    let converted = restored.history.last().unwrap();
    let precise = converted.result;
    assert_eq!(converted.formatted(2), "8.47 mm");
    assert_eq!(restored.history[0].formatted(2), "0.33");
    let reuse = format!("{} in", converted.reusable_input());
    let again = jig::workbench::evaluate(&reuse, 0.0).unwrap();
    assert!((again.value - 1.0 / 3.0).abs() < 1e-15);
    let text = serde_json::to_string(&restored).unwrap();
    let decoded = State::from_json(&text).unwrap();
    assert_eq!(decoded.history[1].result, precise);
    assert_eq!(decoded.history[1].unit.as_deref(), Some("mm"));
    for bad_unit in ["inches", "yd", "mm\u{001b}[31m", ""] {
        let mut invalid = decoded.clone();
        invalid.history[1].unit = Some(bad_unit.into());
        assert!(invalid.validate().is_err(), "{bad_unit}");
        assert!(State::from_json(&serde_json::to_string(&invalid).unwrap()).is_err());
    }

    // This conversion is just below 3.81 in binary floating point. Reading its
    // JSON must preserve the exact value, including across repeated restarts.
    let mut fractional = State::default();
    fractional.calculate("1 1/2 in cm").unwrap();
    let expected_bits = fractional.history[0].result.to_bits();
    for _ in 0..3 {
        fractional = State::from_json(&serde_json::to_string(&fractional).unwrap()).unwrap();
        assert_eq!(fractional.history[0].result.to_bits(), expected_bits);
    }
}
#[test]
fn task_lifecycle_and_reordering() {
    let mut s = State::default();
    let a = s.add_task("Photograph sculptures").unwrap();
    let b = s.add_task("Pack prints").unwrap();
    s.edit_task(a, "Photograph new sculptures").unwrap();
    s.toggle_task(a).unwrap();
    assert!(s.tasks[0].complete);
    s.move_task(b, 1).unwrap();
    assert_eq!(s.tasks[0].id, b);
    assert!(s.move_task(a, 0).is_err());
    assert!(s.edit_task(a, "\n").is_err());
    s.remove_task(a).unwrap();
    assert!(s.toggle_task(a).is_err());
    assert_eq!(s.tasks.len(), 1);
}
#[test]
fn deadline_timezone_feature_and_countdown() {
    let mut s = State::default();
    let id = s
        .set_deadline(None, "Exhibition", "2026-12-01T18:00:00+02:00")
        .unwrap();
    assert_eq!(s.deadlines[0].at.hour(), 16);
    assert_eq!(s.featured, Some(id));
    let now = Utc.with_ymd_and_hms(2026, 11, 30, 14, 57, 56).unwrap();
    assert_eq!(countdown(s.deadlines[0].at, now), "1d 01h 02m 04s");
    assert!(countdown(now, s.deadlines[0].at).starts_with("Overdue"));
    s.set_deadline(Some(id), "Opening", "2026-12-02T18:00:00Z")
        .unwrap();
    s.remove_deadline(id).unwrap();
    assert_eq!(s.featured, None);
    assert!(s.feature(id).is_err());
    assert!(s.set_deadline(None, "Bad", "2026-12-01T18:00:00").is_err());
}
#[test]
fn calendar_leap_year_and_year_boundary() {
    let cells = calendar::month(2024, 2).unwrap();
    assert_eq!(cells.len(), 42);
    assert_eq!(cells.iter().flatten().count(), 29);
    assert_eq!(cells[3].unwrap().day(), 1);
    assert_eq!(calendar::shift(2026, 12, 1).unwrap(), (2027, 1));
    assert_eq!(calendar::shift(2026, 1, -1).unwrap(), (2025, 12));
    assert!(calendar::month(2026, 13).is_err());
}
#[test]
fn portable_state_roundtrip_preserves_precision_and_entries() {
    let mut s = State::default();
    s.calculate("1 / 3").unwrap();
    s.add_task("釉薬 / Glaze").unwrap();
    let id = s.set_entry(None, "Studio visit", "2026-12-02").unwrap();
    s.set_entry(Some(id), "Workshop", "2026-12-03").unwrap();
    s.set_deadline(None, "Opening", "2026-12-02T18:00:00Z")
        .unwrap();
    let decoded: State = serde_json::from_str(&serde_json::to_string_pretty(&s).unwrap()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.history[0].result, 1.0 / 3.0);
    assert_eq!(decoded.number(decoded.history[0].result), "0.33");
    assert_eq!(decoded.entries[0].title, "Workshop");
    assert_eq!(decoded.featured, s.featured);
    s.remove_entry(id).unwrap();
    assert!(s.entries.is_empty());
}

#[test]
fn imported_history_rejects_terminal_controls_and_preserves_ascii_whitespace() {
    let expression = "1 +\n 2\t* 3";
    let mut state = State::default();
    assert_eq!(state.calculate(expression).unwrap(), 7.0);
    let restored: State = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored.history[0].expression, expression);
    for invalid in ["\x1b[31m1", "\u{009b}31m1", "1\x7f"] {
        let mut imported = restored.clone();
        imported.history[0].expression = invalid.into();
        assert!(imported.validate().is_err());
    }
    let maximum_length = format!("1{}", " ".repeat(4095));
    state.calculate(&maximum_length).unwrap();
    state.validate().unwrap();
    state.history[0].expression = "1".repeat(4097);
    assert!(state.validate().is_err());
}

#[test]
fn portable_decoder_rejects_unrelated_or_partial_json() {
    for bad in [
        "{}",
        "[]",
        "null",
        "{\"name\":\"some other application\",\"version\":1}",
        "{\"version\":1}",
    ] {
        assert!(State::from_json(bad).is_err());
    }
    let sample = jig::sample::state(Utc::now());
    let text = serde_json::to_string(&sample).unwrap();
    let restored = State::from_json(&text).unwrap();
    assert_eq!(restored.tasks.len(), sample.tasks.len());
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    value["version"] = 2.into();
    assert!(State::from_json(&value.to_string()).is_err());
}

#[test]
fn deleting_completed_tasks_keeps_unfinished_order_and_other_state() {
    let mut state = State::default();
    let first = state.add_task("Keep first").unwrap();
    let completed = state.add_task("Remove completed").unwrap();
    let last = state.add_task("Keep last").unwrap();
    state.toggle_task(completed).unwrap();
    state.calculate("1 / 3").unwrap();
    assert_eq!(state.remove_completed_tasks(), 1);
    assert_eq!(
        state.tasks.iter().map(|task| task.id).collect::<Vec<_>>(),
        vec![first, last]
    );
    assert_eq!(state.history[0].result, 1.0 / 3.0);
    assert_eq!(state.remove_completed_tasks(), 0);
    state.toggle_task(first).unwrap();
    state.toggle_task(last).unwrap();
    assert_eq!(state.remove_completed_tasks(), 2);
    assert!(state.tasks.is_empty());
}

#[test]
fn dated_task_lifecycle_keeps_one_checklist_item_and_explicit_star() {
    let mut state = State::default();
    let task = state.add_task("Photograph work").unwrap();
    state.calculate("1 / 3").unwrap();
    assert!(state.feature_task(task).is_err());
    state
        .set_task_deadline(task, Some("2026-12-01T09:00:00-05:00"))
        .unwrap();
    let deadline = state.task_deadline(task).unwrap().id;
    assert_eq!(state.tasks.len(), 1);
    assert_eq!(state.deadlines.len(), 1);
    assert_eq!(
        state.featured, None,
        "A date alone must not select the countdown"
    );
    assert!(state.is_deadline_active(deadline));
    assert!(!state.is_deadline_active(999999));
    state.feature_task(task).unwrap();
    assert_eq!(state.featured, Some(deadline));
    state.edit_task(task, "Photograph finished work").unwrap();
    assert_eq!(
        state.task_deadline(task).unwrap().title,
        "Photograph finished work"
    );
    state
        .set_task_deadline(task, Some("2026-12-02T10:00:00-05:00"))
        .unwrap();
    assert_eq!(state.task_deadline(task).unwrap().id, deadline);
    state
        .set_deadline(Some(deadline), "New name", "2026-12-03T15:00:00Z")
        .unwrap();
    assert_eq!(state.tasks[0].text, "New name");
    state.toggle_task(task).unwrap();
    assert_eq!(state.featured, None);
    assert!(!state.is_deadline_active(deadline));
    assert!(state.feature_task(task).is_err());
    assert!(state.feature(deadline).is_err());
    state.toggle_task(task).unwrap();
    assert!(state.is_deadline_active(deadline));
    assert_eq!(
        state.featured, None,
        "Reopening must not silently restore a star"
    );
    state.feature_task(task).unwrap();
    state.set_task_deadline(task, None).unwrap();
    assert_eq!(state.featured, None);
    assert!(state.task_deadline(task).is_none());
    assert!(state.deadlines.is_empty());
    assert_eq!(state.tasks.len(), 1);
    assert_eq!(state.history[0].result, 1.0 / 3.0);
    state.validate().unwrap();
}

#[test]
fn dated_task_state_preserves_legacy_files_and_validates_links() {
    let mut state = State::default();
    let task = state.add_task("Existing checklist item").unwrap();
    let deadline = state
        .set_deadline(None, "Existing opening", "2026-12-01T18:00:00Z")
        .unwrap();
    let old_json = serde_json::to_string(&state).unwrap();
    assert!(!old_json.contains("\"deadline\":"));
    let mut restored = State::from_json(&old_json).unwrap();
    assert_eq!(
        restored.tasks.len(),
        1,
        "Loading must not migrate legacy deadlines"
    );
    assert_eq!(restored.tasks[0].deadline, None);
    assert!(restored.is_deadline_active(deadline));
    let converted = restored.ensure_task_for_deadline(deadline).unwrap();
    assert_ne!(converted, task);
    assert_eq!(
        restored.ensure_task_for_deadline(deadline).unwrap(),
        converted
    );
    assert_eq!(restored.tasks.len(), 2);
    assert_eq!(
        restored.task_deadline(converted).unwrap().title,
        "Existing opening"
    );
    assert_eq!(restored.featured, Some(deadline));
    let roundtrip = State::from_json(&serde_json::to_string(&restored).unwrap()).unwrap();
    assert_eq!(roundtrip.task_deadline(converted).unwrap().id, deadline);
    let before = serde_json::to_string(&restored).unwrap();
    assert!(restored
        .set_task_deadline(converted, Some("not a date"))
        .is_err());
    assert!(restored.set_task_deadline(999999, None).is_err());
    assert!(restored.ensure_task_for_deadline(999999).is_err());
    assert_eq!(serde_json::to_string(&restored).unwrap(), before);

    let mut invalid = roundtrip.clone();
    invalid.tasks[0].deadline = Some(deadline);
    assert!(
        invalid.validate().is_err(),
        "Two tasks cannot share one date record"
    );
    invalid.tasks[0].deadline = Some(999999);
    assert!(invalid.validate().is_err(), "A linked deadline must exist");
    invalid = roundtrip.clone();
    invalid
        .tasks
        .iter_mut()
        .find(|item| item.id == converted)
        .unwrap()
        .complete = true;
    assert!(
        invalid.validate().is_err(),
        "A completed task cannot remain featured"
    );
    restored.remove_deadline(deadline).unwrap();
    assert_eq!(restored.tasks.len(), 2);
    assert!(restored.task_deadline(converted).is_none());
    assert_eq!(restored.featured, None);
    restored.validate().unwrap();
}

#[test]
fn deleting_dated_tasks_removes_their_dates_without_orphans() {
    let mut state = State::default();
    let first = state.add_task("Keep first").unwrap();
    let completed = state.add_task("Remove completed").unwrap();
    let last = state.add_task("Keep last").unwrap();
    for task in [first, completed, last] {
        state
            .set_task_deadline(task, Some("2026-12-01T18:00:00Z"))
            .unwrap();
    }
    let kept_deadline = state.task_deadline(last).unwrap().id;
    let removed_deadline = state.task_deadline(completed).unwrap().id;
    state.feature_task(last).unwrap();
    state.toggle_task(completed).unwrap();
    assert_eq!(state.remove_completed_tasks(), 1);
    assert_eq!(
        state.tasks.iter().map(|task| task.id).collect::<Vec<_>>(),
        vec![first, last]
    );
    assert!(!state
        .deadlines
        .iter()
        .any(|item| item.id == removed_deadline));
    assert_eq!(state.featured, Some(kept_deadline));
    state.remove_task(last).unwrap();
    assert_eq!(state.featured, None);
    assert!(!state.deadlines.iter().any(|item| item.id == kept_deadline));
    assert_eq!(state.tasks.len(), 1);
    assert_eq!(state.deadlines.len(), 1);
    state.validate().unwrap();
}

#[test]
fn calendar_task_colors_show_every_due_task_without_changing_the_star() {
    let mut state = State::default();
    let first = state.add_task("Photograph sculptures").unwrap();
    let second = state.add_task("Pack prints").unwrap();
    let completed = state.add_task("Order paper").unwrap();
    for (task, color) in [
        (first, TaskColor::Rose),
        (second, TaskColor::Sage),
        (completed, TaskColor::Blue),
    ] {
        state
            .set_task_deadline(task, Some("2026-12-02T14:00:00Z"))
            .unwrap();
        state.set_task_color(task, Some(color)).unwrap();
    }
    state.toggle_task(completed).unwrap();
    state.feature_task(first).unwrap();
    let date = chrono::NaiveDate::from_ymd_opt(2026, 12, 2).unwrap();
    let before = serde_json::to_string(&state).unwrap();
    let rows = state.tasks_on_date(date, &Utc);
    assert_eq!(rows.len(), 3, "Same-day tasks must not hide each other");
    assert_eq!(rows[0].task_id, Some(first));
    assert_eq!(rows[0].title, "Photograph sculptures");
    assert_eq!(rows[0].color, Some(TaskColor::Rose));
    assert!(!rows[0].complete);
    assert_eq!(rows[1].task_id, Some(second));
    assert_eq!(rows[1].color, Some(TaskColor::Sage));
    assert_eq!(rows[2].task_id, Some(completed));
    assert_eq!(rows[2].color, Some(TaskColor::Blue));
    assert!(rows[2].complete);
    assert!(state.tasks_on_date(date.pred_opt().unwrap(), &Utc).is_empty());
    assert!(state.tasks_on_date(date.succ_opt().unwrap(), &Utc).is_empty());
    assert_eq!(serde_json::to_string(&state).unwrap(), before);
    assert_eq!(state.featured, Some(rows[0].deadline_id));
    assert_eq!(state.tasks.len(), 3);
    assert_eq!(state.deadlines.len(), 3);
}

#[test]
fn calendar_task_colors_follow_date_edits_completion_and_local_dates() {
    let mut state = State::default();
    let task = state.add_task("Install work").unwrap();
    state
        .set_task_deadline(task, Some("2026-12-02T01:30:00Z"))
        .unwrap();
    state.set_task_color(task, Some(TaskColor::Amber)).unwrap();
    let deadline = state.task_deadline(task).unwrap().id;
    state.feature_task(task).unwrap();
    let local = chrono::FixedOffset::west_opt(5 * 3600).unwrap();
    let first = chrono::NaiveDate::from_ymd_opt(2026, 12, 1).unwrap();
    let next = first.succ_opt().unwrap();
    assert_eq!(state.tasks_on_date(first, &local).len(), 1);
    assert!(state.tasks_on_date(first, &Utc).is_empty());
    assert_eq!(state.tasks_on_date(next, &Utc).len(), 1);
    assert!(state.tasks_on_date(next, &local).is_empty());

    state
        .set_task_deadline(task, Some("2026-12-02T09:00:00-05:00"))
        .unwrap();
    state.edit_task(task, "Install finished work").unwrap();
    assert!(state.tasks_on_date(first, &local).is_empty());
    let row = state.tasks_on_date(next, &local)[0];
    assert_eq!(row.deadline_id, deadline, "Editing keeps one deadline record");
    assert_eq!(row.title, "Install finished work");
    assert_eq!(row.color, Some(TaskColor::Amber));
    assert_eq!(state.featured, Some(deadline));
    state.set_task_color(task, Some(TaskColor::Lilac)).unwrap();
    assert_eq!(state.tasks_on_date(next, &local)[0].color, Some(TaskColor::Lilac));
    assert_eq!(state.featured, Some(deadline));

    state.toggle_task(task).unwrap();
    assert_eq!(state.featured, None);
    let row = state.tasks_on_date(next, &local)[0];
    assert!(row.complete);
    assert_eq!(row.color, Some(TaskColor::Lilac));
    state.toggle_task(task).unwrap();
    assert!(!state.tasks_on_date(next, &local)[0].complete);
    assert_eq!(state.featured, None, "Reopening must not restore the star");
    state.set_task_deadline(task, None).unwrap();
    assert!(state.tasks_on_date(next, &local).is_empty());
    assert_eq!(state.tasks[0].color, Some(TaskColor::Lilac));
    state
        .set_task_deadline(task, Some("2026-12-02T09:00:00-05:00"))
        .unwrap();
    assert_eq!(state.tasks_on_date(next, &local)[0].color, Some(TaskColor::Lilac));
    assert_eq!(state.tasks.len(), 1);
    assert_eq!(state.deadlines.len(), 1);
    state.validate().unwrap();
}

#[test]
fn task_colors_roundtrip_and_legacy_calendar_rows_remain_compatible() {
    let mut state = State::default();
    let task = state.add_task("Existing checklist item").unwrap();
    let legacy = state
        .set_deadline(None, "Existing opening", "2026-12-02T18:00:00Z")
        .unwrap();
    let old = serde_json::to_string_pretty(&state).unwrap();
    assert!(!old.contains("\"color\""));
    let mut restored = State::from_json(&old).unwrap();
    assert_eq!(restored.tasks[0].color, None);
    let date = chrono::NaiveDate::from_ymd_opt(2026, 12, 2).unwrap();
    let before = serde_json::to_string(&restored).unwrap();
    let rows = restored.tasks_on_date(date, &Utc);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].task_id, None);
    assert_eq!(rows[0].deadline_id, legacy);
    assert_eq!(rows[0].title, "Existing opening");
    assert_eq!(rows[0].color, None);
    assert!(!rows[0].complete);
    assert_eq!(serde_json::to_string(&restored).unwrap(), before);

    assert!(restored.set_task_color(task, Some(TaskColor::Rose)).is_err());
    assert!(restored.set_task_color(999999, None).is_err());
    assert_eq!(serde_json::to_string(&restored).unwrap(), before);
    restored
        .set_task_deadline(task, Some("2026-12-02T12:00:00Z"))
        .unwrap();
    for color in TaskColor::ALL {
        restored.set_task_color(task, Some(color)).unwrap();
        let exported = serde_json::to_string_pretty(&restored).unwrap();
        restored = State::from_json(&exported).unwrap();
        assert_eq!(restored.tasks[0].color, Some(color));
        assert_eq!(restored.tasks.len(), 1);
        assert_eq!(restored.deadlines.len(), 2);
        assert_eq!(restored.featured, Some(legacy));
        assert_eq!(restored.tasks_on_date(date, &Utc).len(), 2);
    }
    let mut invalid = serde_json::to_value(&restored).unwrap();
    invalid["tasks"][0]["color"] = "bright_green".into();
    assert!(State::from_json(&invalid.to_string()).is_err());
    restored.set_task_color(task, None).unwrap();
    assert!(!serde_json::to_string(&restored).unwrap().contains("\"color\""));
    restored.validate().unwrap();
}
