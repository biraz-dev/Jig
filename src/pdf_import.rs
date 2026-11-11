//! Local, bounded PDF text extraction and review helpers. Source files never
//! leave this process. Extracted text is data, never an executable formula.
use crate::Result;
use flate2::read::ZlibDecoder;
use lopdf::{content::Content, Document, Object, ObjectId};
use serde::{Deserialize, Serialize};
use std::io::Read;

pub const MAX_PDF_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_PDF_PAGES: usize = 100;
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;
const MAX_STREAM_BYTES: usize = 4 * 1024 * 1024;
const MAX_DECODED_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PdfPage {
    pub name: String,
    pub page: usize,
    pub text: String,
    pub confidence: Option<f32>,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtractedPdf {
    pub name: String,
    pub pages: Vec<PdfPage>,
}

impl ExtractedPdf {
    pub fn has_text(&self) -> bool {
        self.pages.iter().any(|page| !page.text.trim().is_empty())
    }

    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_owned();
        for page in &mut self.pages {
            page.name = name.to_owned();
        }
    }

    pub fn combine(documents: Vec<Self>) -> Self {
        let name = if documents.len() == 1 {
            documents[0].name.clone()
        } else {
            format!("{} documents", documents.len())
        };
        Self {
            name,
            pages: documents.into_iter().flat_map(|item| item.pages).collect(),
        }
    }

    /// Retain precise selectable text on mixed PDFs and use OCR only for pages
    /// whose text layer was empty. Empty OCR pages are retained for review.
    pub fn merge_ocr(mut original: Self, recognized: Self) -> Self {
        for page in recognized.pages {
            match original
                .pages
                .iter_mut()
                .find(|item| item.page == page.page)
            {
                Some(existing) if existing.text.trim().is_empty() => *existing = page,
                Some(_) => {}
                None => original.pages.push(page),
            }
        }
        original.pages.sort_by_key(|page| page.page);
        original
    }

    pub fn from_ocr_text(name: &str, text: &str, confidence: Option<f32>, source: &str) -> Self {
        Self {
            name: name.to_owned(),
            pages: vec![PdfPage {
                name: name.to_owned(),
                page: 1,
                text: text.to_owned(),
                confidence,
                source: source.to_owned(),
            }],
        }
    }

    /// Decode the local OCR worker's result, retaining filename/page provenance.
    pub fn from_ocr_json(text: &str) -> Result<Self> {
        #[derive(Deserialize)]
        struct WorkerPage {
            text: String,
            confidence: Option<f32>,
            source: String,
            page: usize,
        }
        #[derive(Deserialize)]
        struct WorkerResult {
            name: String,
            #[serde(default)]
            pages: Vec<WorkerPage>,
            error: Option<String>,
        }
        if text.len() > MAX_TEXT_BYTES * 2 {
            return Err("OCR text exceeds the review limit".into());
        }
        let result: WorkerResult = serde_json::from_str(text)
            .map_err(|error| format!("Could not decode local OCR result: {error}"))?;
        if let Some(error) = result.error {
            return Err(error);
        }
        if result.pages.len() > MAX_PDF_PAGES {
            return Err(format!("Import at most {MAX_PDF_PAGES} pages per document"));
        }
        let mut total = 0_usize;
        let mut pages = Vec::with_capacity(result.pages.len());
        for page in result.pages {
            total = total.saturating_add(page.text.len());
            if total > MAX_TEXT_BYTES || page.page == 0 {
                return Err("OCR text or page numbers exceed the review limits".into());
            }
            if page
                .confidence
                .is_some_and(|value| !value.is_finite() || !(0.0..=100.0).contains(&value))
            {
                return Err("OCR returned an invalid confidence value".into());
            }
            pages.push(PdfPage {
                name: result.name.clone(),
                page: page.page,
                text: page.text,
                confidence: page.confidence,
                source: page.source,
            });
        }
        Ok(Self {
            name: result.name,
            pages,
        })
    }
}

