use chrono::NaiveDate;
use jig::charts::{ChartKind, ChartSource};
use jig::model::State;
use jig::spreadsheet::{
    date_to_serial, encode_delimited, parse_delimited, serial_to_date, BorderPreset, CellAddr,
    CellAlignment, CellBorders, CellError, CellFont, CellRange, CellStyle, CellValue, NumberFormat,
    StylePatch, Workbook, MAX_CELL_BYTES, MAX_COLS, MAX_ROWS,
};

fn addr(text: &str) -> CellAddr {
    CellAddr::parse(text).unwrap()
}

fn put(book: &mut Workbook, sheet: u64, cell: &str, raw: &str) {
    book.set_cell(sheet, addr(cell), raw).unwrap();
}

fn number(book: &Workbook, sheet: u64, cell: &str) -> f64 {
    match book.recalculate().value(sheet, addr(cell)) {
        CellValue::Number(value) => value,
        value => panic!("Expected a number at {cell}, got {value:?}"),
    }
}

#[test]
fn addresses_cover_the_bounded_grid_and_normalize_selections() {
    assert_eq!(addr(" $az$1000 "), CellAddr::new(999, 51));
    assert_eq!(addr("z9").label(), "Z9");
    assert_eq!(addr("AA1").label(), "AA1");
    for text in ["", "A0", "A1001", "BA1", "1A", "A+1", "A1x", "A-1"] {
        assert!(CellAddr::parse(text).is_err(), "{text}");
    }
    assert_eq!(CellAddr::new(usize::MAX, 0).label(), "#REF!");
    let range = CellRange::new(addr("D4"), addr("B2"));
    assert_eq!(range.start, addr("B2"));
    assert_eq!((range.rows(), range.cols()), (3, 3));
    assert!(range.contains(addr("C3")));
    assert!(!range.contains(addr("A1")));
}

#[test]
fn sparse_edits_pastes_and_clear_are_atomic_and_roundtrip_json() {
    let mut book = Workbook::default();
    assert!(book.used_range(1).is_none());
    put(&mut book, 1, "B2", "Clay body");
    put(&mut book, 1, "C3", "104");
    assert!(!book.set_cell(1, addr("C3"), "104").unwrap());
    let before = book.clone();
    assert!(book
        .set_block(1, addr("AZ1000"), &[vec!["last".into(), "overflow".into()]])
        .is_err());
    assert_eq!(book, before);
    assert!(book
        .set_cell(1, addr("A1"), &"x".repeat(MAX_CELL_BYTES + 1))
        .is_err());
    assert_eq!(book, before);
    assert!(book.set_cell(99, addr("A1"), "missing").is_err());
    assert_eq!(book, before);
    let json = serde_json::to_string(&book).unwrap();
    let restored: Workbook = serde_json::from_str(&json).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored, book);
    assert!(book
        .clear(1, CellRange::new(addr("B2"), addr("C3")))
        .unwrap());
    assert!(book.sheet(1).unwrap().cells.is_empty());
    assert!(!book
        .clear(1, CellRange::new(addr("B2"), addr("C3")))
        .unwrap());
}

#[test]
fn arithmetic_recalculates_references_ranges_and_percentages() {
    let mut book = Workbook::default();
    put(&mut book, 1, "A1", "100");
    put(&mut book, 1, "A2", "94");
    put(&mut book, 1, "A3", "50%");
    put(&mut book, 1, "B1", "=(A1-A2)/A1*100");
    put(&mut book, 1, "B2", "=SUM(A1:A3)");
    put(&mut book, 1, "B3", "=AVERAGE(A1:A2)");
    put(&mut book, 1, "C1", "=MIN(A1:A3)+MAX(A1:A3)");
    put(&mut book, 1, "C2", "=ROUND(1/3,4)");
    put(&mut book, 1, "C3", "=ABS(-25)+2^3^2");
    assert_eq!(number(&book, 1, "B1"), 6.0);
    assert_eq!(number(&book, 1, "B2"), 194.5);
    assert_eq!(number(&book, 1, "B3"), 97.0);
    assert_eq!(number(&book, 1, "C1"), 100.5);
    assert_eq!(number(&book, 1, "C2"), 0.3333);
    assert_eq!(number(&book, 1, "C3"), 537.0);
    put(&mut book, 1, "A2", "90");
    assert_eq!(number(&book, 1, "B1"), 10.0);
    assert_eq!(number(&book, 1, "B3"), 95.0);
    assert_eq!(book.raw(1, addr("B1")), "=(A1-A2)/A1*100");
}

#[test]
fn convert_recalculates_measurements_unit_references_and_material_costs() {
    let mut book = Workbook::default();
    for (cell, raw) in [
        ("A1", "25.4"),
        ("A2", "12.7"),
        ("A3", "2.54"),
        ("B1", "=CONVERT(A1,\"cm\",\"in\")"),
        ("B2", "=CONVERT(A2,\"cm\",\"in\")"),
        ("B3", "=CONVERT(A3,\"cm\",\"in\")"),
        ("C1", "=B1*B2"),
        ("C2", "=B1*B2*B3"),
        ("C3", "=C2*0.12"),
        ("D1", "=CONVERT(A1*A2,\"cm2\",\"in2\")"),
        ("D2", "=CONVERT(A1*A2*A3,\"cm3\",\"in3\")"),
        ("E1", "cm"),
        ("E2", "in"),
        ("F1", "=convert($A$1,E1,E2)"),
    ] {
        put(&mut book, 1, cell, raw);
    }
    for (cell, expected) in [
        ("B1", 10.0),
        ("B2", 5.0),
        ("B3", 1.0),
        ("C1", 50.0),
        ("C2", 50.0),
        ("C3", 6.0),
        ("D1", 50.0),
        ("D2", 50.0),
        ("F1", 10.0),
    ] {
        assert!((number(&book, 1, cell) - expected).abs() < 1e-12, "{cell}");
    }
    put(&mut book, 1, "A1", "50.8");
    assert_eq!(number(&book, 1, "B1"), 20.0);
    assert_eq!(number(&book, 1, "C3"), 12.0);
    assert_eq!(number(&book, 1, "F1"), 20.0);
    put(&mut book, 1, "E2", "ft");
    assert!((number(&book, 1, "F1") - 20.0 / 12.0).abs() < 1e-12);
    put(&mut book, 1, "E1", "mm");
    assert!((number(&book, 1, "F1") - 2.0 / 12.0).abs() < 1e-12);
}

