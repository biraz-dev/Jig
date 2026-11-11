use crate::{
    charts::{self, ChartScene, ChartSource, Primitive, TextAnchor},
    model::State,
    sheet_fonts,
    spreadsheet::{CellAddr, CellFont, CellRange, CellStyle, Workbook},
    Result,
};
use lopdf::{
    content::{Content, Operation},
    dictionary, Dictionary, Document, Object, ObjectId, Stream, StringFormat,
};
use std::collections::BTreeMap;

const PAGE_WIDTH: f64 = 841.89;
const PAGE_HEIGHT: f64 = 595.28;
const MARGIN: f64 = 30.0;
const GRID_TOP: f64 = 537.0;
const GRID_BOTTOM: f64 = 42.0;
const ROW_HEAD: f64 = 28.0;
const COLUMN_WIDTH: f64 = 100.0;
const MAX_PAGES: usize = 200;

struct Font {
    object: ObjectId,
    name: &'static str,
    bytes: &'static [u8],
    face: ttf_parser::Face<'static>,
    used: BTreeMap<u16, char>,
}

impl Font {
    fn new(doc: &mut Document, family: CellFont, name: &'static str) -> Result<Self> {
        let bytes = sheet_fonts::bytes(family);
        let face =
            ttf_parser::Face::parse(bytes, 0).map_err(|_| "Could not load the document font")?;
        Ok(Self {
            object: doc.new_object_id(),
            name,
            bytes,
            face,
            used: BTreeMap::new(),
        })
    }

    fn encode(&mut self, text: &str) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(text.len() * 2);
        for ch in text.chars() {
            let (glyph, actual) = match self.face.glyph_index(ch) {
                Some(glyph) => (glyph, ch),
                None => (
                    self.face.glyph_index('?').unwrap_or(ttf_parser::GlyphId(0)),
                    '?',
                ),
            };
            self.used.entry(glyph.0).or_insert(actual);
            bytes.extend_from_slice(&glyph.0.to_be_bytes());
        }
        bytes
    }

    fn width(&self, text: &str, size: f64) -> f64 {
        let units = f64::from(self.face.units_per_em());
        text.chars()
            .map(|ch| {
                self.face
                    .glyph_index(ch)
                    .or_else(|| self.face.glyph_index('?'))
                    .and_then(|glyph| self.face.glyph_hor_advance(glyph))
                    .map_or(0.0, f64::from)
            })
            .sum::<f64>()
            * size
            / units
    }

    fn finish(self, doc: &mut Document) {
        if self.used.is_empty() {
            return;
        }
        let descriptor = doc.new_object_id();
        let descendant = doc.new_object_id();
        let font_file = doc.add_object(Stream::new(
            dictionary! {"Length1" => self.bytes.len() as i64},
            self.bytes.to_vec(),
        ));
        let units = f64::from(self.face.units_per_em());
        let metric = |value: i16| (f64::from(value) * 1000.0 / units).round() as i64;
        let bbox = self.face.global_bounding_box();
        doc.objects.insert(descriptor, dictionary! {
            "Type"=>"FontDescriptor", "FontName"=>self.name,
            "Flags"=>32, "FontBBox"=>vec![Object::Integer(metric(bbox.x_min)),Object::Integer(metric(bbox.y_min)),Object::Integer(metric(bbox.x_max)),Object::Integer(metric(bbox.y_max))],
            "ItalicAngle"=>0, "Ascent"=>metric(self.face.ascender()), "Descent"=>metric(self.face.descender()),
            "CapHeight"=>metric(self.face.capital_height().unwrap_or(self.face.ascender())), "StemV"=>80,
            "FontFile2"=>font_file,
        }.into());
        let widths: Vec<Object> = self
            .used
            .keys()
            .flat_map(|glyph| {
                let advance = self
                    .face
                    .glyph_hor_advance(ttf_parser::GlyphId(*glyph))
                    .unwrap_or(600);
                [
                    Object::Integer(i64::from(*glyph)),
                    Object::Array(vec![Object::Integer(
                        (f64::from(advance) * 1000.0 / units).round() as i64,
                    )]),
                ]
            })
            .collect();
        doc.objects.insert(descendant, dictionary! {
            "Type"=>"Font", "Subtype"=>"CIDFontType2", "BaseFont"=>self.name,
            "CIDSystemInfo"=>dictionary! {"Registry"=>Object::string_literal("Adobe"),"Ordering"=>Object::string_literal("Identity"),"Supplement"=>0},
            "FontDescriptor"=>descriptor,"DW"=>600,"W"=>widths,"CIDToGIDMap"=>"Identity",
        }.into());
        let mut cmap=String::from("/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Jig-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");
        let glyphs: Vec<_> = self.used.iter().collect();
        for chunk in glyphs.chunks(100) {
            cmap.push_str(&format!("{} beginbfchar\n", chunk.len()));
            for (glyph, ch) in chunk {
                let mut storage = [0u16; 2];
                let utf16 = ch
                    .encode_utf16(&mut storage)
                    .iter()
                    .map(|unit| format!("{unit:04X}"))
                    .collect::<String>();
                cmap.push_str(&format!("<{glyph:04X}> <{utf16}>\n"));
            }
            cmap.push_str("endbfchar\n");
        }
        cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
        let unicode = doc.add_object(Stream::new(Dictionary::new(), cmap.into_bytes()));
        doc.objects.insert(
            self.object,
            dictionary! {
                "Type"=>"Font","Subtype"=>"Type0","BaseFont"=>self.name,"Encoding"=>"Identity-H",
                "DescendantFonts"=>vec![Object::Reference(descendant)],"ToUnicode"=>unicode,
            }
            .into(),
        );
    }
}