/// Extract selectable text from a PDF. Empty pages remain represented so the
/// caller can offer local OCR instead of silently skipping scanned pages.
pub fn extract(bytes: &[u8]) -> Result<ExtractedPdf> {
    if bytes.len() > MAX_PDF_BYTES {
        return Err("PDF exceeds the 10 MiB import limit".into());
    }
    if !bytes.starts_with(b"%PDF-") {
        return Err("Choose a PDF file".into());
    }
    let mut document =
        Document::load_mem(bytes).map_err(|error| format!("Could not read PDF: {error}"))?;
    if document.is_encrypted() {
        return Err(
            "This PDF is password-protected. Open an unlocked copy for local import.".into(),
        );
    }
    if document.objects.len() > 50_000 {
        return Err("PDF contains too many objects to import".into());
    }
    let pages = document.get_pages();
    if pages.is_empty() || pages.len() > MAX_PDF_PAGES {
        return Err(format!("Choose a PDF with 1–{MAX_PDF_PAGES} pages"));
    }
    bounded_streams(&mut document)?;
    let mut total = 0_usize;
    let mut extracted = Vec::with_capacity(pages.len());
    for (number, id) in &pages {
        let text = page_text(&document, *id).map_err(|error| {
            format!("Could not extract text on page {number}: {error}. Try local OCR for this document.")
        })?;
        total = total.saturating_add(text.len());
        if total > MAX_TEXT_BYTES {
            return Err("Extracted PDF text exceeds the 1 MiB review limit".into());
        }
        extracted.push(PdfPage {
            name: "PDF document".into(),
            page: *number as usize,
            text: text.replace('\r', ""),
            confidence: None,
            source: "pdf_text".into(),
        });
    }
    Ok(ExtractedPdf {
        name: "PDF document".into(),
        pages: extracted,
    })
}

struct TextRun {
    x: f64,
    y: f64,
    end_x: f64,
    size: f64,
    text: String,
}

fn number(values: &[Object], index: usize) -> Result<f64> {
    values
        .get(index)
        .and_then(|value| value.as_float().ok())
        .filter(|value| value.is_finite())
        .map(f64::from)
        .ok_or_else(|| "Invalid PDF text position".into())
}

fn transformed(matrix: [f64; 6], x: f64, y: f64) -> (f64, f64) {
    (
        matrix[0] * x + matrix[2] * y + matrix[4],
        matrix[1] * x + matrix[3] * y + matrix[5],
    )
}

