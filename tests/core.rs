use chrono::{Datelike, TimeZone, Timelike, Utc};
use jig::{
    calculator::evaluate,
    calendar,
    model::{countdown, State},
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
