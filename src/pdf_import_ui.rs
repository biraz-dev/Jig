//! Inline, local review of extracted documents before any workbook mutation.
use std::collections::HashMap;

use eframe::egui::{self, Color32, RichText, TextEdit, Ui};
use jig::{
    pdf_import::{self, DateOrder, DecimalMark, ExtractedPdf},
    spreadsheet::{self, CellAddr, MAX_CELL_BYTES, MAX_COLS, MAX_RAW_BYTES, MAX_ROWS},
};

const MUTED: Color32 = Color32::from_rgb(144, 161, 202);
const PINK: Color32 = Color32::from_rgb(234, 133, 173);
const CREAM: Color32 = Color32::from_rgb(246, 218, 174);
const FIELD_WIDTH: f32 = 156.0;

pub enum PdfImportAction {
    Apply {
        sheet: u64,
        origin: CellAddr,
        rows: Vec<Vec<String>>,
        date_columns: Vec<usize>,
        amount_columns: Vec<usize>,
    },
    Cancel,
}

#[derive(Clone, Copy, PartialEq)]
enum Split {
    Financial,
    Tabs,
    Commas,
    Lines,
}

impl Split {
    fn label(self) -> &'static str {
        match self {
            Self::Financial => "Statement columns",
            Self::Tabs => "Tabs",
            Self::Commas => "CSV",
            Self::Lines => "One column",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Role {
    Text,
    Date,
    Description,
    Debit,
    Credit,
    Amount,
    Skip,
}

impl Role {
    const ALL: [Self; 7] = [
        Self::Text,
        Self::Date,
        Self::Description,
        Self::Debit,
        Self::Credit,
        Self::Amount,
        Self::Skip,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Text => "Keep text",
            Self::Date => "Date",
            Self::Description => "Description",
            Self::Debit => "Debit · USD",
            Self::Credit => "Credit · USD",
            Self::Amount => "Amount · USD",
            Self::Skip => "Skip column",
        }
    }

    fn is_amount(self) -> bool {
        matches!(self, Self::Debit | Self::Credit | Self::Amount)
    }
}

struct ReviewRow {
    page: usize,
    line: usize,
    raw: String,
    fields: Vec<String>,
    include: bool,
}

#[derive(Default)]
struct CheckedRow {
    fields: Vec<String>,
    issues: Vec<String>,
    duplicate: bool,
}

pub struct PdfImportPanel {
    document: ExtractedPdf,
    drafts: Vec<String>,
    sheet: u64,
    origin: String,
    page: usize,
    split: Split,
    first_line_headers: bool,
    write_headers: bool,
    source_columns: bool,
    show_text: bool,
    show_original: bool,
    dirty: bool,
    rows: Vec<ReviewRow>,
    checked: Vec<CheckedRow>,
    roles: Vec<Role>,
    names: Vec<String>,
    date_order: DateOrder,
    decimal_mark: DecimalMark,
    keep_flagged: bool,
    parse_error: Option<String>,
    error: Option<String>,
}

impl PdfImportPanel {
    pub fn new(document: ExtractedPdf, sheet: u64, origin: CellAddr) -> Self {
        let drafts = document
            .pages
            .iter()
            .map(|page| page.text.clone())
            .collect();
        let mut panel = Self {
            document,
            drafts,
            sheet,
            origin: origin.label(),
            page: 0,
            split: Split::Financial,
            first_line_headers: false,
            write_headers: origin.row == 0,
            source_columns: true,
            show_text: false,
            show_original: false,
            dirty: false,
            rows: Vec::new(),
            checked: Vec::new(),
            roles: Vec::new(),
            names: Vec::new(),
            date_order: DateOrder::Mdy,
            decimal_mark: DecimalMark::Dot,
            keep_flagged: false,
            parse_error: None,
            error: None,
        };
        panel.rebuild();
        panel
    }

