use jig::pdf_import::{self, DateOrder, DecimalMark, ExtractedPdf};
use lopdf::{
    content::{Content, Operation},
    dictionary, Document, Object, Stream,
};
use std::io::Write;

fn document(lines: &[&str], pages: usize, oversized_stream: bool) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let page_tree = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding",
    });
    let mut operations = vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 12.into()]),
        Operation::new("Td", vec![40.into(), 740.into()]),
    ];
    for line in lines {
        operations.push(Operation::new("Tj", vec![Object::string_literal(*line)]));
        operations.push(Operation::new("Td", vec![0.into(), (-18).into()]));
    }
    operations.push(Operation::new("ET", vec![]));
    let content = if oversized_stream {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&vec![b' '; 4 * 1024 * 1024 + 1]).unwrap();
        doc.add_object(Stream::new(
            dictionary! {"Filter"=>"FlateDecode"},
            encoder.finish().unwrap(),
        ))
    } else {
        doc.add_object(Stream::new(
            dictionary! {},
            Content { operations }.encode().unwrap(),
        ))
    };
    let mut children = Vec::new();
    for _ in 0..pages {
        children.push(Object::Reference(doc.add_object(dictionary! {
            "Type"=>"Page", "Parent"=>page_tree, "MediaBox"=>vec![0.into(),0.into(),612.into(),792.into()],
            "Resources"=>dictionary! { "Font"=>dictionary! {"F1"=>font}}, "Contents"=>content,
        })));
    }
    doc.objects.insert(
        page_tree,
        Object::Dictionary(dictionary! {"Type"=>"Pages", "Kids"=>children, "Count"=>pages as i64}),
    );
    let root = doc.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>page_tree});
    doc.trailer.set("Root", root);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn extracts_real_pdf_text_and_keeps_empty_pages_for_ocr() {
    let bytes = document(
        &[
            "Bank statement",
            "11/11/2026  Clay supplier  (123.45)  4,500.00",
        ],
        2,
        false,
    );
    let mut imported = pdf_import::extract(&bytes).unwrap();
    assert_eq!(imported.pages.len(), 2);
    assert!(imported.has_text());
    assert!(imported.pages[0].text.contains("Clay supplier"));
    assert!(imported.pages[0].text.contains("(123.45)"));
    assert_eq!(
        imported.pages[0].text.lines().count(),
        2,
        "PDF text-position changes must preserve separate bank rows"
    );
    imported.set_name("sample-statement.pdf");
    assert_eq!(imported.pages[1].name, "sample-statement.pdf");
    assert_eq!(imported.pages[1].page, 2);
    assert_eq!(imported.pages[0].source, "pdf_text");
    assert_eq!(imported.pages[0].confidence, None);
    let empty = pdf_import::extract(&document(&[], 1, false)).unwrap();
    assert_eq!(empty.pages.len(), 1);
    assert!(!empty.has_text());
}

#[test]
fn independently_positioned_pdf_cells_keep_rows_and_column_boundaries() {
    let mut doc = Document::load_mem(&document(&[], 1, false)).unwrap();
    let mut operations = vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 12.into()]),
    ];
    for (y, values) in [
        (740, ["11/11/2026", "Sample shop", "19.99", "900.00"]),
        (700, ["11/12/2026", "Sample clay", "25.00", "875.00"]),
    ] {
        for (x, text) in [40, 150, 420, 510].into_iter().zip(values) {
            operations.push(Operation::new(
                "Tm",
                vec![1.into(), 0.into(), 0.into(), 1.into(), x.into(), y.into()],
            ));
            operations.push(Operation::new("Tj", vec![Object::string_literal(text)]));
        }
    }
    operations.push(Operation::new("ET", vec![]));
    let page = *doc.get_pages().values().next().unwrap();
    let stream = doc.get_page_contents(page)[0];
    doc.get_object_mut(stream)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .set_plain_content(Content { operations }.encode().unwrap());
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let extracted = pdf_import::extract(&bytes).unwrap();
    let lines: Vec<_> = extracted.pages[0]
        .text
        .lines()
        .map(pdf_import::split_columns)
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], ["11/11/2026", "Sample shop", "19.99", "900.00"]);
    assert_eq!(lines[1], ["11/12/2026", "Sample clay", "25.00", "875.00"]);
}

#[test]
fn bounds_files_pages_and_decompressed_text_before_import() {
    assert!(pdf_import::extract(b"not a PDF").is_err());
    assert!(pdf_import::extract(&vec![0; pdf_import::MAX_PDF_BYTES + 1]).is_err());
    assert!(pdf_import::extract(&document(&["sample"], 101, false))
        .unwrap_err()
        .contains("pages"));
    assert!(pdf_import::extract(&document(&[], 1, true))
        .unwrap_err()
        .contains("limit"));
}

