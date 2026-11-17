use jig::{
    documents::{read_ods, save_ods},
    draft::{
        Alignment, Block, Draft, ListKind, Run, DEFAULT_SIZE, MAX_BLOCKS, MAX_LINK_BYTES, MAX_RUNS,
        MAX_TEXT_BYTES,
    },
    model::State,
    spreadsheet::{CellAddr, Workbook},
};
use std::io::{Cursor, Read, Write};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

fn run(text: &str) -> Run {
    Run {
        text: text.into(),
        ..Run::default()
    }
}

fn block(text: &str, list: ListKind) -> Block {
    Block {
        runs: vec![run(text)],
        list,
        ..Block::default()
    }
}

fn formatted() -> Draft {
    Draft {
        blocks: vec![
            Block {
                alignment: Alignment::Center,
                runs: vec![Run {
                    text: "Résumé — 作品\t".into(),
                    size: 24,
                    bold: true,
                    italic: true,
                    underline: true,
                    link: Some("https://example.test/作品?q=one%20two".into()),
                }],
                ..Block::default()
            },
            Block {
                alignment: Alignment::Right,
                runs: vec![run("Keep "), run("these spans\nand line breaks.")],
                ..Block::default()
            },
            Block::default(),
            Block {
                alignment: Alignment::Justify,
                ..block("Gather references", ListKind::Bullet)
            },
            block("Arrange", ListKind::Number),
            block("Send", ListKind::Number),
        ],
    }
}

#[test]
fn old_files_load_empty_and_partial_draft_fields_use_sensible_defaults() {
    let old = r#"{"version":1,"preferences":{},"history":[],"tasks":[],"deadlines":[],"entries":[],"featured":null}"#;
    let mut state = State::from_json(old).unwrap();
    assert!(state.draft.is_empty());
    assert_eq!(state.draft.plain_text(), "");
    assert!(!serde_json::to_value(&state)
        .unwrap()
        .as_object()
        .unwrap()
        .contains_key("draft"));

    state.draft =
        serde_json::from_str(r#"{"blocks":[{"runs":[{"text":"  <b>literal</b>  "}]}]}"#).unwrap();
    state.validate().unwrap();
    let first = &state.draft.blocks[0];
    assert_eq!(first.alignment, Alignment::Left);
    assert_eq!(first.list, ListKind::None);
    assert_eq!(first.runs[0].size, DEFAULT_SIZE);
    assert_eq!(state.draft.plain_text(), "  <b>literal</b>  ");
    assert_eq!(
        serde_json::to_value(&state.draft).unwrap(),
        serde_json::json!({"blocks":[{"runs":[{"text":"  <b>literal</b>  "}]}]})
    );
}

#[test]
fn json_round_trip_preserves_all_formatting_and_other_jig_records() {
    let mut state = State {
        draft: formatted(),
        ..State::default()
    };
    state.add_task("Prepare photographs").unwrap();
    state.add_note("Keep this note\nwith its text").unwrap();
    let json = serde_json::to_string_pretty(&state).unwrap();
    let restored = State::from_json(&json).unwrap();
    assert_eq!(restored.draft, state.draft);
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(state).unwrap()
    );
}

#[test]
fn plain_text_preserves_spacing_and_list_order_without_format_markup() {
    let mut draft = formatted();
    draft.blocks.extend([
        block("New paragraph", ListKind::None),
        block("A new list", ListKind::Number),
        Block::default(),
    ]);
    assert_eq!(
        draft.plain_text(),
        "Résumé — 作品\t\nKeep these spans\nand line breaks.\n\n• Gather references\n1. Arrange\n2. Send\nNew paragraph\n1. A new list\n"
    );
    let blank = Draft {
        blocks: vec![Block::default(), Block::default()],
    };
    assert!(
        !blank.is_empty(),
        "Explicit blank paragraphs retain spacing"
    );
    assert_eq!(blank.plain_text(), "\n");
    assert_eq!(
        serde_json::from_str::<Draft>(&serde_json::to_string(&blank).unwrap()).unwrap(),
        blank
    );
}

#[test]
fn document_limits_count_utf8_bytes_and_all_runs_across_blocks() {
    let mut draft = Draft {
        blocks: vec![block(
            &("界".repeat(MAX_TEXT_BYTES / 3) + "aa"),
            ListKind::None,
        )],
    };
    assert_eq!(draft.blocks[0].runs[0].text.len(), MAX_TEXT_BYTES);
    draft.validate().unwrap();
    draft.blocks[0].runs[0].text.push('界');
    assert!(draft.validate().unwrap_err().contains("512 KiB"));

    let mut draft = Draft {
        blocks: vec![Block::default(); MAX_BLOCKS],
    };
    draft.validate().unwrap();
    draft.blocks.push(Block::default());
    assert!(draft.validate().unwrap_err().contains("4096"));

    let mut draft = Draft {
        blocks: vec![
            Block {
                runs: vec![Run::default(); MAX_RUNS / 2],
                ..Block::default()
            };
            2
        ],
    };
    draft.validate().unwrap();
    draft.blocks[1].runs.push(Run::default());
    assert!(draft.validate().unwrap_err().contains("16384"));
}