    fn rebuild(&mut self) {
        let included: HashMap<_, _> = self
            .rows
            .iter()
            .map(|row| ((row.page, row.line, row.raw.clone()), row.include))
            .collect();
        self.rows.clear();
        self.names.clear();
        self.parse_error = None;
        self.error = None;
        if self.drafts.iter().map(String::len).sum::<usize>() > pdf_import::MAX_TEXT_BYTES {
            self.parse_error = Some("Edited source text exceeds the 1 MiB review limit.".into());
            self.checked.clear();
            self.dirty = false;
            return;
        }
        let mut width = 0;
        for (page, text) in self.drafts.iter().enumerate() {
            let lines: Vec<_> = text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .take(MAX_ROWS + 1)
                .collect();
            let parsed = match self.split {
                Split::Commas => spreadsheet::parse_delimited(text, ','),
                Split::Tabs => spreadsheet::parse_delimited(text, '\t'),
                _ => Ok(lines
                    .iter()
                    .map(|line| match self.split {
                        Split::Financial => pdf_import::split_financial(line),
                        _ => vec![line.trim().to_owned()],
                    })
                    .collect()),
            };
            let parsed = match parsed {
                Ok(parsed) => parsed,
                Err(error) => {
                    self.parse_error = Some(format!("Page {}: {error}", page + 1));
                    continue;
                }
            };
            for (line, fields) in parsed.into_iter().enumerate() {
                if fields.iter().all(|field| field.trim().is_empty()) {
                    continue;
                }
                if self.first_line_headers && line == 0 {
                    if self.names.is_empty() {
                        self.names = fields;
                    }
                    continue;
                }
                if self.rows.len() >= MAX_ROWS || fields.len() > MAX_COLS {
                    self.parse_error = Some(format!(
                        "The review supports at most {MAX_ROWS} rows and {MAX_COLS} columns. Split this document into smaller imports."
                    ));
                    break;
                }
                width = width.max(fields.len());
                let raw = if matches!(self.split, Split::Tabs | Split::Commas) {
                    spreadsheet::encode_delimited(std::slice::from_ref(&fields), '\t')
                        .trim_end_matches(['\r', '\n'])
                        .to_owned()
                } else {
                    lines.get(line).copied().unwrap_or_default().to_owned()
                };
                let include = included
                    .get(&(page, line, raw.clone()))
                    .copied()
                    .unwrap_or(true);
                self.rows.push(ReviewRow {
                    page,
                    line,
                    raw,
                    fields,
                    include,
                });
            }
        }
        self.roles.resize(width, Role::Text);
        self.roles.truncate(width);
        self.names.resize_with(width, String::new);
        for row in &mut self.rows {
            row.fields.resize(width, String::new());
        }
        self.dirty = false;
        self.check();
    }

    fn check(&mut self) {
        let mut counts = HashMap::new();
        for row in &self.rows {
            *counts
                .entry(pdf_import::duplicate_key(&row.fields))
                .or_insert(0_usize) += 1;
        }
        self.checked = self
            .rows
            .iter()
            .map(|row| {
                let mut checked = CheckedRow {
                    duplicate: counts[&pdf_import::duplicate_key(&row.fields)] > 1,
                    ..Default::default()
                };
                for (column, (field, role)) in row.fields.iter().zip(&self.roles).enumerate() {
                    let parsed = if field.trim().is_empty() {
                        Some(String::new())
                    } else if *role == Role::Date {
                        pdf_import::parse_date(field, &self.date_order)
                    } else if role.is_amount() {
                        pdf_import::parse_amount(field, self.decimal_mark)
                    } else {
                        Some(field.clone())
                    };
                    if parsed.is_none() {
                        checked.issues.push(format!(
                            "Check {} in column {}",
                            role.label(),
                            column + 1
                        ));
                    }
                    checked.fields.push(parsed.unwrap_or_else(|| field.clone()));
                }
                checked
            })
            .collect();
    }