/// The library's plain extractor discards text-position operators. Keep page
/// baselines and visible column gaps so statement rows do not concatenate.
fn page_text(document: &Document, page: ObjectId) -> Result<String> {
    let fonts = document
        .get_page_fonts(page)
        .map_err(|error| error.to_string())?;
    let mut encodings = std::collections::BTreeMap::new();
    for (name, font) in fonts {
        if let Ok(encoding) = font.get_font_encoding(document) {
            encodings.insert(name, encoding);
        }
    }
    let bytes = document
        .get_page_content(page)
        .map_err(|error| error.to_string())?;
    let content = Content::decode(&bytes).map_err(|error| error.to_string())?;
    if content.operations.len() > 200_000 {
        return Err("PDF page contains too many text operations".into());
    }
    let mut runs = Vec::new();
    let mut font = Vec::new();
    let mut size = 12.0_f64;
    let mut leading = 0.0_f64;
    let (mut x, mut y, mut line_x, mut line_y) = (0.0, 0.0, 0.0, 0.0);
    let mut matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut matrices = Vec::new();
    let mut decoded_bytes = 0_usize;
    let nested_forms = document.objects.values().any(|object| {
        matches!(object, Object::Stream(stream)
            if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Form"))
    });
    for operation in &content.operations {
        let values = &operation.operands;
        if operation.operator == "Do" && nested_forms {
            return Err(
                "PDF uses nested form content; use local OCR to avoid missing fields".into(),
            );
        }
        match operation.operator.as_str() {
            "q" => matrices.push(matrix),
            "Q" => matrix = matrices.pop().unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            "cm" => {
                let next = [
                    number(values, 0)?,
                    number(values, 1)?,
                    number(values, 2)?,
                    number(values, 3)?,
                    number(values, 4)?,
                    number(values, 5)?,
                ];
                let old = matrix;
                matrix = [
                    old[0] * next[0] + old[2] * next[1],
                    old[1] * next[0] + old[3] * next[1],
                    old[0] * next[2] + old[2] * next[3],
                    old[1] * next[2] + old[3] * next[3],
                    old[0] * next[4] + old[2] * next[5] + old[4],
                    old[1] * next[4] + old[3] * next[5] + old[5],
                ];
            }
            "BT" => {
                x = 0.0;
                y = 0.0;
                line_x = 0.0;
                line_y = 0.0;
            }
            "Tf" => {
                font = values
                    .first()
                    .and_then(|value| value.as_name().ok())
                    .ok_or("Invalid PDF font")?
                    .to_vec();
                size = number(values, 1)?.abs().clamp(0.1, 1000.0);
            }
            "TL" => leading = number(values, 0)?,
            "Tm" => {
                x = number(values, 4)?;
                y = number(values, 5)?;
                line_x = x;
                line_y = y;
            }
            "Td" | "TD" => {
                let vertical = number(values, 1)?;
                line_x += number(values, 0)?;
                line_y += vertical;
                x = line_x;
                y = line_y;
                if operation.operator == "TD" {
                    leading = -vertical;
                }
            }
            "T*" | "'" | "\"" => {
                line_y -= leading;
                x = line_x;
                y = line_y;
            }
            _ => {}
        }
        if !matches!(operation.operator.as_str(), "Tj" | "TJ" | "'" | "\"") {
            continue;
        }
        let encoding = encodings
            .get(&font)
            .ok_or("Unsupported PDF font encoding")?;
        let operands: &[Object] = if operation.operator == "TJ" {
            values
                .first()
                .and_then(|value| value.as_array().ok())
                .ok_or("Invalid PDF text array")?
        } else if operation.operator == "\"" {
            values.get(2..).ok_or("Invalid PDF text operand")?
        } else {
            values
        };
        for operand in operands {
            match operand {
                Object::String(encoded, _) => {
                    let text = Document::decode_text(encoding, encoded)
                        .map_err(|error| error.to_string())?;
                    decoded_bytes = decoded_bytes.saturating_add(text.len());
                    if decoded_bytes > MAX_TEXT_BYTES || runs.len() >= 100_000 {
                        return Err("PDF text exceeds the review limit".into());
                    }
                    let advance = text.chars().count() as f64 * size * 0.5;
                    let (px, py) = transformed(matrix, x, y);
                    let (end_x, _) = transformed(matrix, x + advance, y);
                    runs.push(TextRun {
                        x: px,
                        y: py,
                        end_x,
                        size,
                        text,
                    });
                    x += advance;
                }
                Object::Integer(value) => x -= *value as f64 * size / 1000.0,
                Object::Real(value) if value.is_finite() => x -= f64::from(*value) * size / 1000.0,
                _ => return Err("Unsupported PDF text operand".into()),
            }
        }
    }
    runs.sort_by(|a, b| b.y.total_cmp(&a.y).then_with(|| a.x.total_cmp(&b.x)));
    let mut text = String::new();
    let (mut baseline, mut end) = (f64::INFINITY, f64::NEG_INFINITY);
    for run in runs {
        if (run.y - baseline).abs() > (run.size * 0.35).max(1.5) {
            if !text.is_empty() {
                text.push('\n');
            }
            baseline = run.y;
        } else if !text.ends_with(char::is_whitespace) && !run.text.starts_with(char::is_whitespace)
        {
            let gap = run.x - end;
            if gap > run.size * 1.1 {
                text.push_str("  ");
            } else if gap > run.size * 0.15 {
                text.push(' ');
            }
        }
        text.push_str(&run.text);
        end = run.end_x;
        if text.len() > MAX_TEXT_BYTES {
            return Err("PDF text exceeds the review limit".into());
        }
    }
    Ok(text)
}

fn bounded_streams(document: &mut Document) -> Result<()> {
    let mut total = 0_usize;
    for object in document.objects.values_mut() {
        let Object::Stream(stream) = object else {
            continue;
        };
        if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image") {
            continue;
        }
        let decoded = match stream.filters() {
            Ok(filters) if filters == vec![b"FlateDecode".as_slice()] => {
                let mut decoded = Vec::new();
                ZlibDecoder::new(stream.content.as_slice())
                    .take(MAX_STREAM_BYTES as u64 + 1)
                    .read_to_end(&mut decoded)
                    .map_err(|_| "Could not decompress PDF text")?;
                decoded
            }
            Ok(filters) if filters.is_empty() => stream.content.clone(),
            Err(_) if !stream.dict.has(b"Filter") => stream.content.clone(),
            _ => return Err("This PDF uses an unsupported text filter. Try local OCR.".into()),
        };
        total = total.saturating_add(decoded.len());
        if decoded.len() > MAX_STREAM_BYTES || total > MAX_DECODED_BYTES {
            return Err("PDF text streams exceed the local extraction limit".into());
        }
        stream.set_plain_content(decoded);
    }
    Ok(())
}