#[test]
fn convert_supports_length_aliases_and_matching_area_and_volume_dimensions() {
    let mut book = Workbook::default();
    for (source, target, value, expected) in [
        ("mm", "cm", 10.0, 1.0),
        ("cm", "m", 100.0, 1.0),
        ("m", "mm", 1.0, 1000.0),
        ("ft", "in", 1.0, 12.0),
        ("  CENTIMETRES  ", " Inches ", 2.54, 1.0),
        ("foot", "millimeters", 1.0, 304.8),
        ("feet", "meters", 1.0, 0.3048),
        ("cm2", "m2", 10000.0, 1.0),
        ("mm^2", "cm²", 100.0, 1.0),
        ("in²", "ft^2", 144.0, 1.0),
        ("square centimeters", "sq in", 6.4516, 1.0),
        ("cm3", "m3", 1000000.0, 1.0),
        ("mm^3", "cm³", 1000.0, 1.0),
        ("in³", "ft^3", 1728.0, 1.0),
        ("cubic metres", "cu ft", 0.028316846592, 1.0),
        ("cm", "in", -2.54, -1.0),
        ("cm3", "in3", 0.0, 0.0),
    ] {
        put(
            &mut book,
            1,
            "A1",
            &format!("=CONVERT({value},\"{source}\",\"{target}\")"),
        );
        assert!(
            (number(&book, 1, "A1") - expected).abs() < 1e-12,
            "{value} {source} to {target}"
        );
    }
}

#[test]
fn convert_reports_invalid_arguments_units_dimensions_and_numeric_overflow() {
    let mut book = Workbook::default();
    put(&mut book, 1, "B1", "NaN");
    put(&mut book, 1, "B2", "=1/0");
    for formula in [
        "=CONVERT()",
        "=CONVERT(1,\"cm\")",
        "=CONVERT(1,\"cm\",\"in\",2)",
        "=CONVERT(\"2.54\",\"cm\",\"in\")",
        "=CONVERT(A1:A1,\"cm\",\"in\")",
        "=CONVERT(1,B1:B2,\"in\")",
        "=CONVERT(1,\"cm\",B1:B2)",
        "=CONVERT(1,2,\"in\")",
        "=CONVERT(1,\"cm\",TRUE)",
        "=CONVERT(1,\"\",\"in\")",
        "=CONVERT(1,\"yards\",\"in\")",
        "=CONVERT(1,\"cm\",\"seconds\")",
        "=CONVERT(1,\"unknown\",\"unknown\")",
        "=CONVERT(1,\"cm\",\"in2\")",
        "=CONVERT(1,\"cm2\",\"in3\")",
        "=CONVERT(1,\"cm4\",\"in4\")",
        "=CONVERT(1,\"kg\",\"lb\")",
        "=CONVERT(1,\"mL\",\"ML\")",
    ] {
        put(&mut book, 1, "A1", formula);
        assert_eq!(
            book.recalculate().value(1, addr("A1")),
            CellValue::Error(CellError::Value),
            "{formula}"
        );
    }
    for (formula, expected) in [
        ("=CONVERT(1e308,\"m\",\"mm\")", CellError::Num),
        ("=CONVERT(1e309,\"m\",\"in\")", CellError::Num),
        ("=CONVERT(B1,\"cm\",\"in\")", CellError::Num),
        ("=CONVERT(B2,\"cm\",\"in\")", CellError::DivZero),
        ("=CONVERT(1,B2,\"in\")", CellError::DivZero),
        ("=CONVERT(1,\"cm\",B2)", CellError::DivZero),
        ("=CONVERT(A1001,\"cm\",\"in\")", CellError::Ref),
    ] {
        put(&mut book, 1, "A1", formula);
        assert_eq!(
            book.recalculate().value(1, addr("A1")),
            CellValue::Error(expected),
            "{formula}"
        );
    }
    // Numeric scalar arguments follow the same coercion rules as arithmetic.
    put(&mut book, 1, "A1", "=CONVERT(TRUE,\"in\",\"mm\")");
    assert_eq!(number(&book, 1, "A1"), 25.4);
    put(&mut book, 1, "A1", "=CONVERT(Z999,\"cm\",\"in\")");
    assert_eq!(number(&book, 1, "A1"), 0.0);
}

#[test]
fn convert_preserves_precision_and_finite_extreme_values() {
    let mut book = Workbook::default();
    for (cell, raw) in [
        ("A1", "=CONVERT(1,\"cm\",\"in\")"),
        ("A2", "=A1*2.54"),
        ("A3", "=CONVERT(1e308,\"ft3\",\"ft³\")"),
        ("A4", "=CONVERT(1e-300,\"mm3\",\"m3\")"),
    ] {
        put(&mut book, 1, cell, raw);
    }
    let value = number(&book, 1, "A1");
    assert_eq!(value, 0.01_f64 / 0.0254);
    assert_ne!(value, 0.39);
    assert_eq!(book.recalculate().value(1, addr("A1")).display(2), "0.39");
    assert!((number(&book, 1, "A2") - 1.0).abs() < 1e-15);
    assert_eq!(number(&book, 1, "A3"), 1e308);
    assert!(number(&book, 1, "A4") > 0.0);
    let rows = parse_delimited(&book.export_csv(1).unwrap(), ',').unwrap();
    assert_eq!(rows[0][0], value.to_string());
}