    fn output(&self) -> Result<PdfImportAction, String> {
        if self.dirty {
            return Err("Update the preview after editing extracted text.".into());
        }
        if let Some(error) = &self.parse_error {
            return Err(error.clone());
        }
        let origin = CellAddr::parse(self.origin.trim())?;
        let columns: Vec<_> = self
            .roles
            .iter()
            .enumerate()
            .filter(|(_, role)| **role != Role::Skip)
            .collect();
        if columns.is_empty() {
            return Err("Keep at least one source column.".into());
        }
        let selected = self.rows.iter().filter(|row| row.include).count();
        if selected == 0 {
            return Err("Select at least one row to import.".into());
        }
        let width = columns.len() + if self.source_columns { 3 } else { 0 };
        if origin.col + width > MAX_COLS
            || origin.row + selected + usize::from(self.write_headers) > MAX_ROWS
        {
            return Err("The reviewed rows do not fit at this starting cell. Move the start or import fewer rows.".into());
        }
        let mut rows = Vec::new();
        if self.write_headers {
            let mut headers: Vec<_> = columns
                .iter()
                .map(|(column, role)| {
                    if **role == Role::Text {
                        if self.names[*column].is_empty() {
                            format!("Column {}", column + 1)
                        } else {
                            self.names[*column].clone()
                        }
                    } else {
                        role.label().replace(" · USD", "")
                    }
                })
                .collect();
            if self.source_columns {
                headers.extend(["Source file".into(), "Page".into(), "Source text".into()]);
            }
            rows.push(
                headers
                    .into_iter()
                    .map(|text| pdf_import::literal_text(&text))
                    .collect(),
            );
        }
        for (row, checked) in self
            .rows
            .iter()
            .zip(&self.checked)
            .filter(|(row, _)| row.include)
        {
            if !self.keep_flagged && !checked.issues.is_empty() {
                return Err("Correct flagged dates/amounts, exclude their rows, or choose to keep flagged values as text.".into());
            }
            let mut fields: Vec<_> = columns
                .iter()
                .map(|(column, role)| {
                    let source = &row.fields[*column];
                    let typed = (**role == Role::Date
                        && pdf_import::parse_date(source, &self.date_order).is_some())
                        || (role.is_amount()
                            && pdf_import::parse_amount(source, self.decimal_mark).is_some());
                    if typed {
                        pdf_import::literal(&checked.fields[*column])
                    } else {
                        pdf_import::literal_text(&checked.fields[*column])
                    }
                })
                .collect();
            if self.source_columns {
                let page = &self.document.pages[row.page];
                fields.extend([
                    pdf_import::literal_text(&page.name),
                    page.page.to_string(),
                    pdf_import::literal_text(&row.raw),
                ]);
            }
            rows.push(fields);
        }
        if rows
            .iter()
            .flatten()
            .any(|field| field.len() > MAX_CELL_BYTES)
            || rows.iter().flatten().map(String::len).sum::<usize>() > MAX_RAW_BYTES
            || rows.iter().map(Vec::len).sum::<usize>() > spreadsheet::MAX_CELLS
        {
            return Err("This import exceeds the sheet's cell or text limit. Import fewer rows or omit source columns.".into());
        }
        let date_columns = columns
            .iter()
            .enumerate()
            .filter_map(|(index, (_, role))| (**role == Role::Date).then_some(index))
            .collect();
        let amount_columns = columns
            .iter()
            .enumerate()
            .filter_map(|(index, (_, role))| role.is_amount().then_some(index))
            .collect();
        Ok(PdfImportAction::Apply {
            sheet: self.sheet,
            origin,
            rows,
            date_columns,
            amount_columns,
        })
    }

