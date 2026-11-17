use jig::{
    documents::{read_ods, save_ods},
    model::State,
    sheet_controls::{ControlKind, ControlTarget, MAX_CONTROLS},
    spreadsheet::{CellAddr, CellRange, CellValue, Workbook, MAX_COLS, MAX_ROWS},
};
use std::io::{Cursor, Read, Write};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

fn cell(text: &str) -> CellAddr {
    CellAddr::parse(text).unwrap()
}
fn target(sheet: u64, text: &str) -> Option<ControlTarget> {
    Some(ControlTarget {
        sheet,
        cell: cell(text),
    })
}
fn slider() -> ControlKind {
    ControlKind::Slider {
        min: 0.0,
        max: 1.0,
        step: 0.1,
    }
}

#[test]
fn old_documents_default_to_no_controls_and_round_trip_new_metadata() {
    let old = State::from_json(
        r#"{"version":1,"preferences":{},"history":[],"tasks":[],"deadlines":[],"entries":[],"featured":null,"workbook":{"version":1,"sheets":[{"id":1,"name":"Old","cells":{}}]}}"#,
    )
    .unwrap();
    assert!(old.workbook.as_ref().unwrap().controls.is_empty());
    assert!(!serde_json::to_string(&old).unwrap().contains("controls"));
    let mut state = old;
    let book = state.workbook.as_mut().unwrap();
    book.add_control(" Input ", target(1, "A1"), ControlKind::Number)
        .unwrap();
    book.add_control("Result", target(1, "B1"), ControlKind::Result)
        .unwrap();
    book.add_control("Missing source", None, slider()).unwrap();
    let json = serde_json::to_string(&state).unwrap();
    let restored = State::from_json(&json).unwrap();
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(state).unwrap()
    );
}

#[test]
fn number_inputs_share_cell_precision_and_formula_evaluation() {
    let mut book = Workbook::default();
    book.set_cell(1, cell("B1"), "=A1*3").unwrap();
    let input = book
        .add_control("Price", target(1, "A1"), ControlKind::Number)
        .unwrap();
    let output = book
        .add_control("Total", target(1, "B1"), ControlKind::Result)
        .unwrap();
    let value = 12.3456789012345;
    assert!(book.set_control_value(input, value).unwrap());
    assert_eq!(book.raw(1, cell("A1")).parse::<f64>().unwrap(), value);
    assert!(!book.set_control_value(input, value).unwrap());
    assert_eq!(
        book.recalculate().get(1, cell("B1")),
        Some(&CellValue::Number(value * 3.0))
    );
    let before = book.clone();
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(book.set_control_value(input, invalid).is_err());
        assert_eq!(book, before);
    }
    assert!(book.set_control_value(output, 7.0).is_err());
    assert_eq!(book, before);
}

#[test]
fn sliders_clamp_snap_idempotently_and_keep_non_aligned_endpoints() {
    let offset = ControlKind::Slider {
        min: 0.2,
        max: 2.0,
        step: 0.3,
    };
    let preview = offset.normalize_value(0.61).unwrap();
    assert_eq!(preview, 0.5, "The step grid starts at min, not zero");
    assert_eq!(offset.normalize_value(preview).unwrap(), preview);
    let mut book = Workbook::default();
    let id = book
        .add_control("Scale", target(1, "A1"), slider())
        .unwrap();
    assert!(book.set_control_value(id, 0.36).unwrap());
    assert!((book.raw(1, cell("A1")).parse::<f64>().unwrap() - 0.4).abs() < 1e-15);
    assert!(!book.set_control_value(id, 0.36).unwrap());
    book.set_control_value(id, -100.0).unwrap();
    assert_eq!(book.raw(1, cell("A1")), "0");
    book.set_control_value(id, 100.0).unwrap();
    assert_eq!(book.raw(1, cell("A1")), "1");
    book.update_control(
        id,
        "Scale",
        target(1, "A1"),
        ControlKind::Slider {
            min: 0.0,
            max: 10.0,
            step: 3.0,
        },
    )
    .unwrap();
    book.set_control_value(id, 10.0).unwrap();
    assert_eq!(book.raw(1, cell("A1")), "10");
    book.update_control(
        id,
        "Scale",
        target(1, "A1"),
        ControlKind::Slider {
            min: -3.0,
            max: 3.0,
            step: 0.25,
        },
    )
    .unwrap();
    book.set_control_value(id, -0.13).unwrap();
    assert_eq!(book.raw(1, cell("A1")), "-0.25");
}