#[test]
fn convert_formulas_survive_json_and_ods_reload_and_recalculate() {
    let mut book = Workbook::default();
    put(&mut book, 1, "A1", "2.54");
    let formula = "=CONVERT(A1,\"cm\",\"in\")";
    put(&mut book, 1, "B1", formula);
    put(&mut book, 1, "C1", "=B1*0.75");
    let state = State {
        workbook: Some(book),
        ..State::default()
    };
    let json = serde_json::to_string(&state).unwrap();
    let restored_json = State::from_json(&json).unwrap();
    let ods = jig::documents::save_ods(&state).unwrap();
    let restored_ods = jig::documents::read_ods(&ods)
        .unwrap()
        .embedded_state
        .unwrap();
    for restored in [restored_json, restored_ods] {
        let mut book = restored.workbook.unwrap();
        assert_eq!(book.raw(1, addr("B1")), formula);
        assert_eq!(number(&book, 1, "B1"), 1.0);
        put(&mut book, 1, "A1", "5.08");
        assert_eq!(number(&book, 1, "B1"), 2.0);
        assert_eq!(number(&book, 1, "C1"), 1.5);
        assert_eq!(book.raw(1, addr("B1")), formula);
    }
}

#[test]
fn literals_and_numeric_display_preserve_internal_precision() {
    let mut book = Workbook::default();
    put(&mut book, 1, "A1", "'00104");
    put(&mut book, 1, "A2", "'=SUM(B1:B2)");
    put(&mut book, 1, "A3", "TRUE");
    put(&mut book, 1, "B1", "=1/3");
    put(&mut book, 1, "B2", "=A3+1");
    put(&mut book, 1, "B3", "=Z999+1");
    let values = book.recalculate();
    assert_eq!(values.value(1, addr("A1")), CellValue::Text("00104".into()));
    assert_eq!(
        values.value(1, addr("A2")),
        CellValue::Text("=SUM(B1:B2)".into())
    );
    assert_eq!(values.value(1, addr("A3")), CellValue::Bool(true));
    assert_eq!(values.value(1, addr("B1")).display(2), "0.33");
    assert_eq!(
        values.value(1, addr("B1")).csv_value(),
        (1_f64 / 3.0).to_string()
    );
    assert_eq!(number(&book, 1, "B2"), 2.0);
    assert_eq!(number(&book, 1, "B3"), 1.0);
    assert_eq!(values.value(1, addr("Z999")), CellValue::Blank);
}

#[test]
fn errors_are_local_and_propagate_without_hiding_cycles() {
    let mut book = Workbook::default();
    for (cell, raw) in [
        ("A1", "=A2+1"),
        ("A2", "=A1+1"),
        ("B1", "=1/0"),
        ("B2", "=B1+1"),
        ("C1", "=A1001"),
        ("C2", "=NO_SUCH_FUNCTION(2)"),
        ("D1", "NaN"),
        ("D2", "=\"clay\"+1"),
        ("E1", "42"),
    ] {
        put(&mut book, 1, cell, raw);
    }
    let values = book.recalculate();
    for cell in ["A1", "A2"] {
        assert_eq!(
            values.value(1, addr(cell)),
            CellValue::Error(CellError::Cycle)
        );
    }
    for cell in ["B1", "B2"] {
        assert_eq!(
            values.value(1, addr(cell)),
            CellValue::Error(CellError::DivZero)
        );
    }
    assert_eq!(
        values.value(1, addr("C1")),
        CellValue::Error(CellError::Ref)
    );
    assert_eq!(
        values.value(1, addr("C2")),
        CellValue::Error(CellError::Name)
    );
    assert_eq!(
        values.value(1, addr("D1")),
        CellValue::Error(CellError::Num)
    );
    assert_eq!(
        values.value(1, addr("D2")),
        CellValue::Error(CellError::Value)
    );
    assert_eq!(values.value(1, addr("E1")), CellValue::Number(42.0));
}

#[test]
fn if_is_lazy_comparisons_work_and_aggregates_ignore_plain_text() {
    let mut book = Workbook::default();
    for (cell, raw) in [
        ("A1", "3"),
        ("A2", "clay"),
        ("A3", "7"),
        ("B1", "=IF(A1<A3,\"ready\",1/0)"),
        ("B2", "=IF(FALSE,B2,12)"),
        ("B3", "=IF(1=1,TRUE,FALSE)"),
        ("C1", "=SUM(A1:A3)"),
        ("C2", "=COUNT(A1:A3)"),
        ("C3", "=AVERAGE(A2:A2)"),
        ("D1", "=-0=0"),
    ] {
        put(&mut book, 1, cell, raw);
    }
    let values = book.recalculate();
    assert_eq!(values.value(1, addr("B1")), CellValue::Text("ready".into()));
    assert_eq!(values.value(1, addr("B2")), CellValue::Number(12.0));
    assert_eq!(values.value(1, addr("B3")), CellValue::Bool(true));
    assert_eq!(values.value(1, addr("C1")), CellValue::Number(10.0));
    assert_eq!(values.value(1, addr("C2")), CellValue::Number(2.0));
    assert_eq!(
        values.value(1, addr("C3")),
        CellValue::Error(CellError::DivZero)
    );
    assert_eq!(values.value(1, addr("D1")), CellValue::Bool(true));
}

#[test]
fn cross_sheet_formulas_rename_and_deletion_keep_references_meaningful() {
    let mut book = Workbook::default();
    let clay = book.add_sheet("Clay tests").unwrap();
    put(&mut book, clay, "A1", "100");
    put(&mut book, clay, "A2", "94");
    put(&mut book, 1, "A1", "=SUM('Clay tests'!A1:A2)");
    assert_eq!(number(&book, 1, "A1"), 194.0);
    assert!(book.rename_sheet(clay, "Fired measurements").unwrap());
    assert!(book.raw(1, addr("A1")).contains("Fired measurements"));
    assert_eq!(number(&book, 1, "A1"), 194.0);
    assert!(!book.rename_sheet(clay, "Fired measurements").unwrap());
    let unchanged = book.clone();
    assert!(book.rename_sheet(clay, "sheet 1").is_err());
    assert_eq!(book, unchanged);
    book.remove_sheet(clay).unwrap();
    assert_eq!(
        book.recalculate().value(1, addr("A1")),
        CellValue::Error(CellError::Ref)
    );
    book.add_sheet("Fired measurements").unwrap();
    assert_eq!(
        book.recalculate().value(1, addr("A1")),
        CellValue::Error(CellError::Ref)
    );
    book.remove_sheet(2).unwrap();
    assert!(book.remove_sheet(1).is_err());
}

