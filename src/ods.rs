use super::DocumentImport;
use crate::{
    charts::{self, ChartSource},
    model::State,
    sheet_formula::{self, BinaryOp, Expr, Reference, UnaryOp},
    spreadsheet::{
        CellAddr, CellBorders, CellFont, CellRange, CellStyle, CellValue, NumberFormat, Sheet,
        Workbook, MAX_CELLS, MAX_CELL_BYTES, MAX_COLS, MAX_RAW_BYTES, MAX_ROWS, MAX_SHEETS,
    },
    Result,
};
use chrono::NaiveDate;
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    io::{Cursor, Read, Write},
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const MIME: &str = "application/vnd.oasis.opendocument.spreadsheet";
const MAX_ZIP_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
const MAX_XML_EVENTS: usize = 500_000;
const FONT_FACES: &str = "<office:font-face-decls><style:font-face style:name=\"JigMono\" svg:font-family=\"DejaVu Sans Mono\"/><style:font-face style:name=\"JigSans\" svg:font-family=\"DejaVu Sans\"/><style:font-face style:name=\"JigSerif\" svg:font-family=\"DejaVu Serif\"/></office:font-face-decls>";
const NS: &str = "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" xmlns:number=\"urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" xmlns:of=\"urn:oasis:names:tc:opendocument:xmlns:of:1.2\" xmlns:jig=\"https://biraz-dev.github.io/Jig/document/1\"";

#[derive(Serialize, Deserialize)]
struct JigMetadata {
    version: u32,
    content_hash: String,
    state: State,
}

fn escape(text: &str) -> String {
    quick_xml::escape::escape(text).into_owned()
}

