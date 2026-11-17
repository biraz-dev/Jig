use chrono::{FixedOffset, NaiveDate, Utc};
use jig::{
    charts::{self, ChartData, ChartKind, ChartScene, ChartSource, ChartSpec, Primitive},
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

fn line_points(scene: &ChartScene) -> Vec<[f64; 2]> {
    scene
        .primitives
        .iter()
        .filter_map(|primitive| match primitive {
            Primitive::Rect {
                x,
                y,
                width: 6.0,
                height: 6.0,
                fill: Some([109, 185, 238]),
                ..
            } => Some([x + 3.0, y + 3.0]),
            _ => None,
        })
        .collect()
}

fn data_segments(scene: &ChartScene) -> usize {
    scene.primitives.iter().filter(|primitive| matches!(primitive,
        Primitive::Line { stroke, .. } if stroke.color == [109, 185, 238] && stroke.width == 2.0
    )).count()
}

#[test]
fn pie_scenes_contain_the_full_circle_and_legend_at_compact_and_desktop_widths() {
    for count in [1, 3, 24] {
        let mut rows = vec![vec!["Label".to_string(), "Value".to_string()]];
        rows.extend((0..count).map(|index| vec![format!("Item {index}"), "1".into()]));
        let mut book = Workbook::default();
        book.set_block(1, CellAddr::new(0, 0), &rows).unwrap();
        let state = State {
            workbook: Some(book),
            ..State::default()
        };
        let spec = chart(
            ChartKind::Pie,
            ChartSource::Range {
                sheet: 1,
                range: CellRange::new(CellAddr::new(0, 0), CellAddr::new(count, 1)),
            },
        );
        for width in [280.0, 619.0, 620.0, 1440.0] {
            for requested_height in [180.0, 280.0] {
                let scene = charts::scene(&state, &spec, width, requested_height).unwrap();
                assert!(scene.height >= requested_height);
                assert!(matches!(
                    scene.primitives.first(),
                    Some(Primitive::Rect { height, .. }) if *height == scene.height
                ));
                let mut slices = 0;
                for primitive in &scene.primitives {
                    match primitive {
                        Primitive::Polygon { points, .. } => {
                            slices += 1;
                            for [x, y] in points {
                                assert!(
                                    *x >= 0.0 && *x <= scene.width && *y >= 0.0
                                        && *y + 1.0 <= scene.height,
                                    "Pie is clipped: {width}×{requested_height}, {count} items, point ({x}, {y}), scene height {}",
                                    scene.height
                                );
                            }
                        }
                        Primitive::Rect { y, height, .. } => {
                            assert!(*y >= 0.0 && y + height <= scene.height);
                        }
                        Primitive::Text { y, size, .. } => {
                            assert!(*y >= *size && y + size * 0.2 <= scene.height);
                        }
                        Primitive::Line { .. } => {}
                    }
                }
                assert_eq!(slices, count);
            }
        }
    }
}

#[test]
fn line_charts_keep_sheet_order_and_recalculate_formula_values() {
    let mut state = table(&[
        &["Label", "Value"],
        &["Third", "3"],
        &["First", "=B2*2"],
        &["", ""],
        &["Second", "-1"],
    ]);
    let spec = chart(
        ChartKind::Line,
        ChartSource::Range {
            sheet: 1,
            range: range("A1", "B5"),
        },
    );
    let ChartData::Series(items) = charts::chart_data(&state, &spec, &Utc).unwrap() else {
        panic!("Expected a line series");
    };
    assert_eq!(
        items
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>(),
        ["Third", "First", "Second"]
    );
    assert_eq!(
        items.iter().map(|item| item.value).collect::<Vec<_>>(),
        [3.0, 6.0, -1.0]
    );
    state
        .workbook
        .as_mut()
        .unwrap()
        .set_cell(1, CellAddr::parse("B2").unwrap(), "4")
        .unwrap();
    let ChartData::Series(updated) = charts::chart_data(&state, &spec, &Utc).unwrap() else {
        panic!("Expected updated values");
    };
    assert_eq!(updated[1].value, 8.0);
    let scene = charts::scene_light(&state, &spec, 720.0, 300.0).unwrap();
    let points = line_points(&scene);
    assert_eq!(points.len(), 3);
    assert_eq!(data_segments(&scene), 2);
    assert!(points.windows(2).all(|pair| pair[0][0] < pair[1][0]));
    assert!(points[1][1] < points[0][1] && points[0][1] < points[2][1]);
    let svg = charts::to_svg(&scene);
    assert!(svg.contains("stroke=\"#6db9ee\" stroke-width=\"2\""));
    assert!(
        !svg.contains("<polygon"),
        "A line series must remain an open path"
    );
    state
        .workbook
        .as_mut()
        .unwrap()
        .set_cell(1, CellAddr::parse("B3").unwrap(), "=1/0")
        .unwrap();
    assert!(charts::chart_data(&state, &spec, &Utc)
        .unwrap_err()
        .contains("needs a number"));
}

#[test]
fn line_scenes_handle_single_constant_zero_tiny_and_extreme_values() {
    for values in [
        &["42"][..],
        &["0", "0", "0"],
        &["7", "7", "7"],
        &["-7", "-7", "-7"],
        &["-1e-300", "0", "2e-300"],
        &["-1e308", "0", "1e308"],
    ] {
        let rows = values
            .iter()
            .map(|value| ["Point", *value])
            .collect::<Vec<_>>();
        let refs = rows.iter().map(|row| row.as_slice()).collect::<Vec<_>>();
        let state = table(&refs);
        let spec = chart(
            ChartKind::Line,
            ChartSource::Range {
                sheet: 1,
                range: CellRange::new(CellAddr::new(0, 0), CellAddr::new(values.len() - 1, 1)),
            },
        );
        let scene = charts::scene(&state, &spec, 280.0, 180.0).unwrap();
        let points = line_points(&scene);
        assert_eq!(points.len(), values.len(), "{values:?}");
        assert_eq!(data_segments(&scene), values.len() - 1, "{values:?}");
        assert!(
            points.iter().all(|[x, y]| x.is_finite()
                && y.is_finite()
                && *x > 0.0
                && *x < scene.width
                && *y > 0.0
                && *y < scene.height),
            "{values:?}"
        );
        if values.len() == 1 {
            assert!((scene.width * 0.25..scene.width * 0.75).contains(&points[0][0]));
        } else if values.iter().all(|value| *value == values[0]) {
            assert!(points.iter().all(|point| point[1] == points[0][1]));
        } else {
            assert!(points[0][1] > points[1][1] && points[1][1] > points[2][1]);
        }
        let svg = charts::to_svg(&scene);
        assert!(!svg.contains("NaN") && !svg.contains("inf"), "{values:?}");
        if values[0] == "-1e308" {
            assert!((points[1][1] - (points[0][1] + points[2][1]) * 0.5).abs() < 1e-10);
            assert!(svg.contains("e308"));
        }
        if values[0] == "-1e-300" {
            assert!(svg.contains("e-300"));
        }
    }
}

#[test]
fn line_scenes_stay_compact_and_show_sparse_endpoint_labels() {
    let mut book = Workbook::default();
    for index in 0..200 {
        book.sheets[0]
            .cells
            .insert(format!("A{}", index + 1), format!("Point {index:03}"));
        book.sheets[0]
            .cells
            .insert(format!("B{}", index + 1), index.to_string());
    }
    let state = State {
        workbook: Some(book),
        ..State::default()
    };
    let spec = chart(
        ChartKind::Line,
        ChartSource::Range {
            sheet: 1,
            range: range("A1", "B200"),
        },
    );
    let scene = charts::scene(&state, &spec, 720.0, 300.0).unwrap();
    assert_eq!(scene.height, 300.0);
    assert_eq!(line_points(&scene).len(), 200);
    assert_eq!(data_segments(&scene), 199);
    let labels = scene
        .primitives
        .iter()
        .filter_map(|primitive| match primitive {
            Primitive::Text { text, .. } if text.starts_with("Point ") => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(labels.len() <= 8);
    assert_eq!(labels.first(), Some(&"Point 000"));
    assert_eq!(labels.last(), Some(&"Point 199"));
}

#[test]
fn line_chart_metadata_serializes_and_tracks_axis_edits() {
    let mut state = table(&[&["Label", "Value"], &["A", "2"], &["B", "4"]]);
    let book = state.workbook.as_mut().unwrap();
    let id = book
        .add_chart(
            ChartKind::Line,
            ChartSource::Range {
                sheet: 1,
                range: range("A1", "B3"),
            },
            "Line report",
        )
        .unwrap();
    let encoded = serde_json::to_string(&state).unwrap();
    assert!(encoded.contains("\"kind\":\"line\""));
    let mut restored = State::from_json(&encoded).unwrap();
    let book = restored.workbook.as_mut().unwrap();
    assert_eq!(book.charts[0].kind.label(), "Line");
    assert_eq!(book.charts[0].id, id);
    book.insert_rows(1, 1, 1).unwrap();
    book.insert_columns(1, 0, 1).unwrap();
    assert_eq!(
        book.charts[0].source,
        ChartSource::Range {
            sheet: 1,
            range: range("B1", "C4")
        }
    );
    book.delete_rows(1, 1, 1).unwrap();
    assert_eq!(
        book.charts[0].source,
        ChartSource::Range {
            sheet: 1,
            range: range("B1", "C3")
        }
    );
    let invalid = chart(ChartKind::Line, ChartSource::Tasks);
    assert!(invalid.validate(book).is_err());
    book.delete_columns(1, 2, 1).unwrap();
    assert!(
        book.charts.is_empty(),
        "Deleting the value column removes an unusable chart source"
    );
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