#[test]
fn unsupported_text_sizes_and_control_characters_are_rejected() {
    let mut draft = Draft {
        blocks: vec![block("Plain\ttext\r\nand Unicode 👋", ListKind::None)],
    };
    for size in [8, 16, 72] {
        draft.blocks[0].runs[0].size = size;
        draft.validate().unwrap();
    }
    for size in [0, 7, 73, 255] {
        draft.blocks[0].runs[0].size = size;
        assert!(draft.validate().unwrap_err().contains("size"));
    }
    draft.blocks[0].runs[0].size = DEFAULT_SIZE;
    for character in ['\0', '\u{7}', '\u{7f}', '\u{85}'] {
        draft.blocks[0].runs[0].text = character.into();
        assert!(draft.validate().unwrap_err().contains("control"));
    }
    let mut value = serde_json::to_value(State::default()).unwrap();
    value["draft"] = serde_json::json!({"blocks":[{"runs":[{"text":"bad", "size":0}]}]});
    assert!(State::from_json(&value.to_string())
        .unwrap_err()
        .contains("size"));
}

#[test]
fn hyperlink_validation_accepts_portable_urls_and_rejects_executable_schemes() {
    let mut draft = Draft {
        blocks: vec![block("Link", ListKind::None)],
    };
    for link in [
        "http://example.test/path?q=a%20b#c",
        "HTTPS://example.test/作品",
        "mailto:artist@example.test?subject=Studio%20visit",
    ] {
        draft.blocks[0].runs[0].link = Some(link.into());
        draft.validate().unwrap();
    }
    for link in [
        "javascript:alert(1)",
        "JaVaScRiPt:alert(1)",
        "data:text/html,hello",
        "file:///tmp/private",
        "//example.test/path",
        "../notes",
        "https://",
        "http:///path",
        "https://?query",
        "mailto:",
        "https://example.test/a b",
        "https://example.test/\npath",
        "https://example.test/\u{a0}path",
    ] {
        draft.blocks[0].runs[0].link = Some(link.into());
        assert!(draft.validate().is_err(), "Accepted unsafe link: {link:?}");
    }
    draft.blocks[0].runs[0].link =
        Some("https://example.test/".to_owned() + &"a".repeat(MAX_LINK_BYTES));
    assert!(draft.validate().is_err());
    let mut value = serde_json::to_value(State::default()).unwrap();
    value["draft"] = serde_json::to_value(&draft).unwrap();
    assert!(State::from_json(&value.to_string()).is_err());
}

fn change_visible_cell(bytes: &[u8]) -> Vec<u8> {
    let mut source = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut target = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..source.len() {
        let mut file = source.by_index(index).unwrap();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        if file.name() == "content.xml" {
            let xml = String::from_utf8(data).unwrap();
            assert!(xml.contains("office:value=\"10\""));
            data = xml
                .replace("office:value=\"10\"", "office:value=\"11\"")
                .replace("<text:p>10</text:p>", "<text:p>11</text:p>")
                .into_bytes();
        }
        target
            .start_file(file.name(), SimpleFileOptions::default())
            .unwrap();
        target.write_all(&data).unwrap();
    }
    target.finish().unwrap().into_inner()
}

#[test]
fn jig_ods_preserves_draft_even_when_an_external_editor_changes_the_grid() {
    let mut workbook = Workbook::default();
    workbook.set_cell(1, CellAddr::new(0, 0), "10").unwrap();
    let mut state = State {
        draft: formatted(),
        workbook: Some(workbook),
        ..State::default()
    };
    state.add_task("Keep this task").unwrap();
    let bytes = save_ods(&state).unwrap();
    let exact = read_ods(&bytes).unwrap().embedded_state.unwrap();
    assert_eq!(exact.draft, state.draft);
    assert_eq!(
        serde_json::to_value(&exact).unwrap(),
        serde_json::to_value(&state).unwrap()
    );
    let external = read_ods(&change_visible_cell(&bytes)).unwrap();
    assert_eq!(external.workbook.raw(1, CellAddr::new(0, 0)), "11");
    let restored = external.embedded_state.unwrap();
    assert_eq!(restored.draft, state.draft);
    assert_eq!(restored.tasks[0].text, "Keep this task");
    assert_eq!(restored.workbook.unwrap().raw(1, CellAddr::new(0, 0)), "11");
}