fn hash(bytes: &[u8]) -> String {
    let mut value = 0xcbf29ce484222325_u64;
    for byte in bytes {
        value = (value ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    format!("{value:016x}")
}

fn color(color: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
}
fn parse_color(text: &str) -> Option<[u8; 3]> {
    let value = text.strip_prefix('#')?;
    if value.len() != 6 || !value.is_ascii() {
        return None;
    }
    Some([
        u8::from_str_radix(&value[0..2], 16).ok()?,
        u8::from_str_radix(&value[2..4], 16).ok()?,
        u8::from_str_radix(&value[4..6], 16).ok()?,
    ])
}

fn reference(reference: &Reference) -> String {
    let mut cell = CellAddr::new(reference.row, reference.col).label();
    if cell == "#REF!" {
        return cell;
    }
    let split = cell
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(cell.len());
    if reference.row_absolute {
        cell.insert(split, '$');
    }
    if reference.col_absolute {
        cell.insert(0, '$');
    }
    match &reference.sheet {
        Some(sheet) => format!("'{}'.{cell}", sheet.replace('\'', "''")),
        None => format!(".{cell}"),
    }
}

fn expression(expr: &Expr) -> String {
    match expr {
        Expr::Number(number) => number.to_string(),
        Expr::Text(text) => format!("\"{}\"", text.replace('"', "\"\"")),
        Expr::Bool(value) => if *value { "TRUE()" } else { "FALSE()" }.into(),
        Expr::Error(error) => error.code().into(),
        Expr::Ref(cell) => format!("[{}]", reference(cell)),
        Expr::Range(start, end) => format!("[{}:{}]", reference(start), reference(end)),
        Expr::Unary(op, value) => match op {
            UnaryOp::Plus => format!("+({})", expression(value)),
            UnaryOp::Minus => format!("-({})", expression(value)),
            UnaryOp::Percent => format!("({})%", expression(value)),
        },
        Expr::Binary(op, left, right) => {
            let op = match op {
                BinaryOp::Add => "+",
                BinaryOp::Subtract => "-",
                BinaryOp::Multiply => "*",
                BinaryOp::Divide => "/",
                BinaryOp::Power => "^",
                BinaryOp::Equal => "=",
                BinaryOp::NotEqual => "<>",
                BinaryOp::Less => "<",
                BinaryOp::LessEqual => "<=",
                BinaryOp::Greater => ">",
                BinaryOp::GreaterEqual => ">=",
            };
            format!("({}{}{})", expression(left), op, expression(right))
        }
        Expr::Call(name, args) => format!(
            "{}({})",
            name,
            args.iter().map(expression).collect::<Vec<_>>().join(";")
        ),
    }
}

fn to_odf(raw: &str) -> String {
    match sheet_formula::parse(raw) {
        Ok(expr) => format!("of:={}", expression(&expr)),
        Err(_) => "of:=#VALUE!".into(),
    }
}

fn from_odf(raw: &str) -> Result<String> {
    let raw = raw
        .strip_prefix("of:=")
        .or_else(|| raw.strip_prefix("oooc:="))
        .or_else(|| raw.strip_prefix('='))
        .ok_or("Unsupported ODS formula namespace")?;
    let mut output = String::from("=");
    let chars: Vec<char> = raw.chars().collect();
    let mut index = 0;
    let mut quoted = false;
    while index < chars.len() {
        let current = chars[index];
        if current == '"' {
            output.push(current);
            if quoted && chars.get(index + 1) == Some(&'"') {
                output.push('"');
                index += 2;
                continue;
            }
            quoted = !quoted;
        } else if !quoted && current == '[' {
            let start = index + 1;
            let mut end = start;
            let mut sheet_quote = false;
            while end < chars.len() {
                if chars[end] == '\'' {
                    if sheet_quote && chars.get(end + 1) == Some(&'\'') {
                        end += 2;
                        continue;
                    }
                    sheet_quote = !sheet_quote;
                }
                if chars[end] == ']' && !sheet_quote {
                    break;
                }
                end += 1;
            }
            if end == chars.len() {
                return Err("Invalid ODS formula reference".into());
            }
            let content: String = chars[start..end].iter().collect();
            output.push_str(&odf_reference(&content)?);
            index = end;
        } else if !quoted && current == ';' {
            output.push(',');
        } else {
            output.push(current);
        }
        index += 1;
    }
    Ok(output)
}

fn odf_reference(text: &str) -> Result<String> {
    if text.contains("#REF!") {
        return Ok("#REF!".into());
    }
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\'' {
            if quoted && bytes.get(index + 1) == Some(&b'\'') {
                index += 2;
                continue;
            }
            quoted = !quoted;
        } else if bytes[index] == b':' && !quoted {
            parts.push(&text[start..index]);
            start = index + 1;
        }
        index += 1;
    }
    parts.push(&text[start..]);
    if parts.len() > 2 {
        return Err("Invalid ODS range".into());
    }
    let mut output = Vec::new();
    for part in parts {
        let mut dot = None;
        let mut quoted = false;
        let mut iter = part.char_indices().peekable();
        while let Some((offset, c)) = iter.next() {
            if c == '\'' {
                if quoted && iter.peek().is_some_and(|(_, next)| *next == '\'') {
                    iter.next();
                    continue;
                }
                quoted = !quoted;
            } else if c == '.' && !quoted {
                dot = Some(offset);
            }
        }
        let (sheet, cell) = if let Some(dot) = dot {
            (&part[..dot], &part[dot + 1..])
        } else {
            ("", part)
        };
        if CellAddr::parse(cell).is_err() {
            return Ok("#REF!".into());
        }
        if sheet.is_empty() {
            output.push(cell.to_owned());
        } else {
            let sheet = sheet.strip_prefix('$').unwrap_or(sheet);
            if sheet.contains('/') || sheet.contains('#') || sheet.contains('[') {
                return Ok("#REF!".into());
            }
            output.push(format!("{sheet}!{cell}"));
        }
    }
    Ok(output.join(":"))
}

fn number_styles(xml: &mut String, style: &CellStyle, index: usize, decimals: usize) {
    let name = format!("n{index}");
    match &style.number_format {
        NumberFormat::General => {}
        NumberFormat::Number => {
            let _ = write!(xml,"<number:number-style style:name=\"{name}\"><number:number number:decimal-places=\"{decimals}\" number:min-integer-digits=\"1\"/></number:number-style>");
        }
        NumberFormat::Percent => {
            let _ = write!(xml,"<number:percentage-style style:name=\"{name}\"><number:number number:decimal-places=\"{decimals}\" number:min-integer-digits=\"1\"/><number:text>%</number:text></number:percentage-style>");
        }
        NumberFormat::Currency(code) => {
            let _ = write!(xml,"<number:currency-style style:name=\"{name}\"><number:currency-symbol>{}</number:currency-symbol><number:number number:decimal-places=\"{decimals}\" number:min-integer-digits=\"1\"/></number:currency-style>",escape(&format!("{code} ")));
        }
        NumberFormat::Date => {
            let _ = write!(xml,"<number:date-style style:name=\"{name}\"><number:year number:style=\"long\"/><number:text>-</number:text><number:month number:style=\"long\"/><number:text>-</number:text><number:day number:style=\"long\"/></number:date-style>");
        }
    }
}

fn style_xml(xml: &mut String, style: &CellStyle, index: usize) {
    let family = match style.font_family {
        CellFont::Mono => "JigMono",
        CellFont::Sans => "JigSans",
        CellFont::Serif => "JigSerif",
    };
    let data = if style.number_format == NumberFormat::General {
        String::new()
    } else {
        format!(" style:data-style-name=\"n{index}\"")
    };
    let _ = write!(xml,"<style:style style:name=\"c{index}\" style:family=\"table-cell\"{data}><style:text-properties style:font-name=\"{family}\" fo:font-size=\"{}pt\" fo:font-weight=\"{}\" fo:font-style=\"{}\"",style.font_size,if style.bold{"bold"}else{"normal"},if style.italic{"italic"}else{"normal"});
    if style.underline {
        xml.push_str(" style:text-underline-style=\"solid\" style:text-underline-width=\"auto\"");
    }
    if let Some(value) = style.text_color {
        let _ = write!(xml, " fo:color=\"{}\"", color(value));
    }
    xml.push_str("/><style:table-cell-properties");
    if let Some(value) = style.fill_color {
        let _ = write!(xml, " fo:background-color=\"{}\"", color(value));
    }
    for (name, enabled) in [
        ("top", style.borders.top),
        ("right", style.borders.right),
        ("bottom", style.borders.bottom),
        ("left", style.borders.left),
    ] {
        let _ = write!(
            xml,
            " fo:border-{name}=\"{}\"",
            if enabled {
                "0.5pt solid #4b5563"
            } else {
                "none"
            }
        );
    }
    xml.push_str("/></style:style>");
}

fn cell_xml(
    xml: &mut String,
    raw: &str,
    value: CellValue,
    style: &CellStyle,
    style_index: Option<usize>,
    merge: Option<CellRange>,
    decimals: usize,
) {
    xml.push_str("<table:table-cell");
    if let Some(index) = style_index {
        let _ = write!(xml, " table:style-name=\"c{index}\"");
    }
    if let Some(range) = merge {
        let _ = write!(
            xml,
            " table:number-columns-spanned=\"{}\" table:number-rows-spanned=\"{}\"",
            range.cols(),
            range.rows()
        );
    }
    if raw.starts_with('=') {
        let _ = write!(xml, " table:formula=\"{}\"", escape(&to_odf(raw)));
    }
    match &value {
        CellValue::Blank => {}
        CellValue::Number(number) => {
            if let Some(date) = (style.number_format == NumberFormat::Date)
                .then(|| value.as_date())
                .flatten()
            {
                let _ = write!(
                    xml,
                    " office:value-type=\"date\" office:date-value=\"{date}\""
                );
            } else {
                let kind = match style.number_format {
                    NumberFormat::Percent => "percentage",
                    NumberFormat::Currency(_) => "currency",
                    _ => "float",
                };
                let _ = write!(
                    xml,
                    " office:value-type=\"{kind}\" office:value=\"{number}\""
                );
                if let NumberFormat::Currency(code) = &style.number_format {
                    let _ = write!(xml, " office:currency=\"{}\"", escape(code));
                }
            }
        }
        CellValue::Date(date) => {
            let _ = write!(
                xml,
                " office:value-type=\"date\" office:date-value=\"{date}\""
            );
        }
        CellValue::Bool(value) => {
            let _ = write!(
                xml,
                " office:value-type=\"boolean\" office:boolean-value=\"{value}\""
            );
        }
        CellValue::Text(_) | CellValue::Error(_) => xml.push_str(" office:value-type=\"string\""),
    }
    xml.push('>');
    let displayed = value.formatted(&style.number_format, decimals);
    if !displayed.is_empty() {
        for line in displayed.split('\n') {
            let _ = write!(xml, "<text:p>{}</text:p>", paragraph(line));
        }
    }
    xml.push_str("</table:table-cell>");
}

fn paragraph(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == ' ' {
            let mut count = 1;
            while chars.peek() == Some(&' ') {
                chars.next();
                count += 1;
            }
            let _ = write!(result, "<text:s text:c=\"{count}\"/>");
        } else if ch == '\t' {
            result.push_str("<text:tab/>");
        } else {
            result.push_str(&escape(&ch.to_string()));
        }
    }
    result
}