#[test]
fn ocr_results_preserve_every_sources_name_page_and_confidence() {
    let first = ExtractedPdf::from_ocr_json(r#"{"name":"sample.png","pages":[{"page":1,"text":"11/11/2026 Shop 12.34","confidence":79.5,"source":"image"}]}"#).unwrap();
    let mut second = pdf_import::extract(&document(&["Second source"], 1, false)).unwrap();
    second.set_name("sample.pdf");
    let joined = ExtractedPdf::combine(vec![first, second]);
    assert_eq!(joined.pages.len(), 2);
    assert_eq!(joined.pages[0].name, "sample.png");
    assert_eq!(joined.pages[0].confidence, Some(79.5));
    assert_eq!(joined.pages[1].name, "sample.pdf");
    assert!(ExtractedPdf::from_ocr_json(
        r#"{"name":"bad","pages":[{"page":0,"text":"a","confidence":90,"source":"image"}]}"#
    )
    .is_err());
    assert!(ExtractedPdf::from_ocr_json(
        r#"{"name":"bad","pages":[{"page":1,"text":"a","confidence":110,"source":"image"}]}"#
    )
    .is_err());
    assert!(
        ExtractedPdf::from_ocr_json(r#"{"name":"bad","pages":[],"error":"OCR stopped"}"#)
            .unwrap_err()
            .contains("OCR stopped")
    );
}

#[test]
fn mixed_pdf_ocr_never_replaces_an_existing_precise_text_layer() {
    let mut original =
        ExtractedPdf::from_ocr_text("mixed.pdf", "Exact amount 19.99", None, "pdf_text");
    let mut empty = original.pages[0].clone();
    empty.page = 2;
    empty.text.clear();
    original.pages.push(empty);
    let mut recognized =
        ExtractedPdf::from_ocr_text("mixed.pdf", "Wrong OCR amount 19.39", Some(70.0), "pdf_ocr");
    let mut second = recognized.pages[0].clone();
    second.page = 2;
    second.text = "Scanned transaction 12.34".into();
    recognized.pages.push(second);
    let result = ExtractedPdf::merge_ocr(original, recognized);
    assert_eq!(result.pages[0].text, "Exact amount 19.99");
    assert_eq!(result.pages[0].source, "pdf_text");
    assert_eq!(result.pages[1].text, "Scanned transaction 12.34");
    assert_eq!(result.pages[1].confidence, Some(70.0));
    assert_eq!(result.pages[1].name, "mixed.pdf");
}

#[test]
fn financial_columns_keep_multiple_amounts_and_raw_statement_meaning() {
    assert_eq!(
        pdf_import::split_columns("11/11/2026  Clay supplier  12.34  900.00"),
        ["11/11/2026", "Clay supplier", "12.34", "900.00"]
    );
    assert_eq!(
        pdf_import::split_financial("11/11/2026 Clay supplier 12.34 900.00"),
        ["11/11/2026", "Clay supplier", "12.34", "900.00"]
    );
    assert_eq!(
        pdf_import::split_financial("11/11/2026 11/12/2026 SHOP 19.99"),
        ["11/11/2026", "11/12/2026", "SHOP", "19.99"]
    );
    assert_eq!(
        pdf_import::split_financial("Account ending 1234"),
        ["Account ending 1234"]
    );
    assert_eq!(pdf_import::literal("=SUM(A1:A2)"), "'=SUM(A1:A2)");
    assert_eq!(
        pdf_import::literal("   =HYPERLINK(test)"),
        "'   =HYPERLINK(test)"
    );
    assert_eq!(pdf_import::literal("-19.99"), "-19.99");
    let a = vec![
        "2026-11-11".to_owned(),
        "Coffee   Shop".to_owned(),
        "12.34".to_owned(),
    ];
    let b = vec![
        "2026-11-11".to_owned(),
        " coffee shop ".to_owned(),
        "12.34".to_owned(),
    ];
    assert_eq!(pdf_import::duplicate_key(&a), pdf_import::duplicate_key(&b));
}

#[test]
fn dates_require_explicit_year_and_respect_the_chosen_order() {
    assert_eq!(
        pdf_import::parse_date("11/10/2026", &DateOrder::Mdy).as_deref(),
        Some("2026-11-10")
    );
    assert_eq!(
        pdf_import::parse_date("11/10/2026", &DateOrder::Dmy).as_deref(),
        Some("2026-10-11")
    );
    assert_eq!(
        pdf_import::parse_date("2026-11-10", &DateOrder::Mdy).as_deref(),
        Some("2026-11-10")
    );
    for input in ["11/10", "11/10/26", "02/30/2026", "26-11-10", "not a date"] {
        assert_eq!(
            pdf_import::parse_date(input, &DateOrder::Mdy),
            None,
            "{input}"
        );
    }
}

#[test]
fn amounts_retain_explicit_signs_and_do_not_guess_debit_credit_or_balance() {
    for (input, expected) in [
        ("$1,234.56", "1234.56"),
        ("($1,234.56)", "-1234.56"),
        ("-$19.99", "-19.99"),
        ("USD -19.99", "-19.99"),
        (".50", "0.5"),
        ("0.00", "0"),
    ] {
        assert_eq!(
            pdf_import::parse_amount(input, DecimalMark::Dot).as_deref(),
            Some(expected),
            "{input}"
        );
    }
    assert_eq!(
        pdf_import::parse_amount("1.234,56", DecimalMark::Comma).as_deref(),
        Some("1234.56")
    );
    for input in [
        "12.34DR",
        "12.34CR",
        "1,2.00",
        "1.234,56",
        "(--12.00)",
        "balance",
        "1e309",
    ] {
        assert_eq!(
            pdf_import::parse_amount(input, DecimalMark::Dot),
            None,
            "{input}"
        );
    }
}