#[test]
fn formulas_and_hidden_merges_are_checked_again_on_every_write_but_still_load() {
    let mut book = Workbook::default();
    book.set_cell(1, cell("A1"), "5").unwrap();
    let id = book
        .add_control("Input", target(1, "A1"), ControlKind::Number)
        .unwrap();
    book.set_cell(1, cell("A1"), "  =2+2").unwrap();
    let state = State {
        workbook: Some(book.clone()),
        ..State::default()
    };
    State::from_json(&serde_json::to_string(&state).unwrap()).unwrap();
    assert!(book
        .control_input_target(id)
        .unwrap_err()
        .contains("formula"));
    assert!(book.set_control_value(id, 12.0).is_err());
    assert_eq!(book.raw(1, cell("A1")), "  =2+2");

    book.set_cell(1, cell("A1"), "5").unwrap();
    let hidden = book
        .add_control("Hidden", target(1, "B2"), ControlKind::Number)
        .unwrap();
    book.merge(1, CellRange::new(cell("A1"), cell("B2")))
        .unwrap();
    book.validate().unwrap();
    assert!(book
        .set_control_value(hidden, 99.0)
        .unwrap_err()
        .contains("hidden"));
    assert!(
        book.set_control_value(id, 8.0).unwrap(),
        "A visible merge anchor remains a valid input"
    );
    assert_eq!(book.raw(1, cell("A1")), "8");
    assert!(book.raw(1, cell("B2")).is_empty());
    book.unmerge(1, CellRange::new(cell("A1"), cell("B2")))
        .unwrap();
    assert!(book.set_control_value(hidden, 99.0).unwrap());
}

#[test]
fn axis_edits_follow_cells_and_deleted_sources_never_rebind_to_neighbors() {
    let mut book = Workbook::default();
    let other = book.add_sheet("Other").unwrap();
    let id = book
        .add_control("Moving", target(1, "C5"), ControlKind::Number)
        .unwrap();
    let unrelated = book
        .add_control("Other", target(other, "A1"), ControlKind::Result)
        .unwrap();
    book.insert_rows(1, 1, 2).unwrap();
    book.insert_columns(1, 1, 1).unwrap();
    assert_eq!(book.controls[0].target, target(1, "D7"));
    book.delete_rows(1, 0, 1).unwrap();
    book.delete_columns(1, 1, 1).unwrap();
    assert_eq!(book.controls[0].target, target(1, "C6"));
    assert_eq!(book.controls[1].target, target(other, "A1"));
    let before_delete = book.clone();
    book.delete_rows(1, 5, 1).unwrap();
    assert_eq!(book.controls[0].target, None);
    assert_eq!(book.controls[0].id, id);
    book.set_cell(1, cell("C6"), "47").unwrap();
    assert!(book.set_control_value(id, 9.0).is_err());
    assert_eq!(book.raw(1, cell("C6")), "47");
    // The browser's ordinary whole-document Undo snapshot carries controls.
    book = before_delete;
    assert_eq!(book.controls[0].target, target(1, "C6"));
    book.remove_sheet(other).unwrap();
    assert_eq!(
        book.controls
            .iter()
            .find(|c| c.id == unrelated)
            .unwrap()
            .target,
        None
    );
    let reused_id = book.add_sheet("Replacement").unwrap();
    assert_eq!(reused_id, other);
    assert_eq!(
        book.controls
            .iter()
            .find(|c| c.id == unrelated)
            .unwrap()
            .target,
        None
    );
}

#[test]
fn insertion_overflow_cannot_discard_a_link_to_an_empty_edge_cell() {
    let mut book = Workbook::default();
    book.add_control(
        "Bottom",
        Some(ControlTarget {
            sheet: 1,
            cell: CellAddr::new(MAX_ROWS - 1, 0),
        }),
        ControlKind::Number,
    )
    .unwrap();
    book.add_control(
        "Right",
        Some(ControlTarget {
            sheet: 1,
            cell: CellAddr::new(0, MAX_COLS - 1),
        }),
        ControlKind::Result,
    )
    .unwrap();
    let before = book.clone();
    assert!(book.insert_rows(1, 0, 1).is_err());
    assert_eq!(book, before);
    assert!(book.insert_columns(1, 0, 1).is_err());
    assert_eq!(book, before);
}