    pub fn ui(&mut self, ui: &mut Ui) -> Option<PdfImportAction> {
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("REVIEW IMPORT").color(PINK));
            ui.label(RichText::new(&self.document.name).color(CREAM));
            ui.label(
                RichText::new(format!("{} pages · local", self.document.pages.len())).color(MUTED),
            );
            if ui.button("Cancel import").clicked() {
                action = Some(PdfImportAction::Cancel);
            }
        });
        ui.label(
            RichText::new(
                "Map columns, review flagged rows, then import. Nothing changes until Apply.",
            )
            .color(MUTED)
            .size(11.0),
        );
        let mut rebuild = false;
        let mut recheck = false;
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("document_split")
                .selected_text(self.split.label())
                .show_ui(ui, |ui| {
                    for split in [Split::Financial, Split::Tabs, Split::Commas, Split::Lines] {
                        rebuild |= ui
                            .selectable_value(&mut self.split, split, split.label())
                            .changed();
                    }
                });
            rebuild |= ui
                .checkbox(
                    &mut self.first_line_headers,
                    "Each page starts with column names",
                )
                .changed();
            ui.checkbox(&mut self.write_headers, "Write headers");
            ui.checkbox(&mut self.source_columns, "Include source columns");
            ui.checkbox(&mut self.show_text, "Edit extracted text");
        });
        if self.show_text && !self.document.pages.is_empty() {
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("document_page")
                    .selected_text(format!("Page {}", self.page + 1))
                    .show_ui(ui, |ui| {
                        for (index, page) in self.document.pages.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.page,
                                index,
                                format!("{} · page {}", page.name, page.page),
                            );
                        }
                    });
                let page = &self.document.pages[self.page];
                ui.label(
                    RichText::new(format!("{} · {}", page.name, page.source))
                        .color(MUTED)
                        .size(11.0),
                );
                ui.checkbox(&mut self.show_original, "Original text");
                if ui.button("Update preview").clicked() {
                    rebuild = true;
                }
            });
            let height = (ui.available_height() * 0.22).clamp(60.0, 125.0);
            egui::ScrollArea::vertical()
                .id_salt("document_text")
                .max_height(height)
                .show(ui, |ui| {
                    if self.show_original {
                        ui.add(
                            TextEdit::multiline(&mut self.document.pages[self.page].text.as_str())
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .interactive(false),
                        );
                    } else if ui
                        .add(
                            TextEdit::multiline(&mut self.drafts[self.page])
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .char_limit(pdf_import::MAX_TEXT_BYTES),
                        )
                        .changed()
                    {
                        self.dirty = true;
                    }
                });
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Dates:");
            egui::ComboBox::from_id_salt("document_date_order")
                .selected_text(match self.date_order {
                    DateOrder::Mdy => "US · month/day/year",
                    DateOrder::Dmy => "Day/month/year",
                    DateOrder::Ymd => "Year/month/day",
                })
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (DateOrder::Mdy, "US · month/day/year"),
                        (DateOrder::Dmy, "Day/month/year"),
                        (DateOrder::Ymd, "Year/month/day"),
                    ] {
                        recheck |= ui
                            .selectable_value(&mut self.date_order, value, label)
                            .changed();
                    }
                });
            ui.label("Amounts: USD");
            egui::ComboBox::from_id_salt("document_decimal")
                .selected_text(match self.decimal_mark {
                    DecimalMark::Dot => "1,234.56",
                    DecimalMark::Comma => "1.234,56",
                })
                .show_ui(ui, |ui| {
                    recheck |= ui
                        .selectable_value(&mut self.decimal_mark, DecimalMark::Dot, "1,234.56")
                        .changed();
                    recheck |= ui
                        .selectable_value(&mut self.decimal_mark, DecimalMark::Comma, "1.234,56")
                        .changed();
                });
        });
        if rebuild {
            self.rebuild();
        }
        if recheck {
            self.check();
        }
        if let Some(error) = self.parse_error.as_ref().or(self.error.as_ref()) {
            ui.colored_label(PINK, error);
        }
        if self.dirty {
            ui.colored_label(
                PINK,
                "Extracted text changed. Update preview before importing.",
            );
        }
        let selected = self.rows.iter().filter(|row| row.include).count();
        let flagged = self
            .checked
            .iter()
            .zip(&self.rows)
            .filter(|(checked, row)| {
                checked.duplicate
                    || !checked.issues.is_empty()
                    || self.document.pages[row.page]
                        .confidence
                        .is_some_and(|confidence| confidence < 85.0)
            })
            .count();
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "{selected}/{} rows selected · {flagged} flagged",
                self.rows.len()
            ));
            if ui.button("All rows").clicked() {
                self.rows.iter_mut().for_each(|row| row.include = true);
            }
            if ui.button("None").clicked() {
                self.rows.iter_mut().for_each(|row| row.include = false);
            }
            ui.checkbox(
                &mut self.keep_flagged,
                "Keep unrecognized dates/amounts as text",
            );
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Start cell");
            ui.add(
                TextEdit::singleline(&mut self.origin)
                    .desired_width(68.0)
                    .char_limit(12),
            );
            if ui
                .add_enabled(
                    !self.dirty && self.parse_error.is_none() && selected > 0,
                    egui::Button::new("Apply reviewed rows"),
                )
                .clicked()
            {
                match self.output() {
                    Ok(apply) => action = Some(apply),
                    Err(error) => self.error = Some(error),
                }
            }
            ui.label(
                RichText::new("Duplicates remain until unchecked; signs stay as written.")
                    .color(MUTED)
                    .size(10.0),
            );
        });
        ui.separator();
        let mut changed_fields = false;
        let mut changed_roles = false;
        let height = (ui.available_height() - 48.0).max(55.0);
        egui::ScrollArea::horizontal()
            .id_salt("document_columns")
            .show(ui, |ui| {
                ui.set_min_width(84.0 + FIELD_WIDTH * self.roles.len() as f32);
                ui.horizontal(|ui| {
                    ui.add_sized([76.0, 22.0], egui::Label::new("Include"));
                    for (column, role) in self.roles.iter_mut().enumerate() {
                        ui.allocate_ui_with_layout(
                            egui::vec2(FIELD_WIDTH - ui.spacing().item_spacing.x, 22.0),
                            egui::Layout::top_down(egui::Align::LEFT),
                            |ui| {
                                egui::ComboBox::from_id_salt(("document_role", column))
                                    .width(FIELD_WIDTH - 16.0)
                                    .selected_text(format!("{} · {}", column + 1, role.label()))
                                    .show_ui(ui, |ui| {
                                        for candidate in Role::ALL {
                                            changed_roles |= ui
                                                .selectable_value(
                                                    role,
                                                    candidate,
                                                    candidate.label(),
                                                )
                                                .changed();
                                        }
                                    });
                            },
                        );
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("document_rows")
                    .max_height(height)
                    .show_rows(ui, 59.0, self.rows.len(), |ui, visible| {
                        for index in visible {
                            let row = &mut self.rows[index];
                            let checked = &self.checked[index];
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), 59.0),
                                egui::Layout::top_down(egui::Align::LEFT),
                                |ui| {
                                    ui.push_id(index, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.allocate_ui_with_layout(
                                                egui::vec2(76.0, 26.0),
                                                egui::Layout::left_to_right(egui::Align::Center),
                                                |ui| {
                                                    ui.checkbox(
                                                        &mut row.include,
                                                        (index + 1).to_string(),
                                                    );
                                                },
                                            );
                                            for (column, field) in row.fields.iter_mut().enumerate()
                                            {
                                                let text = TextEdit::singleline(field)
                                                    .desired_width(FIELD_WIDTH - 8.0)
                                                    .char_limit(MAX_CELL_BYTES)
                                                    .horizontal_align(
                                                        if self.roles[column].is_amount() {
                                                            egui::Align::RIGHT
                                                        } else {
                                                            egui::Align::LEFT
                                                        },
                                                    );
                                                changed_fields |= ui
                                                    .add_enabled(
                                                        self.roles[column] != Role::Skip,
                                                        text,
                                                    )
                                                    .on_hover_text(&checked.fields[column])
                                                    .changed();
                                            }
                                        });
                                        let page = &self.document.pages[row.page];
                                        let mut notes = Vec::new();
                                        if checked.duplicate {
                                            notes.push("possible duplicate".to_owned());
                                        }
                                        if let Some(confidence) =
                                            page.confidence.filter(|confidence| *confidence < 85.0)
                                        {
                                            notes.push(format!("OCR {confidence:.0}% · review"));
                                        }
                                        notes.extend(checked.issues.iter().cloned());
                                        let source = format!(
                                            "{} · page {} · source row {}",
                                            page.name,
                                            page.page,
                                            row.line + 1
                                        );
                                        let label = if notes.is_empty() {
                                            source
                                        } else {
                                            format!("{source}  |  {}", notes.join(" · "))
                                        };
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(label)
                                                    .color(if notes.is_empty() {
                                                        MUTED
                                                    } else {
                                                        PINK
                                                    })
                                                    .size(10.0),
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(&row.raw);
                                    })
                                },
                            );
                        }
                    });
            });
        if changed_roles || changed_fields {
            self.check();
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(text: &str, row: usize) -> PdfImportPanel {
        PdfImportPanel::new(
            ExtractedPdf::from_ocr_text("statement.png", text, Some(79.0), "image_ocr"),
            7,
            CellAddr::new(row, 0),
        )
    }

    #[test]
    fn reviewed_financial_fields_retain_provenance_and_signs() {
        let mut panel = panel("11/09/2026\tClay order\t(125.50)", 0);
        panel.roles = vec![Role::Date, Role::Description, Role::Amount];
        panel.check();
        let PdfImportAction::Apply {
            sheet,
            origin,
            rows,
            date_columns,
            amount_columns,
        } = panel.output().unwrap()
        else {
            panic!("Expected Apply")
        };
        assert_eq!(sheet, 7);
        assert_eq!(origin, CellAddr::new(0, 0));
        assert_eq!(
            rows[0],
            [
                "Date",
                "Description",
                "Amount",
                "Source file",
                "Page",
                "Source text"
            ]
        );
        assert_eq!(&rows[1][..3], ["2026-11-09", "Clay order", "-125.5"]);
        assert_eq!(&rows[1][3..5], ["statement.png", "1"]);
        assert_eq!(date_columns, [0]);
        assert_eq!(amount_columns, [2]);
    }

    #[test]
    fn unrecognized_amount_requires_explicit_acknowledgment_and_stays_literal() {
        let mut panel = panel("11/09\t=SUM(A1:A9)\t12.OO", 3);
        assert!(!panel.write_headers);
        panel.roles = vec![Role::Date, Role::Description, Role::Amount];
        panel.check();
        assert!(panel.output().is_err());
        panel.keep_flagged = true;
        let PdfImportAction::Apply { rows, .. } = panel.output().unwrap() else {
            panic!("Expected Apply")
        };
        assert_eq!(&rows[0][..3], ["11/09", "'=SUM(A1:A9)", "12.OO"]);
    }

    #[test]
    fn duplicates_are_flagged_but_never_silently_removed() {
        let mut panel = panel("2026-11-09\tClay\t25.00\n2026-11-09\tClay\t25.00", 2);
        assert_eq!(panel.rows.len(), 2);
        assert!(panel.checked.iter().all(|row| row.duplicate));
        let PdfImportAction::Apply { rows, .. } = panel.output().unwrap() else {
            panic!("Expected Apply")
        };
        assert_eq!(rows.len(), 2);
        panel.rows[1].include = false;
        let PdfImportAction::Apply { rows, .. } = panel.output().unwrap() else {
            panic!("Expected Apply")
        };
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn skipped_columns_adjust_formats_and_bounds_are_checked() {
        let mut panel = panel("ignore\t2026-11-09\t25.00", 999);
        panel.roles = vec![Role::Skip, Role::Date, Role::Debit];
        panel.source_columns = false;
        panel.check();
        let PdfImportAction::Apply {
            rows,
            date_columns,
            amount_columns,
            ..
        } = panel.output().unwrap()
        else {
            panic!("Expected Apply")
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(date_columns, [0]);
        assert_eq!(amount_columns, [1]);
        panel.write_headers = true;
        assert!(panel.output().is_err());
        panel.write_headers = false;
        panel.origin = "AZ1000".into();
        assert!(panel.output().is_err());
    }
}