pub fn save_ods(state: &State) -> Result<Vec<u8>> {
    state.validate()?;
    let empty = Workbook::default();
    let book = state.workbook.as_ref().unwrap_or(&empty);
    let mut styles = Vec::<CellStyle>::new();
    for sheet in &book.sheets {
        for style in sheet.styles.values() {
            if !styles.contains(style) {
                styles.push(style.clone());
            }
        }
    }
    let mut content=format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content {NS} office:version=\"1.3\">{FONT_FACES}<office:automatic-styles>");
    for (index, style) in styles.iter().enumerate() {
        number_styles(&mut content, style, index, state.preferences.decimals);
        style_xml(&mut content, style, index);
    }
    content.push_str("</office:automatic-styles><office:body><office:spreadsheet>");
    let today = chrono::Local::now().date_naive();
    let values = book.recalculate_at(today);
    let mut pictures = Vec::<(String, String)>::new();
    for sheet in &book.sheets {
        let _ = write!(
            content,
            "<table:table table:name=\"{}\" jig:id=\"{}\">",
            escape(&sheet.name),
            sheet.id
        );
        let mut end = book
            .used_range(sheet.id)
            .map(|range| range.end)
            .unwrap_or(CellAddr::new(0, 0));
        for address in sheet.styles.keys() {
            let addr = CellAddr::parse(address)?;
            end.row = end.row.max(addr.row);
            end.col = end.col.max(addr.col);
        }
        for merge in &sheet.merges {
            end.row = end.row.max(merge.end.row);
            end.col = end.col.max(merge.end.col);
        }
        let mut shapes = String::new();
        let mut chart_offset = 0.0;
        for spec in &book.charts {
            let relevant = match spec.source {
                ChartSource::Range { sheet: id, .. } => id == sheet.id,
                ChartSource::Tasks => book
                    .sheets
                    .first()
                    .is_some_and(|first| first.id == sheet.id),
            };
            if !relevant {
                continue;
            }
            if let Ok(scene) =
                charts::scene_at(state, spec, 720.0, 360.0, true, &chrono::Local, today)
            {
                let name = format!("Pictures/chart-{}.svg", spec.id);
                pictures.push((name.clone(), charts::to_svg(&scene)));
                let height = scene.height / 28.346;
                let y = (end.row + 2) as f64 * 0.55 + chart_offset;
                let _ = write!(shapes, "<draw:frame draw:name=\"{}\" svg:width=\"25.4cm\" svg:height=\"{height}cm\" svg:x=\"0cm\" svg:y=\"{y}cm\"><draw:image xlink:href=\"{}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/></draw:frame>", escape(&spec.title), escape(&name));
                chart_offset += height + 0.6;
            }
        }
        if !shapes.is_empty() {
            content.push_str("<table:shapes>");
            content.push_str(&shapes);
            content.push_str("</table:shapes>");
        }
        content.push_str("<table:table-column table:number-columns-repeated=\"52\"/>");
        for row in 0..=end.row {
            content.push_str("<table:table-row>");
            for col in 0..=end.col {
                let addr = CellAddr::new(row, col);
                let merge = sheet.merges.iter().find(|range| range.contains(addr));
                if merge.is_some_and(|range| range.start != addr) {
                    content.push_str("<table:covered-table-cell/>");
                    continue;
                }
                let style = book.style(sheet.id, addr);
                let style_index = sheet
                    .styles
                    .get(&addr.label())
                    .and_then(|value| styles.iter().position(|style| style == value));
                cell_xml(
                    &mut content,
                    book.raw(sheet.id, addr),
                    values.value(sheet.id, addr),
                    &style,
                    style_index,
                    merge.copied(),
                    state.preferences.decimals,
                );
            }
            content.push_str("</table:table-row>");
        }
        content.push_str("</table:table>");
    }
    content.push_str("</office:spreadsheet></office:body></office:document-content>");
    if content.len() > MAX_ENTRY_BYTES as usize {
        return Err("ODS document exceeds the supported export size".into());
    }
    let metadata = serde_json::to_vec(&JigMetadata {
        version: 1,
        content_hash: hash(content.as_bytes()),
        state: state.clone(),
    })
    .map_err(|error| error.to_string())?;
    let styles_xml=format!("<?xml version=\"1.0\"?><office:document-styles {NS} office:version=\"1.3\">{FONT_FACES}<office:styles><style:default-style style:family=\"table-cell\"><style:text-properties style:font-name=\"JigMono\" fo:font-size=\"13pt\"/></style:default-style></office:styles></office:document-styles>");
    let mut manifest=String::from("<?xml version=\"1.0\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"application/vnd.oasis.opendocument.spreadsheet\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"jig/state.json\" manifest:media-type=\"application/json\"/>");
    for (name, _) in &pictures {
        let _=write!(manifest,"<manifest:file-entry manifest:full-path=\"{}\" manifest:media-type=\"image/svg+xml\"/>",escape(name));
    }
    manifest.push_str("</manifest:manifest>");
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "mimetype",
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
    )
    .map_err(|e| e.to_string())?;
    zip.write_all(MIME.as_bytes()).map_err(|e| e.to_string())?;
    let compressed = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in [
        ("content.xml", content.as_bytes()),
        ("styles.xml", styles_xml.as_bytes()),
        ("META-INF/manifest.xml", manifest.as_bytes()),
        ("jig/state.json", metadata.as_slice()),
    ] {
        zip.start_file(name, compressed)
            .map_err(|e| e.to_string())?;
        zip.write_all(bytes).map_err(|e| e.to_string())?;
    }
    for (name, svg) in pictures {
        zip.start_file(name, compressed)
            .map_err(|e| e.to_string())?;
        zip.write_all(svg.as_bytes()).map_err(|e| e.to_string())?;
    }
    let bytes = zip.finish().map_err(|e| e.to_string())?.into_inner();
    if bytes.len() > MAX_ZIP_BYTES {
        return Err("ODS export exceeds its file limit".into());
    }
    Ok(bytes)
}