#[test]
fn configuration_order_and_limits_are_validated_atomically() {
    let mut book = Workbook::default();
    let one = book
        .add_control(" One ", target(1, "A1"), ControlKind::Number)
        .unwrap();
    let two = book.add_control("Two", None, ControlKind::Result).unwrap();
    let three = book
        .add_control("Three", target(1, "C1"), slider())
        .unwrap();
    assert_eq!(book.controls[0].label, "One");
    assert!(book.move_control(one, isize::MAX).unwrap());
    assert_eq!(
        book.controls.iter().map(|c| c.id).collect::<Vec<_>>(),
        [two, three, one]
    );
    assert!(!book.move_control(one, 1).unwrap());
    assert!(!book
        .update_control(one, "One", target(1, "A1"), ControlKind::Number)
        .unwrap());
    let before = book.clone();
    for kind in [
        ControlKind::Slider {
            min: 1.0,
            max: 1.0,
            step: 1.0,
        },
        ControlKind::Slider {
            min: 0.0,
            max: 1.0,
            step: 0.0,
        },
        ControlKind::Slider {
            min: 0.0,
            max: 1.0,
            step: 2.0,
        },
        ControlKind::Slider {
            min: -f64::MAX,
            max: f64::MAX,
            step: 1.0,
        },
        ControlKind::Slider {
            min: 0.0,
            max: 1.0,
            step: f64::MIN_POSITIVE / 4.0,
        },
        ControlKind::Slider {
            min: 1e20,
            max: 1e20 + 1e6,
            step: 1.0,
        },
    ] {
        assert!(book
            .update_control(one, "One", target(1, "A1"), kind)
            .is_err());
        assert_eq!(book, before);
    }
    for (label, source) in [
        ("", target(1, "A1")),
        ("bad\nlabel", target(1, "A1")),
        ("Missing", target(99, "A1")),
    ] {
        assert!(book
            .add_control(label, source, ControlKind::Number)
            .is_err());
        assert_eq!(book, before);
    }
    assert!(book.move_control(99, -1).is_err());
    assert!(!book.remove_control(99).unwrap());
    assert!(book.remove_control(two).unwrap());
    while book.controls.len() < MAX_CONTROLS {
        book.add_control("Output", None, ControlKind::Result)
            .unwrap();
    }
    let before = book.clone();
    assert!(book
        .add_control("Too many", None, ControlKind::Result)
        .is_err());
    assert_eq!(book, before);
    book.controls[1].id = book.controls[0].id;
    assert!(book.validate().unwrap_err().contains("unique"));
}

fn rewrite_ods(bytes: &[u8], change: impl FnOnce(String) -> String) -> Vec<u8> {
    let mut input = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut entries = Vec::new();
    for index in 0..input.len() {
        let mut entry = input.by_index(index).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        entries.push((entry.name().to_owned(), bytes));
    }
    let entry = entries
        .iter_mut()
        .find(|(name, _)| name == "content.xml")
        .unwrap();
    entry.1 = change(String::from_utf8(std::mem::take(&mut entry.1)).unwrap()).into_bytes();
    let mut output = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        output
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    output.finish().unwrap().into_inner()
}

#[test]
fn embedded_ods_preserves_controls_and_external_grids_remap_sources_by_name() {
    let mut book = Workbook::default();
    book.rename_sheet(1, "Inputs").unwrap();
    let outputs = book.add_sheet("Outputs").unwrap();
    let discard = book.add_sheet("Other").unwrap();
    book.set_cell(1, cell("A1"), "10").unwrap();
    book.set_cell(outputs, cell("A1"), "25").unwrap();
    book.set_cell(outputs, cell("B1"), "=A1*2").unwrap();
    book.set_cell(discard, cell("A1"), "70").unwrap();
    let input = book
        .add_control("Output input", target(outputs, "A1"), ControlKind::Number)
        .unwrap();
    book.add_control("Calculated", target(outputs, "B1"), ControlKind::Result)
        .unwrap();
    book.add_control("Deleted sheet", target(1, "A1"), ControlKind::Number)
        .unwrap();
    book.add_control("Other", target(discard, "A1"), slider())
        .unwrap();
    book.add_control("Already missing", None, ControlKind::Result)
        .unwrap();
    let mut state = State {
        workbook: Some(book),
        ..State::default()
    };
    state.add_task("Keep this task").unwrap();
    let bytes = save_ods(&state).unwrap();
    let exact = read_ods(&bytes).unwrap();
    assert_eq!(
        serde_json::to_value(exact.embedded_state.unwrap()).unwrap(),
        serde_json::to_value(&state).unwrap()
    );
    let changed = rewrite_ods(&bytes, |mut xml| {
        let start = xml.find("<table:table ").unwrap();
        let end = start + xml[start..].find("</table:table>").unwrap() + "</table:table>".len();
        xml.replace_range(start..end, "");
        assert!(xml.contains("office:value=\"25\""));
        xml = xml.replace(
            "office:value=\"25\"",
            "table:formula=\"of:=3*11\" office:value=\"33\"",
        );
        xml.replace("<text:p>25</text:p>", "<text:p>33</text:p>")
    });
    let imported = read_ods(&changed).unwrap();
    let mut book = imported.workbook;
    assert_eq!(book.sheet(1).unwrap().name, "Outputs");
    assert_eq!(book.controls[0].target, target(1, "A1"));
    assert_eq!(book.controls[1].target, target(1, "B1"));
    assert_eq!(book.controls[2].target, None);
    assert_eq!(book.controls[3].target, target(2, "A1"));
    assert_eq!(book.controls[4].target, None);
    assert_eq!(book.raw(1, cell("A1")), "=3*11");
    assert_eq!(
        book.recalculate().get(1, cell("B1")),
        Some(&CellValue::Number(66.0))
    );
    assert!(book
        .set_control_value(input, 99.0)
        .unwrap_err()
        .contains("formula"));
    assert_eq!(book.raw(1, cell("A1")), "=3*11");
    assert_eq!(
        imported.embedded_state.unwrap().tasks[0].text,
        "Keep this task"
    );
}