#[test]
fn malformed_workbooks_are_rejected_before_import_or_save() {
    let mut book = Workbook::default();
    book.sheets[0].cells.insert("a1".into(), "2".into());
    assert!(book.validate().is_err());
    book.sheets[0].cells.clear();
    book.sheets[0].cells.insert("A1".into(), "".into());
    assert!(book.validate().is_err());
    book.sheets[0].cells.clear();
    book.sheets.push(book.sheets[0].clone());
    assert!(book.validate().is_err());
}

#[test]
fn evaluation_limits_stop_long_dependency_chains() {
    let mut book = Workbook::default();
    let rows: Vec<Vec<String>> = (1..200)
        .map(|row| vec![format!("=A{}+1", row + 1)])
        .chain([vec!["1".into()]])
        .collect();
    book.set_block(1, addr("A1"), &rows).unwrap();
    assert_eq!(
        book.recalculate().value(1, addr("A1")),
        CellValue::Error(CellError::Limit)
    );
    put(&mut book, 1, "B1", "=SUM(A1:AZ1000)");
    assert!(matches!(
        book.recalculate().value(1, addr("B1")),
        CellValue::Error(_)
    ));
}

#[test]
fn exceeding_the_formula_budget_keeps_literals_and_reports_errors_instead_of_blanks() {
    let mut book = Workbook::default();
    let rows = vec![vec!["=1+2+3".to_owned(); 20]; MAX_ROWS];
    book.set_block(1, addr("A1"), &rows).unwrap();
    put(&mut book, 1, "T1000", "42");
    let values = book.recalculate();
    assert_eq!(values.value(1, addr("T1000")), CellValue::Number(42.0));
    assert_eq!(
        values.value(1, addr("T999")),
        CellValue::Error(CellError::Limit)
    );
    assert_eq!(values.value(1, addr("A1")), CellValue::Number(6.0));
}

#[test]
fn delimited_text_preserves_quoted_fields_newlines_and_trailing_empty_cells() {
    let rows = vec![
        vec!["Clay, #104".into(), "say \"hello\"".into(), "".into()],
        vec!["wet\ndry".into(), "two\tparts".into(), "94".into()],
    ];
    for delimiter in [',', '\t'] {
        let text = encode_delimited(&rows, delimiter);
        assert_eq!(parse_delimited(&text, delimiter).unwrap(), rows);
        assert_eq!(parse_delimited(&(text + "\r\n"), delimiter).unwrap(), rows);
    }
    assert_eq!(
        parse_delimited("\u{feff}a,b\r\n1,2,", ',').unwrap(),
        vec![vec!["a", "b"], vec!["1", "2", ""]]
    );
    for bad in ["\"unclosed", "a\"b,c", "\"a\"junk,b"] {
        assert!(parse_delimited(bad, ',').is_err());
    }
    assert!(parse_delimited("nul\0value", ',').is_err());
    assert!(parse_delimited("terminal\u{1b}[2J", ',').is_err());
    assert!(parse_delimited(&"a,".repeat(MAX_COLS), ',').is_err());
    assert!(parse_delimited(&"x\n".repeat(MAX_ROWS + 1), ',').is_err());
}

#[test]
fn csv_import_is_atomic_and_export_uses_full_precision_evaluated_values() {
    let mut book = Workbook::default();
    put(&mut book, 1, "D9", "old data");
    book.import_csv(
        1,
        "Start,Fired,Shrinkage\r\n100,94,=(A2-B2)/A2*100\r\n=1/3,,",
    )
    .unwrap();
    assert_eq!(book.raw(1, addr("D9")), "");
    assert_eq!(number(&book, 1, "C2"), 6.0);
    let exported = parse_delimited(&book.export_csv(1).unwrap(), ',').unwrap();
    assert_eq!(exported[1], vec!["100", "94", "6"]);
    assert_eq!(exported[2][0], (1_f64 / 3.0).to_string());
    let unchanged = book.clone();
    assert!(book.import_csv(1, "\"unterminated").is_err());
    assert_eq!(book, unchanged);
    assert!(book.import_csv(999, "a,b").is_err());
    assert_eq!(book, unchanged);
}

#[test]
fn formatting_patches_preserve_mixed_formats_and_outer_borders_use_the_selection_edges() {
    let mut book = Workbook::default();
    put(&mut book, 1, "A1", "1234.5");
    put(&mut book, 1, "B1", "0.125");
    let money = CellStyle {
        number_format: NumberFormat::Currency("USD".into()),
        fill_color: Some([20, 30, 40]),
        ..CellStyle::default()
    };
    book.set_style(1, CellRange::new(addr("A1"), addr("A1")), money)
        .unwrap();
    let percent = CellStyle {
        number_format: NumberFormat::Percent,
        font_size: 22,
        ..CellStyle::default()
    };
    book.set_style(1, CellRange::new(addr("B1"), addr("B1")), percent)
        .unwrap();
    book.patch_style(
        1,
        CellRange::new(addr("A1"), addr("B1")),
        StylePatch {
            bold: Some(true),
            ..StylePatch::default()
        },
    )
    .unwrap();
    assert!(book.style(1, addr("A1")).bold && book.style(1, addr("B1")).bold);
    assert_eq!(
        book.style(1, addr("A1")).number_format,
        NumberFormat::Currency("USD".into())
    );
    assert_eq!(book.style(1, addr("A1")).fill_color, Some([20, 30, 40]));
    assert_eq!(
        book.style(1, addr("B1")).number_format,
        NumberFormat::Percent
    );
    assert_eq!(book.style(1, addr("B1")).font_size, 22);
    book.set_borders(
        1,
        CellRange::new(addr("A1"), addr("C3")),
        BorderPreset::Outer,
    )
    .unwrap();
    assert_eq!(
        book.style(1, addr("A1")).borders,
        CellBorders {
            top: true,
            left: true,
            bottom: false,
            right: false
        }
    );
    assert_eq!(book.style(1, addr("B2")).borders, CellBorders::none());
    assert_eq!(
        book.style(1, addr("C3")).borders,
        CellBorders {
            bottom: true,
            right: true,
            top: false,
            left: false
        }
    );
    let unchanged = book.clone();
    assert!(book
        .patch_style(
            1,
            CellRange::new(addr("A1"), addr("B1")),
            StylePatch {
                font_size: Some(33),
                ..StylePatch::default()
            }
        )
        .is_err());
    assert_eq!(book, unchanged);
    book.patch_style(
        1,
        CellRange::new(addr("A1"), addr("A1")),
        StylePatch {
            fill_color: Some(None),
            font_family: Some(CellFont::Serif),
            ..StylePatch::default()
        },
    )
    .unwrap();
    assert_eq!(book.style(1, addr("A1")).fill_color, None);
    assert_eq!(book.style(1, addr("A1")).font_family, CellFont::Serif);
    assert_eq!(
        book.recalculate()
            .value(1, addr("A1"))
            .formatted(&NumberFormat::Currency("USD".into()), 2),
        "$1,234.50"
    );
    assert_eq!(
        book.recalculate()
            .value(1, addr("B1"))
            .formatted(&NumberFormat::Percent, 2),
        "12.50%"
    );
}

