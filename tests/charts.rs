use chrono::{FixedOffset, NaiveDate, Utc};
use jig::{
    charts::{self, ChartData, ChartKind, ChartSource, ChartSpec, Primitive},
    model::{State, TaskColor},
    spreadsheet::{CellAddr, CellRange, Workbook},
};

fn range(first: &str, last: &str) -> CellRange {
    CellRange::new(
        CellAddr::parse(first).unwrap(),
        CellAddr::parse(last).unwrap(),
    )
}

fn chart(kind: ChartKind, source: ChartSource) -> ChartSpec {
    ChartSpec {
        id: 1,
        title: "Studio chart".into(),
        kind,
        source,
    }
}

fn table(rows: &[&[&str]]) -> State {
    let mut book = Workbook::default();
    let rows = rows
        .iter()
        .map(|row| row.iter().map(|value| value.to_string()).collect())
        .collect::<Vec<_>>();
    book.set_block(1, CellAddr::new(0, 0), &rows).unwrap();
    State {
        workbook: Some(book),
        ..State::default()
    }
}

#[test]
fn charts_use_evaluated_values_and_legible_escaped_labels() {
    let mut state = table(&[&["Item", "Cost"], &["Clay <&>", "10"], &["Firing", "=B2*3"]]);
    let mut spec = chart(
        ChartKind::Pie,
        ChartSource::Range {
            sheet: 1,
            range: range("A1", "B3"),
        },
    );
    let ChartData::Series(items) = charts::chart_data(&state, &spec, &Utc).unwrap() else {
        panic!("Expected values")
    };
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].value, 10.0);
    assert_eq!(items[1].value, 30.0);
    let scene = charts::scene_light(&state, &spec, 720.0, 300.0).unwrap();
    let svg = charts::to_svg(&scene);
    assert!(svg.contains("Clay &lt;&amp;&gt;"));
    assert!(svg.contains("25.0%") && svg.contains("75.0%"));
    assert_eq!(
        scene
            .primitives
            .iter()
            .filter(|value| matches!(value, Primitive::Polygon { .. }))
            .count(),
        2
    );
    state
        .workbook
        .as_mut()
        .unwrap()
        .set_cell(1, CellAddr::parse("B2").unwrap(), "20")
        .unwrap();
    spec.kind = ChartKind::Bar;
    let ChartData::Series(updated) = charts::chart_data(&state, &spec, &Utc).unwrap() else {
        panic!("Expected values")
    };
    assert_eq!(
        updated[1].value, 60.0,
        "Charts must recalculate formulas after an edit"
    );
    let svg = charts::to_svg(&charts::scene(&state, &spec, 720.0, 300.0).unwrap());
    assert!(svg.contains("60.00"));
}

#[test]
fn pie_rejects_negative_zero_and_error_inputs_instead_of_showing_bad_percentages() {
    let mut state = table(&[&["A", "0"], &["B", "0"]]);
    let spec = chart(
        ChartKind::Pie,
        ChartSource::Range {
            sheet: 1,
            range: range("A1", "B2"),
        },
    );
    assert!(charts::chart_data(&state, &spec, &Utc).is_err());
    state
        .workbook
        .as_mut()
        .unwrap()
        .set_cell(1, CellAddr::parse("B1").unwrap(), "-5")
        .unwrap();
    assert!(charts::chart_data(&state, &spec, &Utc).is_err());
    state
        .workbook
        .as_mut()
        .unwrap()
        .set_cell(1, CellAddr::parse("B1").unwrap(), "=1/0")
        .unwrap();
    assert!(charts::chart_data(&state, &spec, &Utc).is_err());
    let bar = ChartSpec {
        kind: ChartKind::Bar,
        ..spec
    };
    assert!(charts::chart_data(&state, &bar, &Utc).is_err());
}