fn entry(
    zip: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
    required: bool,
) -> Result<Option<Vec<u8>>> {
    let mut file = match zip.by_name(name) {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) if !required => return Ok(None),
        Err(_) => return Err(format!("ODS is missing or cannot read {name}")),
    };
    if file.size() > MAX_ENTRY_BYTES {
        return Err(format!("ODS entry {name} exceeds the supported size"));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_ENTRY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_ENTRY_BYTES {
        return Err("ODS entry exceeds the supported size".into());
    }
    Ok(Some(bytes))
}

pub fn read_ods(bytes: &[u8]) -> Result<DocumentImport> {
    if bytes.len() > MAX_ZIP_BYTES {
        return Err("ODS exceeds the 16 MiB file limit".into());
    }
    let mut zip = ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| "This is not a readable ODS ZIP document")?;
    if zip.len() > 256 {
        return Err("ODS has too many package entries".into());
    }
    let mut total = 0u64;
    for index in 0..zip.len() {
        let file = zip.by_index(index).map_err(|e| e.to_string())?;
        total = total.checked_add(file.size()).ok_or("ODS is too large")?;
        if file.size() > MAX_ENTRY_BYTES || total > MAX_TOTAL_BYTES {
            return Err("ODS package expands beyond the supported size".into());
        }
    }
    if entry(&mut zip, "mimetype", true)?.as_deref() != Some(MIME.as_bytes()) {
        return Err("File is not an OpenDocument Spreadsheet".into());
    }
    let content = entry(&mut zip, "content.xml", true)?.ok_or("ODS has no sheet content")?;
    validate_xml(&content)?;
    let external_styles = entry(&mut zip, "styles.xml", false)?;
    let mut styles = Styles::default();
    if let Some(bytes) = external_styles {
        validate_xml(&bytes)?;
        styles.parse(&bytes)?;
    }
    styles.parse(&content)?;
    let mut workbook = parse_sheets(&content, &styles)?;
    let mut embedded_state = None;
    if let Some(metadata) = entry(&mut zip, "jig/state.json", false)? {
        let metadata: JigMetadata =
            serde_json::from_slice(&metadata).map_err(|_| "Invalid embedded Jig document data")?;
        if metadata.version != 1 {
            return Err("Unsupported embedded Jig document version".into());
        }
        metadata.state.validate()?;
        let mut state = metadata.state;
        if metadata.content_hash == hash(&content) {
            workbook = state.workbook.clone().unwrap_or_default();
        } else {
            // External spreadsheet editors update the standard grid, not Jig's
            // extension. Keep their visible changes and retain the task data.
            if let Some(old) = &state.workbook {
                for chart in &old.charts {
                    let mut chart = chart.clone();
                    if let ChartSource::Range { sheet, .. } = &mut chart.source {
                        let Some(name) = old.sheet(*sheet).map(|item| &item.name) else {
                            continue;
                        };
                        let Some(new) = workbook.sheets.iter().find(|item| item.name == *name)
                        else {
                            continue;
                        };
                        *sheet = new.id;
                    }
                    if chart.validate(&workbook).is_ok() {
                        workbook.charts.push(chart);
                    }
                }
            }
            state.workbook = Some(workbook.clone());
        }
        state.validate()?;
        embedded_state = Some(state);
    }
    workbook.validate()?;
    Ok(DocumentImport {
        workbook,
        embedded_state,
    })
}