#[test]
fn legacy_styles_default_alignment_and_strike_without_changing_the_saved_format() {
    let legacy = serde_json::json!({
        "version": 1,
        "sheets": [{
            "id": 1,
            "name": "Sheet 1",
            "cells": {"A1": "12"},
            "styles": {"A1": {"bold": true, "underline": true, "font_size": 18}}
        }]
    });
    let book: Workbook = serde_json::from_value(legacy.clone()).unwrap();
    book.validate().unwrap();
    let style = book.style(1, addr("A1"));
    assert_eq!(style.alignment, CellAlignment::Auto);
    assert!(!style.strike);
    assert!(style.bold && style.underline);
    assert_eq!(serde_json::to_value(&book).unwrap(), legacy);
    assert_eq!(
        serde_json::to_value(CellStyle::default()).unwrap(),
        serde_json::json!({})
    );
    for (alignment, encoded) in [
        (CellAlignment::Auto, "auto"),
        (CellAlignment::Left, "left"),
        (CellAlignment::Center, "center"),
        (CellAlignment::Right, "right"),
    ] {
        assert_eq!(serde_json::to_value(alignment).unwrap(), encoded);
        assert_eq!(
            serde_json::from_value::<CellAlignment>(serde_json::json!(encoded)).unwrap(),
            alignment
        );
    }
    assert!(serde_json::from_value::<CellStyle>(serde_json::json!({
        "alignment": "diagonal"
    }))
    .is_err());
}

#[test]
fn alignment_and_strike_survive_save_paste_and_structural_edits() {
    let mut book = Workbook::default();
    let source = CellRange::new(addr("B2"), addr("B2"));
    let style = CellStyle {
        alignment: CellAlignment::Center,
        strike: true,
        underline: true,
        fill_color: Some([20, 30, 40]),
        ..CellStyle::default()
    };
    book.set_style(1, source, style.clone()).unwrap();
    put(&mut book, 1, "B2", "original");
    book.patch_style(
        1,
        source,
        StylePatch {
            bold: Some(true),
            ..StylePatch::default()
        },
    )
    .unwrap();
    let expected = CellStyle {
        bold: true,
        ..style
    };
    let encoded = serde_json::to_value(&book).unwrap();
    assert_eq!(encoded["sheets"][0]["styles"]["B2"]["alignment"], "center");
    assert_eq!(encoded["sheets"][0]["styles"]["B2"]["strike"], true);
    let mut book: Workbook = serde_json::from_value(encoded).unwrap();
    book.insert_rows(1, 1, 2).unwrap();
    book.insert_columns(1, 1, 2).unwrap();
    assert_eq!(book.style(1, addr("D4")), expected);
    book.delete_rows(1, 0, 1).unwrap();
    book.delete_columns(1, 0, 1).unwrap();
    assert_eq!(book.style(1, addr("C3")), expected);
    assert_eq!(book.raw(1, addr("C3")), "original");
    book.set_block(1, addr("C3"), &[vec!["pasted".into(), "neighbor".into()]])
        .unwrap();
    assert_eq!(book.raw(1, addr("C3")), "pasted");
    assert_eq!(book.style(1, addr("C3")), expected);
    let destination = CellRange::new(addr("C3"), addr("C3"));
    book.patch_style(
        1,
        destination,
        StylePatch {
            alignment: Some(CellAlignment::Right),
            strike: Some(false),
            ..StylePatch::default()
        },
    )
    .unwrap();
    assert_eq!(
        book.style(1, addr("C3")),
        CellStyle {
            alignment: CellAlignment::Right,
            strike: false,
            ..expected
        }
    );
    book.patch_style(
        1,
        destination,
        StylePatch {
            alignment: Some(CellAlignment::Auto),
            ..StylePatch::default()
        },
    )
    .unwrap();
    let saved = serde_json::to_value(book.style(1, addr("C3"))).unwrap();
    assert!(saved.get("alignment").is_none());
    assert!(saved.get("strike").is_none());
}

#[test]
fn border_presets_replace_all_sides_with_selection_relative_masks() {
    let span = CellRange::new(addr("B2"), addr("D4"));
    for preset in [
        BorderPreset::None,
        BorderPreset::All,
        BorderPreset::Outer,
        BorderPreset::Inner,
        BorderPreset::Horizontal,
        BorderPreset::Vertical,
    ] {
        let mut book = Workbook::default();
        book.patch_style(
            1,
            span,
            StylePatch {
                bold: Some(true),
                borders: Some(CellBorders::all()),
                ..StylePatch::default()
            },
        )
        .unwrap();
        book.set_borders(1, span, preset).unwrap();
        for row in span.start.row..=span.end.row {
            for col in span.start.col..=span.end.col {
                let expected = match preset {
                    BorderPreset::None => CellBorders::none(),
                    BorderPreset::All => CellBorders::all(),
                    BorderPreset::Outer => CellBorders {
                        top: row == span.start.row,
                        bottom: row == span.end.row,
                        left: col == span.start.col,
                        right: col == span.end.col,
                    },
                    BorderPreset::Inner => CellBorders {
                        top: row > span.start.row,
                        bottom: row < span.end.row,
                        left: col > span.start.col,
                        right: col < span.end.col,
                    },
                    BorderPreset::Horizontal => CellBorders {
                        top: true,
                        bottom: true,
                        ..CellBorders::none()
                    },
                    BorderPreset::Vertical => CellBorders {
                        left: true,
                        right: true,
                        ..CellBorders::none()
                    },
                    _ => unreachable!(),
                };
                let style = book.style(1, CellAddr::new(row, col));
                assert_eq!(style.borders, expected, "{preset:?} at {row},{col}");
                assert!(style.bold);
            }
        }
    }
    let mut single = Workbook::default();
    let cell = CellRange::new(addr("A1"), addr("A1"));
    single.set_borders(1, cell, BorderPreset::Inner).unwrap();
    assert_eq!(single.style(1, addr("A1")).borders, CellBorders::none());
    single
        .set_borders(1, cell, BorderPreset::Horizontal)
        .unwrap();
    assert_eq!(
        single.style(1, addr("A1")).borders,
        CellBorders {
            top: true,
            bottom: true,
            ..CellBorders::none()
        }
    );
}