#[test]
fn bar_scenes_keep_negative_and_tiny_numbers_finite_and_proportional() {
    let state = table(&[&["Small", "-1e-300"], &["Larger", "2e-300"]]);
    let spec = chart(
        ChartKind::Bar,
        ChartSource::Range {
            sheet: 1,
            range: range("A1", "B2"),
        },
    );
    let scene = charts::scene(&state, &spec, 720.0, 300.0).unwrap();
    let bars = scene
        .primitives
        .iter()
        .filter_map(|value| match value {
            Primitive::Rect {
                width,
                height,
                fill: Some([109, 185, 238]),
                ..
            } if *height == 18.0 => Some(*width),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(bars.len(), 2);
    assert!(bars.iter().all(|width| width.is_finite() && *width > 0.0));
    assert!((bars[1] / bars[0] - 2.0).abs() < 1e-10);
}

#[test]
fn gantt_accepts_dates_and_milestones_and_rejects_reversed_ranges() {
    let mut state = table(&[
        &["Piece", "Start", "Due"],
        &["Drying", "2026-11-01", "2026-11-03"],
        &["Kiln ready", "", "2026-11-03"],
    ]);
    let spec = chart(
        ChartKind::Gantt,
        ChartSource::Range {
            sheet: 1,
            range: range("A1", "C3"),
        },
    );
    let ChartData::Schedule(items) = charts::chart_data(&state, &spec, &Utc).unwrap() else {
        panic!("Expected dates")
    };
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].start, NaiveDate::from_ymd_opt(2026, 11, 1));
    assert_eq!(items[1].start, None);
    let scene = charts::scene(&state, &spec, 720.0, 300.0).unwrap();
    assert_eq!(
        scene
            .primitives
            .iter()
            .filter(|value| matches!(value, Primitive::Polygon { points, .. } if points.len() == 4))
            .count(),
        1
    );
    assert!(charts::to_svg(&scene).contains("01 Nov"));
    state
        .workbook
        .as_mut()
        .unwrap()
        .set_cell(1, CellAddr::parse("B2").unwrap(), "2026-11-04")
        .unwrap();
    assert!(charts::chart_data(&state, &spec, &Utc)
        .unwrap_err()
        .contains("starts after"));
}

#[test]
fn formula_dates_use_the_supplied_local_day_for_chart_recalculation() {
    let state = table(&[
        &["Piece", "Start", "Due"],
        &["Drying", "=TODAY()", "=TODAY()+2"],
    ]);
    let spec = chart(
        ChartKind::Gantt,
        ChartSource::Range {
            sheet: 1,
            range: range("A1", "C2"),
        },
    );
    let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
    let ChartData::Schedule(items) = charts::chart_data_at(&state, &spec, &Utc, today).unwrap()
    else {
        panic!("Expected date formulas");
    };
    assert_eq!(items[0].start, Some(today));
    assert_eq!(items[0].due, NaiveDate::from_ymd_opt(2026, 11, 13).unwrap());
    let svg =
        charts::to_svg(&charts::scene_at(&state, &spec, 720.0, 300.0, true, &Utc, today).unwrap());
    assert!(svg.contains("11 Nov"));
}

