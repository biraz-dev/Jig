use chrono::NaiveDate;
use jig::{
    charts::{ChartKind, ChartSource},
    documents::{export_pdf, read_ods, save_ods},
    model::State,
    spreadsheet::{
        CellAddr, CellBorders, CellFont, CellRange, CellStyle, CellValue, NumberFormat, Workbook,
    },
};
use std::io::{Cursor, Read, Write};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

fn addr(value: &str) -> CellAddr {
    CellAddr::parse(value).unwrap()
}
fn range(start: &str, end: &str) -> CellRange {
    CellRange::new(addr(start), addr(end))
}

fn fixture() -> State {
    let mut state = State::default();
    let task = state.add_task("Make a sculpture").unwrap();
    state
        .set_task_schedule(
            task,
            Some("2026-11-11T00:00:00-05:00"),
            Some("2026-11-18T17:00:00-05:00"),
        )
        .unwrap();
    state.feature_task(task).unwrap();
    let mut book = Workbook::default();
    book.set_block(
        1,
        addr("A1"),
        &[
            vec!["Item".into(), "Cost".into(), "Doubled".into()],
            vec![
                "Résumé € — studio".into(),
                "123.456789".into(),
                "=B2*2".into(),
            ],
            vec!["Clay".into(), "18".into(), "=SUM(B2:B3)".into()],
            vec!["'=literal text".into(), "'0012".into(), "2026-11-11".into()],
        ],
    )
    .unwrap();
    let style = CellStyle {
        font_family: CellFont::Serif,
        font_size: 17,
        bold: true,
        italic: true,
        underline: true,
        text_color: Some([20, 45, 90]),
        fill_color: Some([225, 235, 248]),
        borders: CellBorders::all(),
        number_format: NumberFormat::Currency("EUR".into()),
    };
    book.set_style(1, range("B2", "B2"), style).unwrap();
    book.set_cell(1, addr("A6"), "Merged heading").unwrap();
    book.merge(1, range("A6", "C7")).unwrap();
    book.set_cell(1, addr("A8"), "46337").unwrap();
    book.set_style(
        1,
        range("A8", "A8"),
        CellStyle {
            number_format: NumberFormat::Date,
            ..CellStyle::default()
        },
    )
    .unwrap();
    let second = book.add_sheet("Clay tests").unwrap();
    book.set_cell(second, addr("A1"), "='Sheet 1'!B2+1")
        .unwrap();
    book.add_chart(
        ChartKind::Bar,
        ChartSource::Range {
            sheet: 1,
            range: range("A1", "B3"),
        },
        "Studio costs",
    )
    .unwrap();
    state.workbook = Some(book);
    state.validate().unwrap();
    state
}