#[test]
fn individual_border_edges_compose_without_changing_other_cell_edges() {
    let mut book = Workbook::default();
    let span = CellRange::new(addr("B2"), addr("D4"));
    let center = CellRange::new(addr("C3"), addr("C3"));
    book.set_borders(1, center, BorderPreset::All).unwrap();
    for preset in [
        BorderPreset::Top,
        BorderPreset::Right,
        BorderPreset::Bottom,
        BorderPreset::Left,
    ] {
        assert!(book.set_borders(1, span, preset).unwrap());
        assert!(!book.set_borders(1, span, preset).unwrap());
        assert_eq!(book.style(1, addr("C3")).borders, CellBorders::all());
    }
    for row in span.start.row..=span.end.row {
        for col in span.start.col..=span.end.col {
            let cell = CellAddr::new(row, col);
            let expected = if cell == addr("C3") {
                CellBorders::all()
            } else {
                CellBorders {
                    top: row == span.start.row,
                    bottom: row == span.end.row,
                    left: col == span.start.col,
                    right: col == span.end.col,
                }
            };
            assert_eq!(book.style(1, cell).borders, expected);
        }
    }
    assert_eq!(book.style(1, addr("A1")), CellStyle::default());
}

#[test]
fn border_presets_treat_merged_cells_as_a_single_perimeter() {
    let merged = CellRange::new(addr("B2"), addr("C3"));
    let span = CellRange::new(addr("A1"), addr("D4"));
    for (preset, expected) in [
        (BorderPreset::None, CellBorders::none()),
        (BorderPreset::All, CellBorders::all()),
        (BorderPreset::Outer, CellBorders::none()),
        (BorderPreset::Inner, CellBorders::all()),
        (
            BorderPreset::Horizontal,
            CellBorders {
                top: true,
                bottom: true,
                ..CellBorders::none()
            },
        ),
        (
            BorderPreset::Vertical,
            CellBorders {
                left: true,
                right: true,
                ..CellBorders::none()
            },
        ),
    ] {
        let mut book = Workbook::default();
        book.merge(1, merged).unwrap();
        book.set_borders(1, span, preset).unwrap();
        assert_eq!(
            book.rendered_style(1, addr("B2")).borders,
            expected,
            "{preset:?}"
        );
        assert_eq!(
            book.rendered_style(1, addr("C3")).borders,
            expected,
            "{preset:?}"
        );
        assert!(!book.style(1, addr("B2")).borders.bottom);
        assert!(!book.style(1, addr("B2")).borders.right);
        assert!(!book.style(1, addr("C3")).borders.top);
        assert!(!book.style(1, addr("C3")).borders.left);
    }
    let mut book = Workbook::default();
    book.merge(1, merged).unwrap();
    book.set_borders(1, merged, BorderPreset::Inner).unwrap();
    assert_eq!(
        book.rendered_style(1, merged.start).borders,
        CellBorders::none()
    );
    for preset in [
        BorderPreset::Top,
        BorderPreset::Bottom,
        BorderPreset::Left,
        BorderPreset::Right,
    ] {
        book.set_borders(1, merged, preset).unwrap();
    }
    assert_eq!(
        book.rendered_style(1, merged.start).borders,
        CellBorders::all()
    );
    assert!(!book.style(1, addr("B2")).borders.bottom);
    assert!(!book.style(1, addr("C3")).borders.top);
}

#[test]
fn rendered_merged_borders_ignore_interior_edges_saved_before_merging() {
    let mut book = Workbook::default();
    let anchor = CellRange::new(addr("A1"), addr("A1"));
    book.patch_style(
        1,
        anchor,
        StylePatch {
            borders: Some(CellBorders {
                bottom: true,
                right: true,
                ..CellBorders::none()
            }),
            alignment: Some(CellAlignment::Left),
            strike: Some(true),
            ..StylePatch::default()
        },
    )
    .unwrap();
    let merged = CellRange::new(addr("A1"), addr("B2"));
    book.merge(1, merged).unwrap();
    let rendered = book.rendered_style(1, addr("B2"));
    assert_eq!(rendered.borders, CellBorders::none());
    assert_eq!(rendered.alignment, CellAlignment::Left);
    assert!(rendered.strike);
    book.unmerge(1, merged).unwrap();
    assert!(book.style(1, addr("A1")).borders.bottom);
    assert!(book.style(1, addr("A1")).borders.right);
}