struct Pdf {
    document: Document,
    pages: ObjectId,
    children: Vec<Object>,
    fonts: Vec<Font>,
}

fn number(value: f64) -> Object {
    Object::Real(value as f32)
}
fn op(operations: &mut Vec<Operation>, name: &str, values: Vec<Object>) {
    operations.push(Operation::new(name, values));
}
fn rgb(operations: &mut Vec<Operation>, name: &str, color: [u8; 3]) {
    op(
        operations,
        name,
        color.map(|v| number(f64::from(v) / 255.0)).to_vec(),
    );
}
fn rect(
    operations: &mut Vec<Operation>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    fill: Option<[u8; 3]>,
    stroke: Option<([u8; 3], f64)>,
) {
    if fill.is_none() && stroke.is_none() {
        return;
    }
    if let Some(color) = fill {
        rgb(operations, "rg", color);
    }
    if let Some((color, width)) = stroke {
        rgb(operations, "RG", color);
        op(operations, "w", vec![number(width)]);
    }
    op(
        operations,
        "re",
        vec![number(x), number(y), number(width), number(height)],
    );
    op(
        operations,
        match (fill.is_some(), stroke.is_some()) {
            (true, true) => "B",
            (true, false) => "f",
            _ => "S",
        },
        vec![],
    );
}
fn line(
    operations: &mut Vec<Operation>,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    color: [u8; 3],
    width: f64,
) {
    rgb(operations, "RG", color);
    op(operations, "w", vec![number(width)]);
    op(operations, "m", vec![number(x1), number(y1)]);
    op(operations, "l", vec![number(x2), number(y2)]);
    op(operations, "S", vec![]);
}
fn font_index(font: CellFont) -> usize {
    match font {
        CellFont::Mono => 0,
        CellFont::Sans => 1,
        CellFont::Serif => 2,
    }
}