#[test]
fn task_gantt_is_a_read_only_projection_with_timezone_and_completion() {
    let mut state = State::default();
    let task = state.add_task("Photograph work").unwrap();
    state
        .set_task_schedule(
            task,
            Some("2026-11-01T05:00:00Z"),
            Some("2026-11-02T00:30:00Z"),
        )
        .unwrap();
    state.set_task_color(task, Some(TaskColor::Rose)).unwrap();
    state.feature_task(task).unwrap();
    state
        .set_deadline(None, "Legacy delivery", "2026-11-03T18:00:00Z")
        .unwrap();
    let spec = chart(ChartKind::Gantt, ChartSource::Tasks);
    let before = serde_json::to_string(&state).unwrap();
    let timezone = FixedOffset::west_opt(5 * 3600).unwrap();
    let ChartData::Schedule(items) = charts::chart_data(&state, &spec, &timezone).unwrap() else {
        panic!("Expected tasks")
    };
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].start, NaiveDate::from_ymd_opt(2026, 11, 1));
    assert_eq!(items[0].due, NaiveDate::from_ymd_opt(2026, 11, 1).unwrap());
    assert_eq!(items[0].color, [234, 133, 173]);
    assert_eq!(items[1].start, None);
    let _ = charts::scene_in_timezone(&state, &spec, 720.0, 300.0, false, &timezone).unwrap();
    assert_eq!(
        serde_json::to_string(&state).unwrap(),
        before,
        "Views must not link legacy records or change the star"
    );
    state.toggle_task(task).unwrap();
    let ChartData::Schedule(done) = charts::chart_data(&state, &spec, &timezone).unwrap() else {
        panic!("Expected tasks")
    };
    assert!(done[0].complete);
    let scene = charts::scene_in_timezone(&state, &spec, 720.0, 300.0, false, &timezone).unwrap();
    assert!(scene
        .primitives
        .iter()
        .any(|value| matches!(value, Primitive::Text {text, ..} if text == "Done")));
    assert_eq!(state.tasks.len(), 1);
    assert_eq!(state.deadlines.len(), 2);
}

#[test]
fn chart_metadata_tracks_structural_edits_and_survives_state_roundtrips() {
    let mut state = table(&[&["Item", "Cost"], &["Clay", "10"], &["Firing", "30"]]);
    let book = state.workbook.as_mut().unwrap();
    let id = book
        .add_chart(
            ChartKind::Bar,
            ChartSource::Range {
                sheet: 1,
                range: range("A1", "B3"),
            },
            "Budget",
        )
        .unwrap();
    assert_eq!(book.charts.len(), 1);
    book.insert_rows(1, 1, 1).unwrap();
    assert_eq!(
        book.charts[0].source,
        ChartSource::Range {
            sheet: 1,
            range: range("A1", "B4")
        }
    );
    book.rename_chart(id, "Studio budget").unwrap();
    let restored = State::from_json(&serde_json::to_string(&state).unwrap()).unwrap();
    assert_eq!(
        restored.workbook.as_ref().unwrap().charts,
        state.workbook.as_ref().unwrap().charts
    );
    let before = serde_json::to_string(&state).unwrap();
    assert!(state
        .workbook
        .as_mut()
        .unwrap()
        .add_chart(
            ChartKind::Pie,
            ChartSource::Range {
                sheet: 99,
                range: range("A1", "B3")
            },
            "Missing"
        )
        .is_err());
    assert_eq!(serde_json::to_string(&state).unwrap(), before);
    state.workbook.as_mut().unwrap().remove_chart(id).unwrap();
    assert!(state.workbook.unwrap().charts.is_empty());
}

#[test]
fn scenes_validate_dimensions_and_bound_date_and_category_work() {
    let state = table(&[&["Item", "Cost"], &["Clay", "10"]]);
    let spec = chart(
        ChartKind::Bar,
        ChartSource::Range {
            sheet: 1,
            range: range("A1", "B2"),
        },
    );
    assert!(charts::scene(&state, &spec, f64::NAN, 300.0).is_err());
    assert!(charts::scene(&state, &spec, 200.0, 300.0).is_err());
    let outside = ChartSpec {
        source: ChartSource::Range {
            sheet: 1,
            range: CellRange {
                start: CellAddr::new(3, 0),
                end: CellAddr::new(1, 1),
            },
        },
        ..spec
    };
    assert!(outside.validate(state.workbook.as_ref().unwrap()).is_err());
    let long = table(&[&["Long", "1900-01-01", "2101-01-01"]]);
    let gantt = chart(
        ChartKind::Gantt,
        ChartSource::Range {
            sheet: 1,
            range: range("A1", "C1"),
        },
    );
    assert!(charts::scene(&long, &gantt, 720.0, 300.0)
        .unwrap_err()
        .contains("100-year"));
}