#[test]
fn merge_never_hides_data_and_unmerge_restores_independent_cells() {
    let mut book = Workbook::default();
    put(&mut book, 1, "A1", "Financial summary");
    put(&mut book, 1, "B1", "Keep this value");
    let span = CellRange::new(addr("A1"), addr("C2"));
    let unchanged = book.clone();
    assert!(book.merge(1, span).is_err());
    assert_eq!(book, unchanged);
    book.set_cell(1, addr("B1"), "").unwrap();
    book.set_borders(1, span, BorderPreset::Outer).unwrap();
    book.merge(1, span).unwrap();
    assert_eq!(book.merge_at(1, addr("C2")), Some(span));
    assert_eq!(
        book.rendered_style(1, addr("A1")).borders,
        CellBorders::all()
    );
    assert_eq!(
        book.rendered_style(1, addr("C2")).borders,
        CellBorders::all()
    );
    assert!(!book.merge(1, span).unwrap());
    let merged = book.clone();
    assert!(book.set_cell(1, addr("B1"), "hidden data").is_err());
    assert_eq!(book, merged);
    assert!(book
        .merge(1, CellRange::new(addr("C2"), addr("D3")))
        .is_err());
    assert_eq!(book, merged);
    let encoded = serde_json::to_string(&book).unwrap();
    let restored: Workbook = serde_json::from_str(&encoded).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored, book);
    book.unmerge(1, CellRange::new(addr("B2"), addr("B2")))
        .unwrap();
    put(&mut book, 1, "B1", "Independent");
    assert!(book.sheet(1).unwrap().merges.is_empty());
    assert_eq!(book.raw(1, addr("A1")), "Financial summary");
}

#[test]
fn row_edits_move_cells_styles_merges_chart_sources_and_every_sheet_reference_together() {
    let mut book = Workbook::default();
    let other = book.add_sheet("Report").unwrap();
    for (cell, raw) in [
        ("A1", "10"),
        ("A2", "20"),
        ("A3", "30"),
        ("B1", "=SUM(A1:A3)"),
    ] {
        put(&mut book, 1, cell, raw);
    }
    put(&mut book, other, "A1", "='Sheet 1'!$A$2+1");
    book.set_style(
        1,
        CellRange::new(addr("A2"), addr("A2")),
        CellStyle {
            number_format: NumberFormat::Currency("USD".into()),
            ..CellStyle::default()
        },
    )
    .unwrap();
    book.merge(1, CellRange::new(addr("D1"), addr("E2")))
        .unwrap();
    let chart = book
        .add_chart(
            ChartKind::Bar,
            ChartSource::Range {
                sheet: 1,
                range: CellRange::new(addr("A1"), addr("B3")),
            },
            "Amounts",
        )
        .unwrap();
    book.insert_rows(1, 1, 1).unwrap();
    assert_eq!(book.raw(1, addr("A2")), "");
    assert_eq!(book.raw(1, addr("A3")), "20");
    assert_eq!(book.raw(1, addr("B1")), "=SUM(A1:A4)");
    assert_eq!(book.raw(other, addr("A1")), "='Sheet 1'!$A$3+1");
    assert_eq!(number(&book, other, "A1"), 21.0);
    assert_eq!(
        book.style(1, addr("A3")).number_format,
        NumberFormat::Currency("USD".into())
    );
    assert_eq!(
        book.merge_at(1, addr("E3")),
        Some(CellRange::new(addr("D1"), addr("E3")))
    );
    assert_eq!(
        book.charts[0].source,
        ChartSource::Range {
            sheet: 1,
            range: CellRange::new(addr("A1"), addr("B4"))
        }
    );
    book.delete_rows(1, 2, 1).unwrap();
    assert_eq!(book.raw(1, addr("A3")), "30");
    assert_eq!(book.raw(1, addr("B1")), "=SUM(A1:A3)");
    assert_eq!(
        book.recalculate().value(other, addr("A1")),
        CellValue::Error(CellError::Ref)
    );
    assert_eq!(book.style(1, addr("A3")), CellStyle::default());
    assert_eq!(
        book.merge_at(1, addr("E2")),
        Some(CellRange::new(addr("D1"), addr("E2")))
    );
    assert_eq!(book.charts[0].id, chart);
    assert_eq!(
        book.charts[0].source,
        ChartSource::Range {
            sheet: 1,
            range: CellRange::new(addr("A1"), addr("B3"))
        }
    );
    assert_eq!(number(&book, 1, "B1"), 40.0);
}

#[test]
fn column_deletion_breaks_deleted_references_and_insertion_refuses_to_discard_edge_data() {
    let mut book = Workbook::default();
    let report = book.add_sheet("Report").unwrap();
    put(&mut book, 1, "C3", "8");
    put(&mut book, 1, "B1", "=C3");
    put(&mut book, report, "A1", "=SUM('Sheet 1'!C3:C4)");
    book.insert_columns(1, 1, 1).unwrap();
    assert_eq!(book.raw(1, addr("D3")), "8");
    assert_eq!(book.raw(1, addr("C1")), "=D3");
    assert_eq!(number(&book, report, "A1"), 8.0);
    book.delete_columns(1, 3, 1).unwrap();
    assert_eq!(
        book.recalculate().value(1, addr("C1")),
        CellValue::Error(CellError::Ref)
    );
    assert_eq!(
        book.recalculate().value(report, addr("A1")),
        CellValue::Error(CellError::Ref)
    );
    put(&mut book, 1, "AZ1000", "Keep edge data");
    let unchanged = book.clone();
    assert!(book.insert_rows(1, 0, 1).is_err());
    assert_eq!(book, unchanged);
    assert!(book.insert_columns(1, 0, 1).is_err());
    assert_eq!(book, unchanged);
    assert!(book.delete_columns(1, MAX_COLS - 1, 2).is_err());
    assert_eq!(book, unchanged);
}