impl Pdf {
    fn new() -> Result<Self> {
        let mut document = Document::with_version("1.7");
        let pages = document.new_object_id();
        let fonts = vec![
            Font::new(&mut document, CellFont::Mono, "JigMono")?,
            Font::new(&mut document, CellFont::Sans, "JigSans")?,
            Font::new(&mut document, CellFont::Serif, "JigSerif")?,
        ];
        Ok(Self {
            document,
            pages,
            children: Vec::new(),
            fonts,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        operations: &mut Vec<Operation>,
        x: f64,
        y: f64,
        text: &str,
        style: &CellStyle,
        size: f64,
        anchor: TextAnchor,
    ) {
        let font = &mut self.fonts[font_index(style.font_family)];
        let width = font.width(text, size);
        let x = match anchor {
            TextAnchor::Start => x,
            TextAnchor::Middle => x - width / 2.0,
            TextAnchor::End => x - width,
        };
        let bytes = font.encode(text);
        op(operations, "q", vec![]);
        rgb(operations, "rg", style.text_color.unwrap_or([31, 38, 47]));
        if style.bold {
            rgb(operations, "RG", style.text_color.unwrap_or([31, 38, 47]));
            op(operations, "w", vec![number(0.2)]);
        }
        op(operations, "BT", vec![]);
        op(
            operations,
            "Tf",
            vec![
                Object::Name(format!("F{}", font_index(style.font_family)).into_bytes()),
                number(size),
            ],
        );
        op(
            operations,
            "Tr",
            vec![Object::Integer(if style.bold { 2 } else { 0 })],
        );
        op(
            operations,
            "Tm",
            vec![
                number(1.0),
                number(0.0),
                number(if style.italic { 0.2 } else { 0.0 }),
                number(1.0),
                number(x),
                number(y),
            ],
        );
        op(
            operations,
            "Tj",
            vec![Object::String(bytes, StringFormat::Hexadecimal)],
        );
        op(operations, "ET", vec![]);
        if style.underline {
            line(
                operations,
                x,
                y - 1.5,
                x + width,
                y - 1.5,
                style.text_color.unwrap_or([31, 38, 47]),
                0.45,
            );
        }
        op(operations, "Q", vec![]);
    }

    fn page(&mut self, mut operations: Vec<Operation>, title: &str) -> Result<()> {
        if self.children.len() >= MAX_PAGES {
            return Err("PDF exceeds 200 pages; select a smaller print range".into());
        }
        let footer = CellStyle {
            font_size: 9,
            text_color: Some([95, 105, 119]),
            ..CellStyle::default()
        };
        self.text(
            &mut operations,
            MARGIN,
            22.0,
            &format!("Jig · {title}"),
            &footer,
            8.0,
            TextAnchor::Start,
        );
        self.text(
            &mut operations,
            PAGE_WIDTH - MARGIN,
            22.0,
            &format!("{}", self.children.len() + 1),
            &footer,
            8.0,
            TextAnchor::End,
        );
        let content = Content { operations }
            .encode()
            .map_err(|e| format!("Could not encode PDF page: {e}"))?;
        let content = self
            .document
            .add_object(Stream::new(Dictionary::new(), content));
        let fonts: Dictionary = self
            .fonts
            .iter()
            .enumerate()
            .filter(|(_, font)| !font.used.is_empty())
            .map(|(index, font)| {
                (
                    format!("F{index}").into_bytes(),
                    Object::Reference(font.object),
                )
            })
            .collect();
        let page=self.document.add_object(dictionary! {
            "Type"=>"Page","Parent"=>self.pages,"MediaBox"=>vec![number(0.0),number(0.0),number(PAGE_WIDTH),number(PAGE_HEIGHT)],
            "Contents"=>content,"Resources"=>dictionary!{"Font"=>fonts},
        });
        self.children.push(page.into());
        Ok(())
    }

    fn finish(mut self, title: &str) -> Result<Vec<u8>> {
        let count = self.children.len();
        self.document.objects.insert(
            self.pages,
            dictionary! {"Type"=>"Pages","Kids"=>self.children,"Count"=>count as i64}.into(),
        );
        let catalog = self
            .document
            .add_object(dictionary! {"Type"=>"Catalog","Pages"=>self.pages});
        self.document.trailer.set("Root", catalog);
        let mut title_bytes = vec![0xfe, 0xff];
        for ch in title.encode_utf16() {
            title_bytes.extend_from_slice(&ch.to_be_bytes());
        }
        let info=self.document.add_object(dictionary!{"Producer"=>Object::string_literal("Jig local spreadsheet"),"Title"=>Object::String(title_bytes,StringFormat::Hexadecimal)});
        self.document.trailer.set("Info", info);
        for font in self.fonts {
            font.finish(&mut self.document);
        }
        self.document.compress();
        let mut bytes = Vec::new();
        self.document
            .save_to(&mut bytes)
            .map_err(|e| format!("Could not save PDF: {e}"))?;
        Ok(bytes)
    }
}

fn title_style() -> CellStyle {
    CellStyle {
        bold: true,
        font_size: 16,
        ..CellStyle::default()
    }
}

fn grid_pages(
    pdf: &mut Pdf,
    state: &State,
    book: &Workbook,
    sheet_id: u64,
    range: CellRange,
    today: chrono::NaiveDate,
) -> Result<()> {
    let sheet = book.sheet(sheet_id).ok_or("Sheet not found")?;
    let values = book.recalculate_at(today);
    let row_height_at = |row| {
        let font = (range.start.col..=range.end.col)
            .map(|col| book.style(sheet_id, CellAddr::new(row, col)).font_size)
            .max()
            .unwrap_or(13);
        (f64::from(font) * 1.2 + 6.0).max(21.0)
    };
    let per_page = ((PAGE_WIDTH - MARGIN * 2.0 - ROW_HEAD) / COLUMN_WIDTH).floor() as usize;
    let columns_per_page = range.cols().min(per_page).max(1);
    for first_col in (range.start.col..=range.end.col).step_by(columns_per_page) {
        let last_col = (first_col + columns_per_page - 1).min(range.end.col);
        let mut first_row = range.start.row;
        while first_row <= range.end.row {
            let mut operations = Vec::new();
            pdf.text(
                &mut operations,
                MARGIN,
                PAGE_HEIGHT - MARGIN - 5.0,
                &sheet.name,
                &title_style(),
                15.0,
                TextAnchor::Start,
            );
            let header = CellStyle {
                bold: true,
                font_size: 10,
                text_color: Some([72, 82, 97]),
                ..CellStyle::default()
            };
            let label = format!(
                "{}–{}",
                CellAddr::new(first_row, first_col).label(),
                CellAddr::new(range.end.row, last_col).label()
            );
            pdf.text(
                &mut operations,
                PAGE_WIDTH - MARGIN,
                PAGE_HEIGHT - MARGIN - 5.0,
                &label,
                &header,
                9.0,
                TextAnchor::End,
            );
            rect(
                &mut operations,
                MARGIN,
                GRID_TOP,
                ROW_HEAD + (last_col - first_col + 1) as f64 * COLUMN_WIDTH,
                19.0,
                Some([236, 239, 243]),
                None,
            );
            for col in first_col..=last_col {
                let address = CellAddr::new(0, col).label();
                let letters = address.trim_end_matches('1');
                pdf.text(
                    &mut operations,
                    MARGIN
                        + ROW_HEAD
                        + (col - first_col) as f64 * COLUMN_WIDTH
                        + COLUMN_WIDTH / 2.0,
                    GRID_TOP + 5.0,
                    letters,
                    &header,
                    9.0,
                    TextAnchor::Middle,
                );
            }
            let mut y = GRID_TOP;
            let mut row = first_row;
            while row <= range.end.row {
                let row_height = row_height_at(row);
                if y - row_height < GRID_BOTTOM && row > first_row {
                    break;
                }
                let bottom = y - row_height;
                rect(
                    &mut operations,
                    MARGIN,
                    bottom,
                    ROW_HEAD,
                    row_height,
                    Some([246, 247, 249]),
                    Some(([216, 220, 226], 0.25)),
                );
                pdf.text(
                    &mut operations,
                    MARGIN + ROW_HEAD - 5.0,
                    bottom + row_height / 2.0 - 3.0,
                    &(row + 1).to_string(),
                    &header,
                    8.5,
                    TextAnchor::End,
                );
                for col in first_col..=last_col {
                    let addr = CellAddr::new(row, col);
                    let merge = sheet.merges.iter().find(|range| range.contains(addr));
                    if merge.is_some_and(|merge| {
                        merge.start.row.max(first_row) != row
                            || merge.start.col.max(first_col) != col
                    }) {
                        continue;
                    }
                    let x = MARGIN + ROW_HEAD + (col - first_col) as f64 * COLUMN_WIDTH;
                    let width = merge.map_or(COLUMN_WIDTH, |merge| {
                        ((merge.end.col.min(last_col) - col + 1) as f64) * COLUMN_WIDTH
                    });
                    let source = merge.map_or(addr, |merge| merge.start);
                    let cell_height = merge
                        .map_or(row_height, |merge| {
                            (row..=merge.end.row.min(range.end.row))
                                .map(row_height_at)
                                .sum::<f64>()
                        })
                        .min(y - GRID_BOTTOM);
                    let bottom = y - cell_height;
                    let style = book.rendered_style(sheet_id, source);
                    rect(
                        &mut operations,
                        x,
                        bottom,
                        width,
                        cell_height,
                        style.fill_color,
                        Some(([216, 220, 226], 0.25)),
                    );
                    if style.borders.top {
                        line(&mut operations, x, y, x + width, y, [55, 65, 80], 0.7);
                    }
                    if style.borders.bottom {
                        line(
                            &mut operations,
                            x,
                            bottom,
                            x + width,
                            bottom,
                            [55, 65, 80],
                            0.7,
                        );
                    }
                    if style.borders.left {
                        line(&mut operations, x, bottom, x, y, [55, 65, 80], 0.7);
                    }
                    if style.borders.right {
                        line(
                            &mut operations,
                            x + width,
                            bottom,
                            x + width,
                            y,
                            [55, 65, 80],
                            0.7,
                        );
                    }
                    let value = values.value(sheet_id, source);
                    let mut text = value
                        .formatted(&style.number_format, state.preferences.decimals)
                        .replace(['\r', '\n', '\t'], " ");
                    let numeric = matches!(
                        value,
                        crate::spreadsheet::CellValue::Number(_)
                            | crate::spreadsheet::CellValue::Date(_)
                    );
                    let size = f64::from(style.font_size);
                    if numeric
                        && pdf.fonts[font_index(style.font_family)].width(&text, size) > width - 8.0
                    {
                        // Never crop away leading digits or a currency symbol:
                        // a partial amount can be mistaken for a smaller value.
                        text = "####".into();
                    }
                    op(&mut operations, "q", vec![]);
                    op(
                        &mut operations,
                        "re",
                        vec![
                            number(x + 2.0),
                            number(bottom + 1.0),
                            number(width - 4.0),
                            number(cell_height - 2.0),
                        ],
                    );
                    op(&mut operations, "W", vec![]);
                    op(&mut operations, "n", vec![]);
                    pdf.text(
                        &mut operations,
                        if numeric { x + width - 4.0 } else { x + 4.0 },
                        bottom + cell_height / 2.0 - size * 0.35,
                        &text,
                        &style,
                        size,
                        if numeric {
                            TextAnchor::End
                        } else {
                            TextAnchor::Start
                        },
                    );
                    op(&mut operations, "Q", vec![]);
                }
                y = bottom;
                row += 1;
            }
            pdf.page(operations, &sheet.name)?;
            first_row = row;
        }
    }
    Ok(())
}

fn draw_chart(pdf: &mut Pdf, scene: &ChartScene, title: &str) -> Result<()> {
    let scale = (PAGE_WIDTH - MARGIN * 2.0) / scene.width.max(1.0);
    let height = PAGE_HEIGHT - MARGIN * 2.0 - 30.0;
    let slices = (scene.height * scale / height).ceil().max(1.0) as usize;
    if slices > MAX_PAGES {
        return Err("Chart exceeds the PDF page limit".into());
    }
    for slice in 0..slices {
        let mut operations = Vec::new();
        pdf.text(
            &mut operations,
            MARGIN,
            PAGE_HEIGHT - MARGIN - 5.0,
            title,
            &title_style(),
            15.0,
            TextAnchor::Start,
        );
        op(&mut operations, "q", vec![]);
        op(
            &mut operations,
            "re",
            vec![
                number(MARGIN),
                number(MARGIN + 10.0),
                number(PAGE_WIDTH - MARGIN * 2.0),
                number(height),
            ],
        );
        op(&mut operations, "W", vec![]);
        op(&mut operations, "n", vec![]);
        let y = |value: f64| PAGE_HEIGHT - MARGIN - 30.0 - value * scale + slice as f64 * height;
        let x = |value: f64| MARGIN + value * scale;
        for primitive in &scene.primitives {
            match primitive {
                Primitive::Rect {
                    x: rx,
                    y: ry,
                    width,
                    height,
                    fill,
                    stroke,
                } => rect(
                    &mut operations,
                    x(*rx),
                    y(*ry + *height),
                    *width * scale,
                    *height * scale,
                    *fill,
                    stroke
                        .as_ref()
                        .map(|stroke| (stroke.color, stroke.width * scale)),
                ),
                Primitive::Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    stroke,
                } => line(
                    &mut operations,
                    x(*x1),
                    y(*y1),
                    x(*x2),
                    y(*y2),
                    stroke.color,
                    stroke.width * scale,
                ),
                Primitive::Text {
                    x: tx,
                    y: ty,
                    text,
                    size,
                    color,
                    anchor,
                } => {
                    let style = CellStyle {
                        text_color: Some(*color),
                        ..CellStyle::default()
                    };
                    pdf.text(
                        &mut operations,
                        x(*tx),
                        y(*ty),
                        text,
                        &style,
                        *size * scale,
                        *anchor,
                    );
                }
                Primitive::Polygon {
                    points,
                    fill,
                    stroke,
                } => {
                    if let Some(color) = fill {
                        rgb(&mut operations, "rg", *color);
                    }
                    if let Some(stroke) = stroke {
                        rgb(&mut operations, "RG", stroke.color);
                        op(&mut operations, "w", vec![number(stroke.width * scale)]);
                    }
                    if let Some(first) = points.first() {
                        op(
                            &mut operations,
                            "m",
                            vec![number(x(first[0])), number(y(first[1]))],
                        );
                        for point in points.iter().skip(1) {
                            op(
                                &mut operations,
                                "l",
                                vec![number(x(point[0])), number(y(point[1]))],
                            );
                        }
                        op(&mut operations, "h", vec![]);
                        op(
                            &mut operations,
                            match (fill.is_some(), stroke.is_some()) {
                                (true, true) => "B",
                                (true, false) => "f",
                                _ => "S",
                            },
                            vec![],
                        );
                    }
                }
            }
        }
        op(&mut operations, "Q", vec![]);
        pdf.page(operations, title)?;
    }
    Ok(())
}

/// Print one sheet (or a selected area), followed by its vector charts. Text is
/// embedded as Unicode-mapped glyphs, so currency and accented names survive
/// export and remain searchable in a PDF reader.
pub fn export_pdf(state: &State, sheet_id: u64, area: Option<CellRange>) -> Result<Vec<u8>> {
    state.validate()?;
    let empty = Workbook::default();
    let book = state.workbook.as_ref().unwrap_or(&empty);
    let sheet = book.sheet(sheet_id).ok_or("Sheet not found")?;
    let range = area
        .or_else(|| book.used_range(sheet_id))
        .unwrap_or(CellRange::new(CellAddr::new(0, 0), CellAddr::new(0, 0)));
    if !range.start.in_bounds()
        || !range.end.in_bounds()
        || range.start.row > range.end.row
        || range.start.col > range.end.col
    {
        return Err("PDF print range is outside the sheet".into());
    }
    let mut pdf = Pdf::new()?;
    let today = chrono::Local::now().date_naive();
    grid_pages(&mut pdf, state, book, sheet_id, range, today)?;
    for spec in &book.charts {
        let relevant = match spec.source {
            ChartSource::Range { sheet, .. } => sheet == sheet_id,
            ChartSource::Tasks => true,
        };
        if !relevant {
            continue;
        }
        match charts::scene_at(state, spec, 720.0, 360.0, true, &chrono::Local, today) {
            Ok(scene) => draw_chart(&mut pdf, &scene, &spec.title)?,
            Err(error) => {
                let mut ops = Vec::new();
                pdf.text(
                    &mut ops,
                    MARGIN,
                    PAGE_HEIGHT - MARGIN - 10.0,
                    &spec.title,
                    &title_style(),
                    15.0,
                    TextAnchor::Start,
                );
                pdf.text(
                    &mut ops,
                    MARGIN,
                    PAGE_HEIGHT - MARGIN - 40.0,
                    &error,
                    &CellStyle::default(),
                    10.0,
                    TextAnchor::Start,
                );
                pdf.page(ops, &spec.title)?;
            }
        }
    }
    pdf.finish(&sheet.name)
}