fn local(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

fn validate_xml(bytes: &[u8]) -> Result<()> {
    let mut reader = Reader::from_reader(bytes);
    let mut depth = 0_usize;
    let mut count = 0_usize;
    loop {
        count += 1;
        if count > MAX_XML_EVENTS {
            return Err("ODS XML exceeds its event limit".into());
        }
        match reader.read_event().map_err(|_| "Invalid ODS XML")? {
            Event::Start(_) => {
                depth += 1;
                if depth > 128 {
                    return Err("ODS XML nesting exceeds its limit".into());
                }
            }
            Event::End(_) => depth = depth.checked_sub(1).ok_or("Unbalanced ODS XML")?,
            Event::DocType(_) => {
                return Err("ODS documents with DTD declarations are not supported".into())
            }
            Event::GeneralRef(event) => {
                entity_value(&event)?;
            }
            Event::Eof => {
                return if depth == 0 {
                    Ok(())
                } else {
                    Err("Incomplete ODS XML".into())
                }
            }
            _ => {}
        }
    }
}
fn attributes(event: &BytesStart<'_>, reader: &Reader<&[u8]>) -> Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    for attr in event.attributes() {
        let attr = attr.map_err(|_| "Invalid ODS XML attribute")?;
        let key = String::from_utf8_lossy(local(attr.key.as_ref())).into_owned();
        let value = attr
            .decode_and_unescape_value(reader.decoder())
            .map_err(|_| "Invalid ODS XML text")?
            .into_owned();
        result.insert(key, value);
    }
    Ok(result)
}
fn repeat(attrs: &BTreeMap<String, String>, key: &str) -> Result<usize> {
    match attrs.get(key) {
        None => Ok(1),
        Some(value) => {
            let n = value
                .parse::<usize>()
                .map_err(|_| "Invalid ODS repeated row or column count")?;
            if n == 0 || n > 10_000_000 {
                Err("ODS repeated row or column count is outside its limit".into())
            } else {
                Ok(n)
            }
        }
    }
}
fn text_value(event: &quick_xml::events::BytesText<'_>) -> Result<String> {
    let decoded = event
        .decode()
        .map_err(|_| "ODS XML must use readable text")?;
    quick_xml::escape::unescape(&decoded)
        .map(|value| value.into_owned())
        .map_err(|_| "Unsupported XML entity in ODS".to_owned())
}
fn entity_value(event: &quick_xml::events::BytesRef<'_>) -> Result<String> {
    let name = event.decode().map_err(|_| "Invalid ODS XML entity")?;
    quick_xml::escape::unescape(&format!("&{name};"))
        .map(|value| value.into_owned())
        .map_err(|_| "Unsupported XML entity in ODS".to_owned())
}

