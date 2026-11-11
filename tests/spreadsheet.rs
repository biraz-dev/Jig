use chrono::NaiveDate;
use jig::charts::{ChartKind, ChartSource};
use jig::model::State;
use jig::spreadsheet::{
    date_to_serial, encode_delimited, parse_delimited, serial_to_date, BorderPreset, CellAddr,
    CellBorders, CellError, CellFont, CellRange, CellStyle, CellValue, NumberFormat, StylePatch,
    Workbook, MAX_CELL_BYTES, MAX_COLS, MAX_ROWS,
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