fn entry(bytes: &[u8], name: &str) -> String {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name(name)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

fn package(content: &str, styles: Option<&str>) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "mimetype",
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
    )
    .unwrap();
    zip.write_all(b"application/vnd.oasis.opendocument.spreadsheet")
        .unwrap();
    zip.start_file(
        "content.xml",
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
    )
    .unwrap();
    zip.write_all(content.as_bytes()).unwrap();
    if let Some(styles) = styles {
        zip.start_file(
            "styles.xml",
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(styles.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn ods_is_standard_zip_with_formulas_styles_merges_and_exact_jig_roundtrip() {
    let state = fixture();
    let bytes = save_ods(&state).unwrap();
    let mut zip = ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let first = zip.by_index(0).unwrap();
    assert_eq!(first.name(), "mimetype");
    assert_eq!(first.compression(), CompressionMethod::Stored);
    drop(first);
    assert!(zip.by_name("Pictures/chart-1.svg").is_ok());
    let xml = entry(&bytes, "content.xml");
    assert!(xml.contains("table:formula=\"of:="));
    assert!(xml.contains("[.B2]"));
    assert!(xml.contains("office:currency=\"EUR\""));
    assert!(xml.contains("table:number-columns-spanned=\"3\""));
    assert!(xml.contains("table:number-rows-spanned=\"2\""));
    assert!(xml.contains("fo:font-weight=\"bold\""));
    assert!(xml.contains("fo:font-style=\"italic\""));
    assert!(xml.contains("office:value-type=\"date\" office:date-value=\"2026-11-11\""));
    assert!(xml.find("<table:shapes>").unwrap() < xml.find("<table:table-column ").unwrap());
    let imported = read_ods(&bytes).unwrap();
    assert_eq!(
        serde_json::to_value(imported.embedded_state.unwrap()).unwrap(),
        serde_json::to_value(state).unwrap()
    );
    std::fs::write("/tmp/jig-document-review.ods", &bytes).unwrap();
}

#[test]
fn external_ods_imports_typed_cells_openformula_and_repeated_empty_space() {
    let content = r##"<?xml version="1.0"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:number="urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0"><office:automatic-styles><number:currency-style style:name="eur"><number:currency-symbol>EUR </number:currency-symbol></number:currency-style><style:style style:name="money" style:family="table-cell" style:data-style-name="eur"><style:text-properties fo:font-weight="bold" fo:color="#143255"/><style:table-cell-properties fo:background-color="#e1e2e3" fo:border="0.5pt solid #000000"/></style:style></office:automatic-styles><office:body><office:spreadsheet><table:table table:name="Imported"><table:table-row><table:table-cell office:value-type="string"><text:p>Text<text:s text:c="2"/>€<text:tab/>one</text:p><text:p>two</text:p></table:table-cell><table:table-cell office:value-type="currency" office:currency="EUR" office:value="12.125" table:style-name="money"><text:p>€12.13</text:p></table:table-cell><table:table-cell office:value-type="date" office:date-value="2026-11-11"/><table:table-cell office:value-type="string"><text:p>=not a formula</text:p></table:table-cell></table:table-row><table:table-row table:number-rows-repeated="2"><table:table-cell/><table:table-cell office:value-type="float" office:value="3.5"/></table:table-row><table:table-row><table:table-cell table:formula="of:=SUM([.B2:.B3])" office:value-type="float" office:value="7"/></table:table-row><table:table-row table:number-rows-repeated="1048572"><table:table-cell table:number-columns-repeated="16384"/></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"##;
    let imported = read_ods(&package(content, None)).unwrap();
    assert!(imported.embedded_state.is_none());
    let book = imported.workbook;
    let values = book.recalculate();
    assert_eq!(book.raw(1, addr("A1")), "Text  €\tone\ntwo");
    assert_eq!(values.value(1, addr("B1")), CellValue::Number(12.125));
    assert_eq!(
        book.style(1, addr("B1")).number_format,
        NumberFormat::Currency("EUR".into())
    );
    assert!(book.style(1, addr("B1")).bold);
    assert!(book.style(1, addr("B1")).borders.left);
    assert_eq!(
        values.value(1, addr("C1")),
        CellValue::Date(NaiveDate::from_ymd_opt(2026, 11, 11).unwrap())
    );
    assert_eq!(
        values.value(1, addr("D1")),
        CellValue::Text("=not a formula".into())
    );
    assert_eq!(book.raw(1, addr("A4")), "=SUM(B2:B3)");
    assert_eq!(values.value(1, addr("A4")), CellValue::Number(7.0));
}

#[test]
fn externally_edited_standard_cells_override_old_metadata_without_losing_tasks() {
    let state = fixture();
    let original = save_ods(&state).unwrap();
    let mut source = ZipArchive::new(Cursor::new(original)).unwrap();
    let mut rewritten = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..source.len() {
        let mut file = source.by_index(index).unwrap();
        let name = file.name().to_owned();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        if name == "content.xml" {
            let text = String::from_utf8(bytes).unwrap();
            bytes = text
                .replace("office:value=\"123.456789\"", "office:value=\"200.25\"")
                .into_bytes();
        }
        rewritten
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(if index == 0 {
                    CompressionMethod::Stored
                } else {
                    CompressionMethod::Deflated
                }),
            )
            .unwrap();
        rewritten.write_all(&bytes).unwrap();
    }
    let bytes = rewritten.finish().unwrap().into_inner();
    let imported = read_ods(&bytes).unwrap();
    assert_eq!(
        imported.workbook.recalculate().value(1, addr("B2")),
        CellValue::Number(200.25)
    );
    let saved = imported.embedded_state.unwrap();
    assert_eq!(saved.tasks[0].id, state.tasks[0].id);
    assert_eq!(saved.featured, state.featured);
    assert_eq!(saved.deadlines[0].at, state.deadlines[0].at);
    assert_eq!(
        saved.workbook.unwrap().recalculate().value(1, addr("C2")),
        CellValue::Number(400.5)
    );
}

#[test]
fn malformed_or_oversized_ods_is_rejected_without_allocating_repeats() {
    assert!(read_ods(b"not a zip file").is_err());
    let malformed = package("<office:document-content>", None);
    assert!(read_ods(&malformed).is_err());
    let dtd = package(
        "<!DOCTYPE x [<!ENTITY test SYSTEM 'https://example.com/x'>]><x/>",
        None,
    );
    assert!(read_ods(&dtd).is_err());
    let too_many = r##"<d xmlns:table="t" xmlns:office="o"><table:table table:name="Sheet"><table:table-row table:number-rows-repeated="1001"><table:table-cell office:value-type="float" office:value="1"/></table:table-row></table:table></d>"##;
    assert!(read_ods(&package(too_many, None))
        .unwrap_err()
        .contains("grid"));
    let nested =
        r#"<d xmlns:table="t"><table:table table:name="Sheet"><table:table/></table:table></d>"#;
    assert!(read_ods(&package(nested, None))
        .unwrap_err()
        .contains("Nested"));
    let excessive_text = r#"<d xmlns:table="t" xmlns:text="x"><table:table table:name="Sheet"><table:table-row><table:table-cell><text:p><text:s text:c="3000"/><text:s text:c="3000"/></text:p></table:table-cell></table:table-row></table:table></d>"#;
    assert!(read_ods(&package(excessive_text, None))
        .unwrap_err()
        .contains("text limit"));
}

#[test]
fn external_ods_preserves_empty_text_paragraphs() {
    let content = r#"<d xmlns:table="t" xmlns:text="x" xmlns:office="o"><table:table table:name="Sheet"><table:table-row><table:table-cell office:value-type="string"><text:p/><text:p>A &amp; B &lt; C &gt; D &quot;quoted&quot; &#8364;</text:p><text:p/></table:table-cell></table:table-row></table:table></d>"#;
    let imported = read_ods(&package(content, None)).unwrap();
    assert_eq!(
        imported.workbook.raw(1, addr("A1")),
        "\nA & B < C > D \"quoted\" €\n"
    );
}

#[test]
fn pdf_contains_searchable_unicode_formatted_values_grid_and_vector_chart_pages() {
    let state = fixture();
    let bytes = export_pdf(&state, 1, None).unwrap();
    assert!(bytes.starts_with(b"%PDF-1.7"));
    let pdf = lopdf::Document::load_mem(&bytes).unwrap();
    let pages = pdf.get_pages();
    assert_eq!(pages.len(), 2, "The sheet and chart each need a real page");
    let text = pdf
        .extract_text(&pages.keys().copied().collect::<Vec<_>>())
        .unwrap();
    assert!(text.contains("Résumé € — studio"), "{text}");
    assert!(text.contains("246.91"));
    assert!(text.contains("Studio costs"));
    assert!(text.contains("Merged heading"));
    for id in pages.values() {
        let content = pdf.get_page_content(*id).unwrap();
        let ops = lopdf::content::Content::decode(&content).unwrap();
        assert!(ops.operations.iter().any(|op| op.operator == "Tj"));
        assert!(ops
            .operations
            .iter()
            .any(|op| op.operator == "re" || op.operator == "l"));
    }
    std::fs::write("/tmp/jig-document-review.pdf", &bytes).unwrap();
}

#[test]
fn pdf_print_range_and_pagination_preserve_later_rows() {
    let mut state = State::default();
    let mut book = Workbook::default();
    for row in 0..70 {
        book.set_cell(1, CellAddr::new(row, 0), &format!("Record {}", row + 1))
            .unwrap();
    }
    state.workbook = Some(book);
    let bytes = export_pdf(&state, 1, None).unwrap();
    let pdf = lopdf::Document::load_mem(&bytes).unwrap();
    assert!(pdf.get_pages().len() >= 3);
    let text = pdf
        .extract_text(&pdf.get_pages().keys().copied().collect::<Vec<_>>())
        .unwrap();
    assert!(text.contains("Record 70"));
    let selected = export_pdf(&state, 1, Some(range("A2", "A3"))).unwrap();
    let pdf = lopdf::Document::load_mem(&selected).unwrap();
    let text = pdf.extract_text(&[1]).unwrap();
    assert!(text.contains("Record 2") && text.contains("Record 3"));
    assert!(!text.contains("Record 1\n"));
}

#[test]
fn pdf_never_prints_a_clipped_financial_amount() {
    let mut state = State::default();
    let mut book = Workbook::default();
    book.set_cell(1, addr("A1"), "1234567890123.45").unwrap();
    book.set_style(
        1,
        range("A1", "A1"),
        CellStyle {
            number_format: NumberFormat::Currency("USD".into()),
            ..CellStyle::default()
        },
    )
    .unwrap();
    state.workbook = Some(book);
    let pdf = lopdf::Document::load_mem(&export_pdf(&state, 1, None).unwrap()).unwrap();
    let text = pdf.extract_text(&[1]).unwrap();
    assert!(text.contains("####"));
    assert!(!text.contains("1234567890123"));
}