/// Imported statements are literal data even when a description begins '='.
pub fn literal(text: &str) -> String {
    if text.starts_with('\'') || text.trim_start().starts_with('=') {
        format!("'{text}")
    } else {
        text.to_owned()
    }
}

/// Keep identifiers and reviewed source text as text in the receiving sheet.
pub fn literal_text(text: &str) -> String {
    let trimmed = text.trim();
    if crate::spreadsheet::parse_number(text).is_some()
        || crate::spreadsheet::parse_date(text).is_some()
        || trimmed.eq_ignore_ascii_case("TRUE")
        || trimmed.eq_ignore_ascii_case("FALSE")
    {
        format!("'{text}")
    } else {
        literal(text)
    }
}

/// Conservative column boundaries: tabs or at least two spaces. A single space
/// stays in the description; no amount or transaction sign is invented.
pub fn split_columns(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut spaces = 0_usize;
    for character in line.chars().chain(std::iter::once('\t')) {
        if character == ' ' {
            spaces += 1;
            continue;
        }
        if character == '\t' || spaces >= 2 {
            if !field.trim().is_empty() {
                fields.push(field.trim().to_owned());
            }
            field.clear();
        } else if spaces == 1 && !field.is_empty() {
            field.push(' ');
        }
        spaces = 0;
        if character != '\t' {
            field.push(character);
        }
    }
    if fields.is_empty() {
        vec![line.trim().to_owned()]
    } else {
        fields
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DateOrder {
    Mdy,
    Dmy,
    Ymd,
}

/// Normalize an explicit full date. Missing and two-digit years remain review
/// items instead of being silently supplied from the current clock.
pub fn parse_date(text: &str, order: &DateOrder) -> Option<String> {
    let text = text.trim();
    if text.len() == 10 && text.as_bytes().get(4) == Some(&b'-') {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d") {
            return Some(date.to_string());
        }
    }
    let parts: Vec<_> = text.split(['/', '-', '.']).collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    let (year, month, day) = match order {
        DateOrder::Mdy if parts[2].len() == 4 => (parts[2], parts[0], parts[1]),
        DateOrder::Dmy if parts[2].len() == 4 => (parts[2], parts[1], parts[0]),
        DateOrder::Ymd if parts[0].len() == 4 => (parts[0], parts[1], parts[2]),
        _ => return None,
    };
    chrono::NaiveDate::from_ymd_opt(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?)
        .map(|date| date.to_string())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecimalMark {
    Dot,
    Comma,
}

/// Parse explicit statement numbers, including thousands separators and
/// parenthesized negatives. DR/CR labels are not interpreted as invented signs.
pub fn parse_amount(text: &str, mark: DecimalMark) -> Option<String> {
    let mut text = text.trim();
    if text.len() > 4096 {
        return None;
    }
    let parentheses = text.starts_with('(') && text.ends_with(')');
    if parentheses {
        text = text.get(1..text.len() - 1)?.trim();
    }
    let mut negative = text.starts_with('-');
    let mut explicit_sign = text.starts_with(['-', '+']);
    text = text.strip_prefix(['-', '+']).unwrap_or(text).trim();
    for prefix in ["USD", "EUR", "GBP", "CAD", "AUD", "$", "€", "£", "¥"] {
        if let Some(value) = text.strip_prefix(prefix) {
            text = value.trim();
            break;
        }
    }
    for suffix in ["USD", "EUR", "GBP", "CAD", "AUD", "$", "€", "£", "¥"] {
        if let Some(value) = text.strip_suffix(suffix) {
            text = value.trim();
            break;
        }
    }
    if text.starts_with(['-', '+']) {
        if explicit_sign {
            return None;
        }
        negative = text.starts_with('-');
        explicit_sign = true;
        text = text.strip_prefix(['-', '+'])?.trim();
    }
    if text.is_empty() || (parentheses && explicit_sign) {
        return None;
    }
    let (decimal, thousands) = match mark {
        DecimalMark::Dot => ('.', ','),
        DecimalMark::Comma => (',', '.'),
    };
    let mut pieces = text.split(decimal);
    let whole = pieces.next()?;
    let fraction = pieces.next();
    if pieces.next().is_some()
        || fraction.is_some_and(|value| {
            value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return None;
    }
    let whole = if whole.is_empty() && fraction.is_some() {
        "0"
    } else {
        whole
    };
    let groups: Vec<_> = whole.split(thousands).collect();
    if groups
        .iter()
        .any(|group| group.is_empty() || !group.bytes().all(|byte| byte.is_ascii_digit()))
        || (groups.len() > 1
            && (groups[0].len() > 3 || groups[1..].iter().any(|group| group.len() != 3)))
    {
        return None;
    }
    let mut number = groups.concat();
    if let Some(fraction) = fraction {
        number.push('.');
        number.push_str(fraction);
    }
    if negative || parentheses {
        number.insert(0, '-');
    }
    let value: f64 = number.parse().ok()?;
    value.is_finite().then(|| value.to_string())
}

fn looks_like_date(text: &str) -> bool {
    let pieces: Vec<_> = text.split(['/', '-', '.']).collect();
    (pieces.len() == 2 || pieces.len() == 3)
        && pieces
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && pieces[0].len() <= 4
}

/// Split OCR transaction lines while preserving all trailing amounts as
/// separate columns. The review UI, not this parser, chooses which is a debit,
/// credit, transaction amount, or account balance.
pub fn split_financial(line: &str) -> Vec<String> {
    let columns = split_columns(line);
    if columns.len() > 1 {
        return columns;
    }
    let words: Vec<_> = line.split_whitespace().collect();
    if words.len() < 3 || !looks_like_date(words[0]) {
        return columns;
    }
    let mut first_text = 1;
    while first_text < words.len() && looks_like_date(words[first_text]) {
        first_text += 1;
    }
    let mut first_amount = words.len();
    while first_amount > first_text {
        let value = words[first_amount - 1];
        if parse_amount(value, DecimalMark::Dot).is_none()
            && parse_amount(value, DecimalMark::Comma).is_none()
        {
            break;
        }
        first_amount -= 1;
    }
    if first_amount == words.len() || first_amount == first_text {
        return columns;
    }
    let mut result: Vec<_> = words[..first_text]
        .iter()
        .map(|value| (*value).to_owned())
        .collect();
    result.push(words[first_text..first_amount].join(" "));
    result.extend(
        words[first_amount..]
            .iter()
            .map(|value| (*value).to_owned()),
    );
    result
}

pub fn duplicate_key(fields: &[String]) -> String {
    fields
        .iter()
        .map(|field| {
            field
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        })
        .collect::<Vec<_>>()
        .join("\u{1f}")
}