#[derive(Clone, Default)]
struct StyleDef {
    style: CellStyle,
    parent: Option<String>,
    format: Option<String>,
}
#[derive(Default)]
struct Styles {
    definitions: BTreeMap<String, StyleDef>,
    formats: BTreeMap<String, NumberFormat>,
    fonts: BTreeMap<String, CellFont>,
}
impl Styles {
    fn parse(&mut self, bytes: &[u8]) -> Result<()> {
        let mut reader = Reader::from_reader(bytes);
        let mut current: Option<(String, StyleDef)> = None;
        let mut format: Option<(String, NumberFormat)> = None;
        let mut currency = String::new();
        let mut in_currency = false;
        let mut depth = 0;
        let mut events = 0;
        loop {
            events += 1;
            if events > MAX_XML_EVENTS {
                return Err("ODS XML exceeds its event limit".into());
            }
            match reader.read_event().map_err(|_| "Invalid ODS XML")? {
                Event::Start(event) | Event::Empty(event) => {
                    let tag = local(event.name().as_ref()).to_vec();
                    let a = attributes(&event, &reader)?;
                    match tag.as_slice() {
                        b"font-face" => {
                            if let (Some(name), Some(family)) =
                                (a.get("name"), a.get("font-family"))
                            {
                                self.fonts.insert(name.clone(), font_family(family));
                            }
                        }
                        b"style" if a.get("family").is_some_and(|value| value == "table-cell") => {
                            if let Some(name) = a.get("name") {
                                current = Some((
                                    name.clone(),
                                    StyleDef {
                                        style: CellStyle::default(),
                                        parent: a.get("parent-style-name").cloned(),
                                        format: a.get("data-style-name").cloned(),
                                    },
                                ));
                            }
                        }
                        b"text-properties" => {
                            if let Some((_, definition)) = &mut current {
                                let s = &mut definition.style;
                                if let Some(value) =
                                    a.get("font-name").or_else(|| a.get("font-family"))
                                {
                                    s.font_family = self
                                        .fonts
                                        .get(value)
                                        .copied()
                                        .unwrap_or_else(|| font_family(value));
                                }
                                if let Some(value) = a.get("font-size") {
                                    let number =
                                        value.trim_end_matches("pt").parse::<f64>().unwrap_or(13.0);
                                    s.font_size = number.round().clamp(9.0, 32.0) as u8;
                                }
                                s.bold = a
                                    .get("font-weight")
                                    .is_some_and(|v| v == "bold" || v == "700");
                                s.italic = a
                                    .get("font-style")
                                    .is_some_and(|v| v == "italic" || v == "oblique");
                                s.underline =
                                    a.get("text-underline-style").is_some_and(|v| v != "none");
                                s.text_color = a.get("color").and_then(|v| parse_color(v));
                            }
                        }
                        b"table-cell-properties" => {
                            if let Some((_, definition)) = &mut current {
                                let s = &mut definition.style;
                                s.fill_color =
                                    a.get("background-color").and_then(|v| parse_color(v));
                                let all = a.get("border").is_some_and(|v| v != "none");
                                s.borders = CellBorders {
                                    top: all || a.get("border-top").is_some_and(|v| v != "none"),
                                    right: all
                                        || a.get("border-right").is_some_and(|v| v != "none"),
                                    bottom: all
                                        || a.get("border-bottom").is_some_and(|v| v != "none"),
                                    left: all || a.get("border-left").is_some_and(|v| v != "none"),
                                };
                            }
                        }
                        b"number-style" | b"percentage-style" | b"currency-style"
                        | b"date-style" => {
                            if let Some(name) = a.get("name") {
                                let kind = match tag.as_slice() {
                                    b"percentage-style" => NumberFormat::Percent,
                                    b"currency-style" => NumberFormat::Currency("USD".into()),
                                    b"date-style" => NumberFormat::Date,
                                    _ => NumberFormat::Number,
                                };
                                format = Some((name.clone(), kind));
                                currency.clear();
                            }
                        }
                        b"currency-symbol" => in_currency = true,
                        _ => {}
                    }
                    // Empty elements have no matching End; style/property data
                    // is nevertheless complete. Limits are checked separately.
                    depth += 1;
                    if depth > MAX_XML_EVENTS {
                        return Err("ODS XML exceeds its limit".into());
                    }
                }
                Event::Text(event) => {
                    if in_currency {
                        currency.push_str(&text_value(&event)?);
                    }
                }
                Event::GeneralRef(event) => {
                    if in_currency {
                        currency.push_str(&entity_value(&event)?);
                    }
                }
                Event::End(event) => {
                    let tag = local(event.name().as_ref()).to_vec();
                    match tag.as_slice() {
                        b"style" => {
                            if let Some((name, style)) = current.take() {
                                self.definitions.insert(name, style);
                            }
                        }
                        b"currency-symbol" => in_currency = false,
                        b"number-style" | b"percentage-style" | b"currency-style"
                        | b"date-style" => {
                            if let Some((name, mut kind)) = format.take() {
                                if matches!(kind, NumberFormat::Currency(_)) {
                                    kind = NumberFormat::Currency(currency_code(&currency));
                                }
                                self.formats.insert(name, kind);
                            }
                        }
                        _ => {}
                    }
                }
                Event::DocType(_) => {
                    return Err("ODS documents with DTD declarations are not supported".into())
                }
                Event::Eof => break,
                _ => {}
            }
        }
        Ok(())
    }
    fn get(&self, name: &str) -> CellStyle {
        let Some(definition) = self.definitions.get(name) else {
            return CellStyle::default();
        };
        let mut style = definition.style.clone();
        if let Some(format) = definition
            .format
            .as_ref()
            .and_then(|name| self.formats.get(name))
        {
            style.number_format = format.clone();
        } else if let Some(parent) = definition
            .parent
            .as_ref()
            .and_then(|name| self.definitions.get(name))
            .and_then(|item| item.format.as_ref())
            .and_then(|name| self.formats.get(name))
        {
            style.number_format = parent.clone();
        }
        style
    }
}
fn font_family(name: &str) -> CellFont {
    let name = name.to_lowercase();
    if name.contains("mono") || name.contains("courier") {
        CellFont::Mono
    } else if name.contains("serif") && !name.contains("sans") || name.contains("times") {
        CellFont::Serif
    } else {
        CellFont::Sans
    }
}
fn currency_code(symbol: &str) -> String {
    let symbol = symbol.trim();
    match symbol {
        "€" => "EUR",
        "£" => "GBP",
        "¥" => "JPY",
        "$" => "USD",
        value if value.len() == 3 && value.chars().all(|c| c.is_ascii_alphabetic()) => value,
        _ => "USD",
    }
    .to_ascii_uppercase()
}

#[derive(Clone)]
struct ParsedCell {
    attrs: BTreeMap<String, String>,
    text: String,
    paragraph: bool,
}
impl ParsedCell {
    fn push(&mut self, text: &str) -> Result<()> {
        if self.text.len() + text.len() > MAX_CELL_BYTES {
            return Err("ODS cell exceeds its text limit".into());
        }
        self.text.push_str(text);
        Ok(())
    }

    fn paragraph(&mut self) -> Result<()> {
        if self.paragraph {
            self.push("\n")?;
        }
        self.paragraph = true;
        Ok(())
    }
}
fn literal(text: String) -> String {
    if text.starts_with('=')
        || text.starts_with('\'')
        || text.parse::<f64>().is_ok()
        || matches!(text.as_str(), "TRUE" | "FALSE" | "true" | "false")
        || NaiveDate::parse_from_str(&text, "%Y-%m-%d").is_ok()
    {
        format!("'{text}")
    } else {
        text
    }
}
fn raw_cell(cell: &ParsedCell) -> Result<String> {
    if let Some(formula) = cell.attrs.get("formula") {
        return from_odf(formula);
    }
    Ok(match cell.attrs.get("value-type").map(String::as_str) {
        Some("float" | "percentage" | "currency") => {
            let value = cell
                .attrs
                .get("value")
                .ok_or("ODS numeric cell has no value")?;
            if !value.parse::<f64>().is_ok_and(f64::is_finite) {
                return Err("ODS numeric values must be finite".into());
            }
            value.clone()
        }
        Some("boolean") => {
            if cell
                .attrs
                .get("boolean-value")
                .is_some_and(|value| value == "true" || value == "1")
            {
                "TRUE".into()
            } else {
                "FALSE".into()
            }
        }
        Some("date") => {
            let value = cell
                .attrs
                .get("date-value")
                .ok_or("ODS date cell has no date")?;
            let date = value.get(..10).ok_or("ODS date is invalid")?;
            NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| "ODS date is invalid")?;
            date.into()
        }
        _ => literal(
            cell.attrs
                .get("string-value")
                .cloned()
                .unwrap_or_else(|| cell.text.clone()),
        ),
    })
}