#[test]
fn dates_are_typed_recalculate_deterministically_and_support_basic_calendar_arithmetic() {
    let mut book = Workbook::default();
    let date = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
    for (cell, raw) in [
        ("A1", "2026-11-11"),
        ("B1", "=A1+20"),
        ("B2", "=DATE(YEAR(A1),MONTH(A1)+1,0)"),
        ("B3", "=DAY(A1)+MONTH(A1)+YEAR(A1)"),
        ("C1", "=TODAY()"),
        ("C2", "=B1-A1"),
        ("C3", "=DATE(2024,2,29)"),
        ("C4", "=DATE(2026,13,1)"),
        ("D1", "=IF(TRUE(),1,FALSE())"),
    ] {
        put(&mut book, 1, cell, raw);
    }
    let values = book.recalculate_at(date);
    assert_eq!(values.value(1, addr("A1")), CellValue::Date(date));
    assert_eq!(values.value(1, addr("B1")).csv_value(), "2026-12-01");
    assert_eq!(values.value(1, addr("B2")).csv_value(), "2026-11-30");
    assert_eq!(values.value(1, addr("B3")), CellValue::Number(2048.0));
    assert_eq!(values.value(1, addr("C1")), CellValue::Date(date));
    assert_eq!(values.value(1, addr("C2")), CellValue::Number(20.0));
    assert_eq!(values.value(1, addr("C3")).csv_value(), "2024-02-29");
    assert_eq!(values.value(1, addr("C4")).csv_value(), "2027-01-01");
    assert_eq!(values.value(1, addr("D1")), CellValue::Number(1.0));
    assert_eq!(serial_to_date(date_to_serial(date)), Some(date));
    assert_eq!(
        CellValue::Number(date_to_serial(date)).formatted(&NumberFormat::Date, 2),
        "2026-11-11"
    );
    assert_eq!(
        values
            .value(1, addr("A1"))
            .formatted(&NumberFormat::Number, 0),
        date_to_serial(date).to_string()
    );
    let exported = parse_delimited(&book.export_csv_at(1, date).unwrap(), ',').unwrap();
    assert_eq!(exported[0][2], "2026-11-11");
}

#[test]
fn financial_amount_literals_aggregate_without_losing_the_raw_statement_text() {
    let mut book = Workbook::default();
    for (cell, raw) in [
        ("A1", "$1,234.50"),
        ("A2", "(25.00)"),
        ("A3", "EUR 20"),
        ("A4", "50%"),
        ("A5", "12,34"),
        ("A6", "-$-25"),
        ("A7", "$25%"),
        ("B1", "=SUM(A1:A4)"),
    ] {
        put(&mut book, 1, cell, raw);
    }
    assert_eq!(number(&book, 1, "A1"), 1234.5);
    assert_eq!(number(&book, 1, "A2"), -25.0);
    assert_eq!(number(&book, 1, "B1"), 1230.0);
    assert_eq!(book.raw(1, addr("A1")), "$1,234.50");
    assert_eq!(
        book.recalculate().value(1, addr("A5")),
        CellValue::Text("12,34".into())
    );
    assert_eq!(
        book.recalculate().value(1, addr("A6")),
        CellValue::Text("-$-25".into())
    );
    assert_eq!(
        book.recalculate().value(1, addr("A7")),
        CellValue::Text("$25%".into())
    );
    assert_eq!(
        CellValue::Number(-1234.5).formatted(&NumberFormat::Currency("USD".into()), 2),
        "-$1,234.50"
    );
}

#[test]
fn chart_lifecycle_and_format_metadata_stay_portable_and_validate_before_assignment() {
    let mut book = Workbook::default();
    let source = ChartSource::Range {
        sheet: 1,
        range: CellRange::new(addr("A1"), addr("B3")),
    };
    let chart = book.add_chart(ChartKind::Pie, source, "Costs").unwrap();
    book.rename_chart(chart, "Studio costs").unwrap();
    let unchanged = book.clone();
    assert!(book
        .add_chart(ChartKind::Pie, ChartSource::Tasks, "Invalid task pie")
        .is_err());
    assert_eq!(book, unchanged);
    book.add_chart(ChartKind::Gantt, ChartSource::Tasks, "Work")
        .unwrap();
    book.add_sheet("Keep workbook").unwrap();
    book.remove_sheet(1).unwrap();
    assert_eq!(book.charts.len(), 1);
    assert_eq!(book.charts[0].source, ChartSource::Tasks);
    let encoded = serde_json::to_string(&book).unwrap();
    let decoded: Workbook = serde_json::from_str(&encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, book);
    let legacy = r#"{"version":1,"sheets":[{"id":1,"name":"Old","cells":{"A1":"12"}}]}"#;
    let old: Workbook = serde_json::from_str(legacy).unwrap();
    old.validate().unwrap();
    assert!(
        old.charts.is_empty() && old.sheets[0].styles.is_empty() && old.sheets[0].merges.is_empty()
    );
    assert_eq!(number(&old, 1, "A1"), 12.0);
}

#[test]
fn workbook_edits_preserve_legacy_state_tasks_deadlines_history_and_hidden_entries() {
    let mut state = State::default();
    let task = state.add_task("Photograph sculpture").unwrap();
    state
        .set_task_schedule(
            task,
            Some("2026-11-01T00:00:00-04:00"),
            Some("2026-11-15T18:00:00-05:00"),
        )
        .unwrap();
    state.feature_task(task).unwrap();
    state.calculate("125*1.15").unwrap();
    state
        .set_entry(None, "Hidden studio note", "2026-11-11")
        .unwrap();
    let original = serde_json::to_value(&state).unwrap();
    assert!(original.get("workbook").is_none());
    let mut restored = State::from_json(&original.to_string()).unwrap();
    assert!(restored.workbook.is_none());
    let mut book = Workbook::default();
    put(&mut book, 1, "A1", "Income");
    put(&mut book, 1, "B1", "$1,250.00");
    book.set_style(
        1,
        CellRange::new(addr("B1"), addr("B1")),
        CellStyle {
            number_format: NumberFormat::Currency("USD".into()),
            ..CellStyle::default()
        },
    )
    .unwrap();
    book.insert_rows(1, 0, 1).unwrap();
    book.add_chart(ChartKind::Gantt, ChartSource::Tasks, "Studio dates")
        .unwrap();
    restored.workbook = Some(book);
    restored.validate().unwrap();
    let encoded = serde_json::to_value(&restored).unwrap();
    for field in [
        "version",
        "preferences",
        "tasks",
        "deadlines",
        "history",
        "entries",
        "featured",
    ] {
        assert_eq!(encoded[field], original[field], "{field}");
    }
    let reloaded = State::from_json(&encoded.to_string()).unwrap();
    assert_eq!(number(reloaded.workbook.as_ref().unwrap(), 1, "B2"), 1250.0);
    assert_eq!(reloaded.featured, state.featured);
    let mut invalid = encoded.clone();
    invalid["workbook"]["sheets"][0]["styles"]["B2"]["font_size"] = 255.into();
    assert!(State::from_json(&invalid.to_string()).is_err());
    assert_eq!(serde_json::to_value(&restored).unwrap(), encoded);
}