fn parse_sheets(bytes: &[u8], styles: &Styles) -> Result<Workbook> {
    let mut reader = Reader::from_reader(bytes);
    let mut book = Workbook::default();
    book.sheets.clear();
    let mut sheet: Option<Sheet> = None;
    let mut row = 0usize;
    let mut row_repeat = 1;
    let mut col = 0usize;
    let mut row_cells = Vec::<(usize, usize, ParsedCell)>::new();
    let mut cell: Option<ParsedCell> = None;
    let mut cells = 0usize;
    let mut merges = 0usize;
    let mut raw_bytes = 0usize;
    let mut event_count = 0;
    let mut text_depth = 0usize;
    loop {
        event_count += 1;
        if event_count > MAX_XML_EVENTS {
            return Err("ODS contains too many XML events".into());
        }
        match reader.read_event().map_err(|_| "Invalid ODS sheet XML")? {
            Event::Start(event) => {
                let tag = local(event.name().as_ref()).to_vec();
                let a = attributes(&event, &reader)?;
                match tag.as_slice() {
                    b"table" => {
                        if sheet.is_some() {
                            return Err("Nested ODS tables are not supported".into());
                        }
                        if book.sheets.len() >= MAX_SHEETS {
                            return Err("ODS contains too many sheets".into());
                        }
                        let mut value = Workbook::default().sheets.remove(0);
                        value.id = book.sheets.len() as u64 + 1;
                        value.name = a
                            .get("name")
                            .cloned()
                            .unwrap_or_else(|| format!("Sheet {}", value.id));
                        value.cells.clear();
                        value.styles.clear();
                        value.merges.clear();
                        sheet = Some(value);
                        row = 0;
                    }
                    b"table-row" if sheet.is_some() => {
                        row_repeat = repeat(&a, "number-rows-repeated")?;
                        col = 0;
                        row_cells.clear();
                    }
                    b"table-cell" if sheet.is_some() => {
                        cell = Some(ParsedCell {
                            attrs: a,
                            text: String::new(),
                            paragraph: false,
                        });
                    }
                    b"covered-table-cell" if sheet.is_some() => {
                        col = col
                            .checked_add(repeat(&a, "number-columns-repeated")?)
                            .ok_or("ODS column count overflow")?;
                    }
                    b"p" if cell.is_some() => {
                        cell.as_mut().unwrap().paragraph()?;
                        text_depth += 1;
                    }
                    b"s" if cell.is_some() => {
                        let spaces = a
                            .get("c")
                            .and_then(|value| value.parse::<usize>().ok())
                            .unwrap_or(1);
                        if spaces > MAX_CELL_BYTES {
                            return Err("ODS cell contains too much text".into());
                        }
                        cell.as_mut().unwrap().push(&" ".repeat(spaces))?;
                    }
                    b"tab" if cell.is_some() => cell.as_mut().unwrap().push("\t")?,
                    b"line-break" if cell.is_some() => cell.as_mut().unwrap().push("\n")?,
                    _ => {}
                }
            }
            Event::Empty(event) => {
                let tag = local(event.name().as_ref()).to_vec();
                let a = attributes(&event, &reader)?;
                match tag.as_slice() {
                    b"table" if sheet.is_some() => {
                        return Err("Nested ODS tables are not supported".into());
                    }
                    b"table-row" if sheet.is_some() => {
                        row = row
                            .checked_add(repeat(&a, "number-rows-repeated")?)
                            .ok_or("ODS row count overflow")?
                    }
                    b"table-cell" if sheet.is_some() => {
                        let repeat = repeat(&a, "number-columns-repeated")?;
                        row_cells.push((
                            col,
                            repeat,
                            ParsedCell {
                                attrs: a,
                                text: String::new(),
                                paragraph: false,
                            },
                        ));
                        col = col.checked_add(repeat).ok_or("ODS column count overflow")?;
                    }
                    b"covered-table-cell" if sheet.is_some() => {
                        col = col
                            .checked_add(repeat(&a, "number-columns-repeated")?)
                            .ok_or("ODS column count overflow")?
                    }
                    b"s" if cell.is_some() => {
                        let count = a
                            .get("c")
                            .and_then(|value| value.parse::<usize>().ok())
                            .unwrap_or(1);
                        if count > MAX_CELL_BYTES {
                            return Err("ODS cell contains too much text".into());
                        }
                        cell.as_mut().unwrap().push(&" ".repeat(count))?;
                    }
                    b"p" if cell.is_some() => cell.as_mut().unwrap().paragraph()?,
                    b"tab" if cell.is_some() => cell.as_mut().unwrap().push("\t")?,
                    b"line-break" if cell.is_some() => cell.as_mut().unwrap().push("\n")?,
                    _ => {}
                }
            }
            Event::Text(event) => {
                if let Some(cell) = cell.as_mut().filter(|_| text_depth > 0) {
                    let value = text_value(&event)?;
                    cell.push(&value)?;
                }
            }
            Event::GeneralRef(event) => {
                if let Some(cell) = cell.as_mut().filter(|_| text_depth > 0) {
                    cell.push(&entity_value(&event)?)?;
                }
            }
            Event::CData(event) => {
                if let Some(cell) = cell.as_mut().filter(|_| text_depth > 0) {
                    let text = event.decode().map_err(|_| "Invalid ODS text")?;
                    cell.push(&text)?;
                }
            }
            Event::End(event) => {
                let tag = local(event.name().as_ref()).to_vec();
                match tag.as_slice() {
                    b"p" => text_depth = text_depth.saturating_sub(1),
                    b"table-cell" => {
                        if let Some(value) = cell.take() {
                            let count = repeat(&value.attrs, "number-columns-repeated")?;
                            row_cells.push((col, count, value));
                            col = col.checked_add(count).ok_or("ODS column count overflow")?;
                        }
                    }
                    b"table-row" if sheet.is_some() => {
                        let sheet = sheet.as_mut().unwrap();
                        for (column, count, value) in &row_cells {
                            let raw = raw_cell(value)?;
                            let mut style = value
                                .attrs
                                .get("style-name")
                                .map(|name| styles.get(name))
                                .unwrap_or_default();
                            if value
                                .attrs
                                .get("value-type")
                                .is_some_and(|kind| kind == "currency")
                            {
                                style.number_format = NumberFormat::Currency(
                                    value.attrs.get("currency").cloned().unwrap_or_else(|| {
                                        match &style.number_format {
                                            NumberFormat::Currency(code) => code.clone(),
                                            _ => "USD".into(),
                                        }
                                    }),
                                );
                            }
                            if value
                                .attrs
                                .get("value-type")
                                .is_some_and(|kind| kind == "percentage")
                            {
                                style.number_format = NumberFormat::Percent;
                            }
                            if value
                                .attrs
                                .get("value-type")
                                .is_some_and(|kind| kind == "date")
                            {
                                style.number_format = NumberFormat::Date;
                            }
                            let merge_cols = value
                                .attrs
                                .get("number-columns-spanned")
                                .map(|v| v.parse::<usize>())
                                .transpose()
                                .map_err(|_| "Invalid ODS merged column span")?
                                .unwrap_or(1);
                            let merge_rows = value
                                .attrs
                                .get("number-rows-spanned")
                                .map(|v| v.parse::<usize>())
                                .transpose()
                                .map_err(|_| "Invalid ODS merged row span")?
                                .unwrap_or(1);
                            let meaningful = !raw.is_empty()
                                || style != CellStyle::default()
                                || merge_cols > 1
                                || merge_rows > 1;
                            if !meaningful {
                                continue;
                            }
                            let last_row = row
                                .checked_add(row_repeat)
                                .ok_or("ODS row count overflow")?;
                            let last_col = column
                                .checked_add(*count)
                                .ok_or("ODS column count overflow")?;
                            if last_row > MAX_ROWS
                                || last_col > MAX_COLS
                                || merge_cols == 0
                                || merge_rows == 0
                            {
                                return Err(
                                    "ODS content extends beyond Jig's supported grid".into()
                                );
                            }
                            for r in row..last_row {
                                for c in *column..last_col {
                                    let addr = CellAddr::new(r, c);
                                    if !raw.is_empty() || style != CellStyle::default() {
                                        cells += 1;
                                        if cells > MAX_CELLS {
                                            return Err(
                                                "ODS workbook exceeds its cell or style limit"
                                                    .into(),
                                            );
                                        }
                                    }
                                    if !raw.is_empty() {
                                        raw_bytes = raw_bytes
                                            .checked_add(raw.len())
                                            .ok_or("ODS cell text limit overflow")?;
                                        if cells > MAX_CELLS || raw_bytes > MAX_RAW_BYTES {
                                            return Err(
                                                "ODS workbook exceeds its cell or text limit"
                                                    .into(),
                                            );
                                        }
                                        sheet.cells.insert(addr.label(), raw.clone());
                                    }
                                    if style != CellStyle::default() {
                                        sheet.styles.insert(addr.label(), style.clone());
                                    }
                                    if merge_cols > 1 || merge_rows > 1 {
                                        merges += 1;
                                        if merges > 2000 {
                                            return Err(
                                                "ODS workbook has too many merged ranges".into()
                                            );
                                        }
                                        let end = CellAddr::new(
                                            r.checked_add(merge_rows - 1)
                                                .ok_or("ODS merge overflow")?,
                                            c.checked_add(merge_cols - 1)
                                                .ok_or("ODS merge overflow")?,
                                        );
                                        if !end.in_bounds() {
                                            return Err(
                                                "ODS merge extends beyond the supported grid"
                                                    .into(),
                                            );
                                        }
                                        sheet.merges.push(CellRange::new(addr, end));
                                    }
                                }
                            }
                        }
                        row = row
                            .checked_add(row_repeat)
                            .ok_or("ODS row count overflow")?;
                    }
                    b"table" => {
                        if let Some(sheet) = sheet.take() {
                            book.sheets.push(sheet);
                        }
                    }
                    _ => {}
                }
            }
            Event::DocType(_) => {
                return Err("ODS documents with DTD declarations are not supported".into())
            }
            Event::Eof => break,
            _ => {}
        }
    }
    book.validate()?;
    Ok(book)
}
