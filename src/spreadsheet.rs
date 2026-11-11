//! Sparse, portable workbook data and bounded spreadsheet evaluation.
//! Browser and terminal adapters share these records; no files or network APIs
//! are needed to edit, recalculate, or exchange comma/tab separated values.
use crate::sheet_formula::{self, BinaryOp, Expr, Reference, UnaryOp};
use crate::Result;
use chrono::{Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub use crate::sheet_formula::FormulaError as CellError;

pub const MAX_ROWS: usize = 1000;
pub const MAX_COLS: usize = 52;
pub const MAX_SHEETS: usize = 16;
pub const MAX_CELLS: usize = 20_000;
pub const MAX_RAW_BYTES: usize = 1024 * 1024;
pub const MAX_CELL_BYTES: usize = 4096;
const MAX_EVAL_DEPTH: usize = 128;
const MAX_EVAL_STEPS: usize = 100_000;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct CellAddr {
    pub row: usize,
    pub col: usize,
}

impl CellAddr {
    pub const fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }

    pub fn parse(text: &str) -> Result<Self> {
        let text = text.trim();
        let bytes = text.as_bytes();
        let mut index = usize::from(bytes.first() == Some(&b'$'));
        let mut col = 0_usize;
        let letters = index;
        while bytes.get(index).is_some_and(u8::is_ascii_alphabetic) {
            col = col
                .checked_mul(26)
                .and_then(|value| {
                    value.checked_add(usize::from(bytes[index].to_ascii_uppercase() - b'A') + 1)
                })
                .ok_or("Cell is outside the supported grid")?;
            index += 1;
        }
        if index == letters {
            return Err("Use a cell address such as A1".into());
        }
        if bytes.get(index) == Some(&b'$') {
            index += 1;
        }
        if !bytes
            .get(index..)
            .is_some_and(|digits| !digits.is_empty() && digits.iter().all(u8::is_ascii_digit))
        {
            return Err("Use a cell address such as A1".into());
        }
        let row: usize = text
            .get(index..)
            .unwrap_or("")
            .parse()
            .map_err(|_| "Use a cell address such as A1")?;
        if row == 0 || row > MAX_ROWS || col == 0 || col > MAX_COLS {
            return Err(format!("Use a cell between A1 and AZ{MAX_ROWS}"));
        }
        Ok(Self {
            row: row - 1,
            col: col - 1,
        })
    }

    pub fn label(self) -> String {
        if !self.in_bounds() {
            return "#REF!".into();
        }
        let mut col = self.col + 1;
        let mut letters = Vec::new();
        while col > 0 {
            letters.push((b'A' + ((col - 1) % 26) as u8) as char);
            col = (col - 1) / 26;
        }
        letters.reverse();
        format!(
            "{}{}",
            letters.into_iter().collect::<String>(),
            self.row + 1
        )
    }

    pub fn in_bounds(self) -> bool {
        self.row < MAX_ROWS && self.col < MAX_COLS
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CellRange {
    pub start: CellAddr,
    pub end: CellAddr,
}

impl CellRange {
    pub fn new(a: CellAddr, b: CellAddr) -> Self {
        Self {
            start: CellAddr::new(a.row.min(b.row), a.col.min(b.col)),
            end: CellAddr::new(a.row.max(b.row), a.col.max(b.col)),
        }
    }

    pub fn rows(self) -> usize {
        self.end.row.saturating_sub(self.start.row) + 1
    }

    pub fn cols(self) -> usize {
        self.end.col.saturating_sub(self.start.col) + 1
    }

    pub fn contains(self, addr: CellAddr) -> bool {
        addr.row >= self.start.row
            && addr.row <= self.end.row
            && addr.col >= self.start.col
            && addr.col <= self.end.col
    }

    pub fn intersects(self, other: Self) -> bool {
        self.start.row <= other.end.row
            && self.end.row >= other.start.row
            && self.start.col <= other.end.col
            && self.end.col >= other.start.col
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellFont {
    #[default]
    Mono,
    Sans,
    Serif,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumberFormat {
    #[default]
    General,
    Number,
    Percent,
    Currency(String),
    Date,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CellBorders {
    #[serde(skip_serializing_if = "is_false")]
    pub top: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub right: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub bottom: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub left: bool,
}

impl CellBorders {
    pub const fn none() -> Self {
        Self {
            top: false,
            right: false,
            bottom: false,
            left: false,
        }
    }

    pub const fn all() -> Self {
        Self {
            top: true,
            right: true,
            bottom: true,
            left: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BorderPreset {
    None,
    All,
    Outer,
}

fn is_false(value: &bool) -> bool {
    !value
}
fn default_font_size() -> u8 {
    13
}
fn is_default_font_size(value: &u8) -> bool {
    *value == 13
}
fn is_default_font(value: &CellFont) -> bool {
    *value == CellFont::Mono
}
fn is_general_format(value: &NumberFormat) -> bool {
    *value == NumberFormat::General
}
fn is_no_border(value: &CellBorders) -> bool {
    *value == CellBorders::none()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CellStyle {
    #[serde(skip_serializing_if = "is_default_font")]
    pub font_family: CellFont,
    #[serde(
        default = "default_font_size",
        skip_serializing_if = "is_default_font_size"
    )]
    pub font_size: u8,
    #[serde(skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_color: Option<[u8; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<[u8; 3]>,
    #[serde(skip_serializing_if = "is_no_border")]
    pub borders: CellBorders,
    #[serde(skip_serializing_if = "is_general_format")]
    pub number_format: NumberFormat,
}

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            font_family: CellFont::Mono,
            font_size: 13,
            bold: false,
            italic: false,
            underline: false,
            text_color: None,
            fill_color: None,
            borders: CellBorders::none(),
            number_format: NumberFormat::General,
        }
    }
}

impl CellStyle {
    pub fn validate(&self) -> Result<()> {
        if !(9..=32).contains(&self.font_size) {
            return Err("Font size must be between 9 and 32".into());
        }
        if let NumberFormat::Currency(code) = &self.number_format {
            if code.len() != 3 || !code.bytes().all(|c| c.is_ascii_uppercase()) {
                return Err("Use a three-letter uppercase currency code, e.g. USD".into());
            }
        }
        Ok(())
    }
}

/// Toolbar changes preserve every property that was not explicitly changed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StylePatch {
    pub font_family: Option<CellFont>,
    pub font_size: Option<u8>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub text_color: Option<Option<[u8; 3]>>,
    pub fill_color: Option<Option<[u8; 3]>>,
    pub borders: Option<CellBorders>,
    pub number_format: Option<NumberFormat>,
}

impl StylePatch {
    pub fn apply(&self, style: &mut CellStyle) {
        if let Some(value) = self.font_family {
            style.font_family = value;
        }
        if let Some(value) = self.font_size {
            style.font_size = value;
        }
        if let Some(value) = self.bold {
            style.bold = value;
        }
        if let Some(value) = self.italic {
            style.italic = value;
        }
        if let Some(value) = self.underline {
            style.underline = value;
        }
        if let Some(value) = self.text_color {
            style.text_color = value;
        }
        if let Some(value) = self.fill_color {
            style.fill_color = value;
        }
        if let Some(value) = self.borders {
            style.borders = value;
        }
        if let Some(value) = &self.number_format {
            style.number_format = value.clone();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub cells: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub styles: BTreeMap<String, CellStyle>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merges: Vec<CellRange>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Workbook {
    pub version: u32,
    pub sheets: Vec<Sheet>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub charts: Vec<crate::charts::ChartSpec>,
}

impl Default for Workbook {
    fn default() -> Self {
        Self {
            version: 1,
            sheets: vec![Sheet {
                id: 1,
                name: "Sheet 1".into(),
                cells: BTreeMap::new(),
                styles: BTreeMap::new(),
                merges: Vec::new(),
            }],
            charts: Vec::new(),
        }
    }
}

fn sheet_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > 64
        || name
            .chars()
            .any(|c| c.is_control() || "[]:*?/\\!".contains(c))
    {
        return Err("Sheet names need 1–64 characters and cannot contain []:*?/\\!".into());
    }
    Ok(name.into())
}

fn valid_range(range: CellRange) -> Result<()> {
    if !range.start.in_bounds()
        || !range.end.in_bounds()
        || range.start.row > range.end.row
        || range.start.col > range.end.col
    {
        Err("Invalid cell range".into())
    } else {
        Ok(())
    }
}

fn shifted_addr(
    mut addr: CellAddr,
    axis: sheet_formula::Axis,
    edit: sheet_formula::Edit,
) -> Result<Option<CellAddr>> {
    use sheet_formula::{Axis, Edit};
    let value = match axis {
        Axis::Row => &mut addr.row,
        Axis::Column => &mut addr.col,
    };
    match edit {
        Edit::Insert { at, count } if *value >= at => {
            *value = value
                .checked_add(count)
                .ok_or("No room to insert without losing cells")?;
        }
        Edit::Delete { at, count } if *value >= at && *value < at + count => return Ok(None),
        Edit::Delete { at, count } if *value >= at + count => *value -= count,
        _ => {}
    }
    if addr.in_bounds() {
        Ok(Some(addr))
    } else {
        Err("No room to insert without losing cells or formatting".into())
    }
}

fn shift_map<T: Clone>(
    source: &BTreeMap<String, T>,
    axis: sheet_formula::Axis,
    edit: sheet_formula::Edit,
) -> Result<BTreeMap<String, T>> {
    let mut result = BTreeMap::new();
    for (address, value) in source {
        if let Some(addr) = shifted_addr(CellAddr::parse(address)?, axis, edit)? {
            result.insert(addr.label(), value.clone());
        }
    }
    Ok(result)
}

fn adjusted_range(
    range: CellRange,
    name: &str,
    axis: sheet_formula::Axis,
    edit: sheet_formula::Edit,
) -> Option<CellRange> {
    let source = format!("={}:{}", range.start.label(), range.end.label());
    let adjusted = sheet_formula::adjust_references(&source, name, name, axis, edit);
    match sheet_formula::parse(&adjusted).ok()? {
        Expr::Range(first, last) => Some(CellRange::new(
            CellAddr::new(first.row, first.col),
            CellAddr::new(last.row, last.col),
        )),
        Expr::Ref(reference) => Some(CellRange::new(
            CellAddr::new(reference.row, reference.col),
            CellAddr::new(reference.row, reference.col),
        )),
        _ => None,
    }
}

impl Workbook {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 || self.sheets.is_empty() || self.sheets.len() > MAX_SHEETS {
            return Err(format!(
                "A workbook needs 1–{MAX_SHEETS} sheets and version 1"
            ));
        }
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        let mut count = 0;
        let mut bytes = 0;
        let mut merge_count = 0;
        for sheet in &self.sheets {
            let mut occupied = HashSet::new();
            if sheet.id == 0 || !ids.insert(sheet.id) {
                return Err("Sheet IDs must be unique and nonzero".into());
            }
            let name = sheet_name(&sheet.name)?;
            if name != sheet.name || !names.insert(name.to_ascii_lowercase()) {
                return Err("Sheet names must be trimmed and unique".into());
            }
            for (address, raw) in &sheet.cells {
                let addr = CellAddr::parse(address)?;
                occupied.insert(addr);
                if addr.label() != *address
                    || raw.is_empty()
                    || raw.len() > MAX_CELL_BYTES
                    || raw
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\t' | '\r' | '\n'))
                {
                    return Err("Invalid cell address or contents".into());
                }
                count += 1;
                bytes += raw.len();
                if count > MAX_CELLS || bytes > MAX_RAW_BYTES {
                    return Err("Workbook exceeds its cell or text limit".into());
                }
            }
            let nonempty = occupied.clone();
            for (address, style) in &sheet.styles {
                let addr = CellAddr::parse(address)?;
                if addr.label() != *address {
                    return Err("Invalid style cell address".into());
                }
                style.validate()?;
                if occupied.insert(addr) {
                    count += 1;
                }
                if count > MAX_CELLS {
                    return Err("Workbook exceeds its cell or style limit".into());
                }
            }
            merge_count += sheet.merges.len();
            if merge_count > 2000 {
                return Err("Workbook has too many merged ranges".into());
            }
            for (index, range) in sheet.merges.iter().enumerate() {
                valid_range(*range)?;
                if range.rows() * range.cols() < 2 {
                    return Err("A merged range needs at least two cells".into());
                }
                if sheet.merges[..index]
                    .iter()
                    .any(|other| range.intersects(*other))
                {
                    return Err("Merged ranges cannot overlap".into());
                }
                let hides_content = (range.start.row..=range.end.row).any(|row| {
                    (range.start.col..=range.end.col).any(|col| {
                        let addr = CellAddr::new(row, col);
                        addr != range.start && nonempty.contains(&addr)
                    })
                });
                if hides_content {
                    return Err("Merged ranges must not hide nonempty cells".into());
                }
            }
        }
        if self.charts.len() > 16 {
            return Err("A workbook can contain up to 16 charts".into());
        }
        let mut chart_ids = HashSet::new();
        for chart in &self.charts {
            if chart.id == 0 || !chart_ids.insert(chart.id) {
                return Err("Chart IDs must be unique and nonzero".into());
            }
            chart.validate(self)?;
        }
        Ok(())
    }

    pub fn sheet(&self, id: u64) -> Option<&Sheet> {
        self.sheets.iter().find(|sheet| sheet.id == id)
    }

    pub fn raw(&self, sheet: u64, addr: CellAddr) -> &str {
        self.sheet(sheet)
            .and_then(|sheet| sheet.cells.get(&addr.label()))
            .map_or("", String::as_str)
    }

    pub fn style(&self, sheet: u64, addr: CellAddr) -> CellStyle {
        self.sheet(sheet)
            .and_then(|sheet| sheet.styles.get(&addr.label()))
            .cloned()
            .unwrap_or_default()
    }

    pub fn merge_at(&self, sheet: u64, addr: CellAddr) -> Option<CellRange> {
        self.sheet(sheet)?
            .merges
            .iter()
            .copied()
            .find(|range| range.contains(addr))
    }

    /// A merged cell uses its anchor's text/fill and the complete perimeter's
    /// borders. Stored covered-cell styles are retained for later unmerging.
    pub fn rendered_style(&self, sheet: u64, addr: CellAddr) -> CellStyle {
        let Some(range) = self.merge_at(sheet, addr) else {
            return self.style(sheet, addr);
        };
        let mut result = self.style(sheet, range.start);
        let Some(sheet) = self.sheet(sheet) else {
            return result;
        };
        let edges = |row, col| {
            sheet
                .styles
                .get(&CellAddr::new(row, col).label())
                .map_or(CellBorders::none(), |style| style.borders)
        };
        for col in range.start.col..=range.end.col {
            result.borders.top |= edges(range.start.row, col).top;
            result.borders.bottom |= edges(range.end.row, col).bottom;
        }
        for row in range.start.row..=range.end.row {
            result.borders.left |= edges(row, range.start.col).left;
            result.borders.right |= edges(row, range.end.col).right;
        }
        result
    }

    fn update_styles(
        &mut self,
        sheet: u64,
        range: CellRange,
        mut change: impl FnMut(CellAddr, &mut CellStyle),
    ) -> Result<bool> {
        valid_range(range)?;
        let mut next = self.clone();
        let target = next
            .sheets
            .iter_mut()
            .find(|item| item.id == sheet)
            .ok_or("Sheet not found")?;
        for row in range.start.row..=range.end.row {
            for col in range.start.col..=range.end.col {
                let addr = CellAddr::new(row, col);
                let key = addr.label();
                let mut style = target.styles.get(&key).cloned().unwrap_or_default();
                change(addr, &mut style);
                if style == CellStyle::default() {
                    target.styles.remove(&key);
                } else {
                    target.styles.insert(key, style);
                }
            }
        }
        next.validate()?;
        let changed = next != *self;
        if changed {
            *self = next;
        }
        Ok(changed)
    }

    pub fn set_style(&mut self, sheet: u64, range: CellRange, style: CellStyle) -> Result<bool> {
        style.validate()?;
        self.update_styles(sheet, range, |_, current| *current = style.clone())
    }

    pub fn patch_style(&mut self, sheet: u64, range: CellRange, patch: StylePatch) -> Result<bool> {
        self.update_styles(sheet, range, |_, current| patch.apply(current))
    }

    pub fn set_borders(
        &mut self,
        sheet: u64,
        range: CellRange,
        preset: BorderPreset,
    ) -> Result<bool> {
        self.update_styles(sheet, range, |addr, style| {
            style.borders = match preset {
                BorderPreset::None => CellBorders::none(),
                BorderPreset::All => CellBorders::all(),
                BorderPreset::Outer => CellBorders {
                    top: addr.row == range.start.row,
                    bottom: addr.row == range.end.row,
                    left: addr.col == range.start.col,
                    right: addr.col == range.end.col,
                },
            };
        })
    }

    pub fn merge(&mut self, sheet: u64, range: CellRange) -> Result<bool> {
        valid_range(range)?;
        let current = self.sheet(sheet).ok_or("Sheet not found")?;
        if range.rows() * range.cols() < 2 || current.merges.contains(&range) {
            return Ok(false);
        }
        if current.merges.iter().any(|other| range.intersects(*other)) {
            return Err("Unmerge overlapping cells before merging a new range".into());
        }
        if current.cells.keys().any(|address| {
            CellAddr::parse(address).is_ok_and(|addr| addr != range.start && range.contains(addr))
        }) {
            return Err(
                "Merging would hide cell contents; keep content in the top-left cell only".into(),
            );
        }
        let mut next = self.clone();
        next.sheets
            .iter_mut()
            .find(|item| item.id == sheet)
            .ok_or("Sheet not found")?
            .merges
            .push(range);
        next.validate()?;
        *self = next;
        Ok(true)
    }

    pub fn unmerge(&mut self, sheet: u64, range: CellRange) -> Result<bool> {
        valid_range(range)?;
        let target = self
            .sheets
            .iter_mut()
            .find(|item| item.id == sheet)
            .ok_or("Sheet not found")?;
        let before = target.merges.len();
        target.merges.retain(|merged| !merged.intersects(range));
        Ok(before != target.merges.len())
    }

    pub fn insert_rows(&mut self, sheet: u64, at: usize, count: usize) -> Result<bool> {
        self.edit_axis(
            sheet,
            sheet_formula::Axis::Row,
            sheet_formula::Edit::Insert { at, count },
        )
    }

    pub fn delete_rows(&mut self, sheet: u64, at: usize, count: usize) -> Result<bool> {
        self.edit_axis(
            sheet,
            sheet_formula::Axis::Row,
            sheet_formula::Edit::Delete { at, count },
        )
    }

    pub fn insert_columns(&mut self, sheet: u64, at: usize, count: usize) -> Result<bool> {
        self.edit_axis(
            sheet,
            sheet_formula::Axis::Column,
            sheet_formula::Edit::Insert { at, count },
        )
    }

    pub fn delete_columns(&mut self, sheet: u64, at: usize, count: usize) -> Result<bool> {
        self.edit_axis(
            sheet,
            sheet_formula::Axis::Column,
            sheet_formula::Edit::Delete { at, count },
        )
    }

    fn edit_axis(
        &mut self,
        sheet: u64,
        axis: sheet_formula::Axis,
        edit: sheet_formula::Edit,
    ) -> Result<bool> {
        use sheet_formula::{Axis, Edit};
        let affected = self.sheet(sheet).ok_or("Sheet not found")?.name.clone();
        let limit = match axis {
            Axis::Row => MAX_ROWS,
            Axis::Column => MAX_COLS,
        };
        let (at, count) = match edit {
            Edit::Insert { at, count } | Edit::Delete { at, count } => (at, count),
        };
        if count == 0 {
            return Ok(false);
        }
        if at >= limit || count > limit - at {
            return Err("Row or column operation is outside the supported grid".into());
        }
        let mut next = self.clone();
        let target = next
            .sheets
            .iter_mut()
            .find(|item| item.id == sheet)
            .ok_or("Sheet not found")?;
        target.cells = shift_map(&target.cells, axis, edit)?;
        target.styles = shift_map(&target.styles, axis, edit)?;
        let mut merges = Vec::new();
        for range in &target.merges {
            match adjusted_range(*range, &affected, axis, edit) {
                Some(range) if range.rows() * range.cols() >= 2 => merges.push(range),
                None if matches!(edit, Edit::Insert { .. }) => {
                    return Err("No room to insert without losing a merged range".into())
                }
                _ => {}
            }
        }
        target.merges = merges;
        for item in &mut next.sheets {
            for raw in item
                .cells
                .values_mut()
                .filter(|raw| raw.trim_start().starts_with('='))
            {
                *raw = sheet_formula::adjust_references(raw, &item.name, &affected, axis, edit);
            }
        }
        let mut chart_overflow = false;
        next.charts.retain_mut(|chart| {
            if let crate::charts::ChartSource::Range {
                sheet: source,
                range,
            } = &mut chart.source
            {
                if *source == sheet {
                    let Some(adjusted) = adjusted_range(*range, &affected, axis, edit) else {
                        chart_overflow |= matches!(edit, Edit::Insert { .. });
                        return false;
                    };
                    let columns = if chart.kind == crate::charts::ChartKind::Gantt {
                        3
                    } else {
                        2
                    };
                    if adjusted.cols() < columns {
                        return false;
                    }
                    *range = adjusted;
                }
            }
            true
        });
        if chart_overflow {
            return Err("No room to insert without losing a chart source".into());
        }
        next.validate()?;
        let changed = next != *self;
        if changed {
            *self = next;
        }
        Ok(changed)
    }

    pub fn add_chart(
        &mut self,
        kind: crate::charts::ChartKind,
        source: crate::charts::ChartSource,
        title: &str,
    ) -> Result<u64> {
        let mut next = self.clone();
        let id = next
            .charts
            .iter()
            .map(|chart| chart.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("Chart IDs exhausted")?;
        next.charts.push(crate::charts::ChartSpec {
            id,
            title: title.trim().into(),
            kind,
            source,
        });
        next.validate()?;
        *self = next;
        Ok(id)
    }

    pub fn rename_chart(&mut self, id: u64, title: &str) -> Result<bool> {
        let mut next = self.clone();
        let chart = next
            .charts
            .iter_mut()
            .find(|chart| chart.id == id)
            .ok_or("Chart not found")?;
        chart.title = title.trim().into();
        next.validate()?;
        let changed = next != *self;
        if changed {
            *self = next;
        }
        Ok(changed)
    }

    pub fn remove_chart(&mut self, id: u64) -> Result<bool> {
        let before = self.charts.len();
        self.charts.retain(|chart| chart.id != id);
        Ok(before != self.charts.len())
    }

    pub fn used_range(&self, sheet: u64) -> Option<CellRange> {
        let sheet = self.sheet(sheet)?;
        let mut bounds: Option<CellRange> = None;
        for address in sheet.cells.keys().chain(sheet.styles.keys()) {
            let Ok(addr) = CellAddr::parse(address) else {
                continue;
            };
            bounds = Some(match bounds {
                None => CellRange::new(addr, addr),
                Some(range) => CellRange {
                    start: CellAddr::new(
                        range.start.row.min(addr.row),
                        range.start.col.min(addr.col),
                    ),
                    end: CellAddr::new(range.end.row.max(addr.row), range.end.col.max(addr.col)),
                },
            });
        }
        for range in &sheet.merges {
            bounds = Some(match bounds {
                None => *range,
                Some(bounds) => CellRange {
                    start: CellAddr::new(
                        bounds.start.row.min(range.start.row),
                        bounds.start.col.min(range.start.col),
                    ),
                    end: CellAddr::new(
                        bounds.end.row.max(range.end.row),
                        bounds.end.col.max(range.end.col),
                    ),
                },
            });
        }
        bounds
    }

    pub fn set_cell(&mut self, sheet: u64, addr: CellAddr, raw: &str) -> Result<bool> {
        self.set_block(sheet, addr, &[vec![raw.to_owned()]])
    }

    pub fn set_block(
        &mut self,
        sheet: u64,
        origin: CellAddr,
        rows: &[Vec<String>],
    ) -> Result<bool> {
        if !origin.in_bounds()
            || rows.len() > MAX_ROWS - origin.row
            || rows.iter().any(|row| row.len() > MAX_COLS - origin.col)
        {
            return Err("The pasted cells extend beyond the supported grid".into());
        }
        let mut next = self.clone();
        let target = next
            .sheets
            .iter_mut()
            .find(|item| item.id == sheet)
            .ok_or("Sheet not found")?;
        for (row, values) in rows.iter().enumerate() {
            for (col, raw) in values.iter().enumerate() {
                let address = CellAddr::new(origin.row + row, origin.col + col).label();
                if raw.is_empty() {
                    target.cells.remove(&address);
                } else {
                    target.cells.insert(address, raw.clone());
                }
            }
        }
        next.validate()?;
        let changed = next != *self;
        if changed {
            *self = next;
        }
        Ok(changed)
    }

    pub fn clear(&mut self, sheet: u64, range: CellRange) -> Result<bool> {
        if !range.start.in_bounds()
            || !range.end.in_bounds()
            || range.start.row > range.end.row
            || range.start.col > range.end.col
        {
            return Err("Invalid cell range".into());
        }
        let target = self
            .sheets
            .iter_mut()
            .find(|item| item.id == sheet)
            .ok_or("Sheet not found")?;
        let before = target.cells.len();
        target
            .cells
            .retain(|address, _| CellAddr::parse(address).is_ok_and(|addr| !range.contains(addr)));
        Ok(before != target.cells.len())
    }

    pub fn add_sheet(&mut self, name: &str) -> Result<u64> {
        let name = sheet_name(name)?;
        if self.sheets.len() >= MAX_SHEETS {
            return Err("Too many sheets".into());
        }
        if self
            .sheets
            .iter()
            .any(|sheet| sheet.name.eq_ignore_ascii_case(&name))
        {
            return Err("A sheet already has that name".into());
        }
        let id = self
            .sheets
            .iter()
            .map(|sheet| sheet.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("Sheet IDs exhausted")?;
        self.sheets.push(Sheet {
            id,
            name,
            cells: BTreeMap::new(),
            styles: BTreeMap::new(),
            merges: Vec::new(),
        });
        Ok(id)
    }

    pub fn rename_sheet(&mut self, id: u64, name: &str) -> Result<bool> {
        let name = sheet_name(name)?;
        let current = self.sheet(id).ok_or("Sheet not found")?;
        if current.name == name {
            return Ok(false);
        }
        if self
            .sheets
            .iter()
            .any(|sheet| sheet.id != id && sheet.name.eq_ignore_ascii_case(&name))
        {
            return Err("A sheet already has that name".into());
        }
        let old = current.name.clone();
        let mut next = self.clone();
        for sheet in &mut next.sheets {
            if sheet.id == id {
                sheet.name = name.clone();
            }
            for raw in sheet
                .cells
                .values_mut()
                .filter(|raw| raw.trim_start().starts_with('='))
            {
                *raw = sheet_formula::rename_sheet_references(raw, &old, &name);
            }
        }
        next.validate()?;
        *self = next;
        Ok(true)
    }

    pub fn remove_sheet(&mut self, id: u64) -> Result<bool> {
        let index = self
            .sheets
            .iter()
            .position(|sheet| sheet.id == id)
            .ok_or("Sheet not found")?;
        if self.sheets.len() == 1 {
            return Err("Keep at least one sheet in the workbook".into());
        }
        let mut next = self.clone();
        let name = next.sheets.remove(index).name;
        next.charts.retain(|chart| !matches!(chart.source, crate::charts::ChartSource::Range { sheet, .. } if sheet == id));
        for sheet in &mut next.sheets {
            for raw in sheet
                .cells
                .values_mut()
                .filter(|raw| raw.trim_start().starts_with('='))
            {
                *raw = sheet_formula::delete_sheet_references(raw, &name);
            }
        }
        next.validate()?;
        *self = next;
        Ok(true)
    }

    pub fn recalculate(&self) -> Evaluation {
        self.recalculate_at(Utc::now().date_naive())
    }

    pub fn recalculate_at(&self, today: NaiveDate) -> Evaluation {
        let mut evaluator = Recalculator {
            book: self,
            values: BTreeMap::new(),
            active: HashSet::new(),
            steps: 0,
            today,
        };
        // Preserve inexpensive literal values even if a large set of formulas
        // exhausts its operation budget later in this recalculation.
        for sheet in &self.sheets {
            for (address, raw) in &sheet.cells {
                if let (Ok(addr), Some(value)) = (CellAddr::parse(address), literal(raw)) {
                    evaluator.values.insert((sheet.id, addr), value);
                }
            }
        }
        for sheet in &self.sheets {
            for address in sheet.cells.keys() {
                if let Ok(addr) = CellAddr::parse(address) {
                    let value = evaluator.cell(sheet.id, addr, 0);
                    evaluator.values.entry((sheet.id, addr)).or_insert(value);
                }
            }
        }
        Evaluation {
            values: evaluator.values,
        }
    }

    /// Import a complete CSV into one sheet. UI paste/import-at-selection can
    /// instead call parse_delimited followed by set_block.
    pub fn import_csv(&mut self, sheet: u64, text: &str) -> Result<bool> {
        let rows = parse_delimited(text, ',')?;
        let mut next = self.clone();
        next.sheets
            .iter_mut()
            .find(|item| item.id == sheet)
            .ok_or("Sheet not found")?
            .cells
            .clear();
        next.set_block(sheet, CellAddr::new(0, 0), &rows)?;
        let changed = next != *self;
        if changed {
            *self = next;
        }
        Ok(changed)
    }

    /// CSV contains evaluated values at full precision; JSON retains formulas.
    pub fn export_csv(&self, sheet: u64) -> Result<String> {
        self.export_csv_at(sheet, Utc::now().date_naive())
    }

    pub fn export_csv_at(&self, sheet: u64, today: NaiveDate) -> Result<String> {
        self.sheet(sheet).ok_or("Sheet not found")?;
        let Some(range) = self.used_range(sheet) else {
            return Ok(String::new());
        };
        let evaluation = self.recalculate_at(today);
        let rows: Vec<Vec<String>> = (0..=range.end.row)
            .map(|row| {
                (0..=range.end.col)
                    .map(|col| evaluation.value(sheet, CellAddr::new(row, col)).csv_value())
                    .collect()
            })
            .collect();
        Ok(encode_delimited(&rows, ','))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    Blank,
    Number(f64),
    Text(String),
    Bool(bool),
    Date(NaiveDate),
    Error(CellError),
}

impl CellValue {
    pub fn display(&self, decimals: usize) -> String {
        match self {
            Self::Blank => String::new(),
            Self::Number(value) => format!("{:.*}", decimals.min(10), value),
            Self::Text(value) => value.clone(),
            Self::Bool(value) => if *value { "TRUE" } else { "FALSE" }.into(),
            Self::Date(value) => value.format("%Y-%m-%d").to_string(),
            Self::Error(error) => error.code().into(),
        }
    }

    pub fn csv_value(&self) -> String {
        match self {
            Self::Number(value) => value.to_string(),
            _ => self.display(2),
        }
    }

    pub fn as_date(&self) -> Option<NaiveDate> {
        match self {
            Self::Date(value) => Some(*value),
            Self::Number(value) => serial_to_date(*value),
            Self::Text(value) => parse_date(value),
            _ => None,
        }
    }

    pub fn formatted(&self, format: &NumberFormat, decimals: usize) -> String {
        let decimals = decimals.min(10);
        if *format == NumberFormat::General {
            return self.display(decimals);
        }
        if *format == NumberFormat::Date {
            return self.as_date().map_or_else(
                || self.display(decimals),
                |date| date.format("%Y-%m-%d").to_string(),
            );
        }
        let number = match self {
            Self::Number(value) => *value,
            Self::Date(date) => date_to_serial(*date),
            _ => return self.display(decimals),
        };
        match format {
            NumberFormat::Number => format!("{number:.*}", decimals),
            NumberFormat::Percent => {
                let percent = number * 100.0;
                if percent.is_finite() {
                    format!("{percent:.*}%", decimals)
                } else {
                    CellError::Num.code().into()
                }
            }
            NumberFormat::Currency(code) => {
                let sign = if number < 0.0 { "-" } else { "" };
                let plain = format!("{:.*}", decimals, number.abs());
                let (integer, fraction) = plain.split_once('.').unwrap_or((&plain, ""));
                let mut amount = String::new();
                for (index, digit) in integer.chars().enumerate() {
                    if index > 0 && (integer.len() - index) % 3 == 0 {
                        amount.push(',');
                    }
                    amount.push(digit);
                }
                if decimals > 0 {
                    amount.push('.');
                    amount.push_str(fraction);
                }
                match code.as_str() {
                    "USD" => format!("{sign}${amount}"),
                    "EUR" => format!("{sign}€{amount}"),
                    "GBP" => format!("{sign}£{amount}"),
                    "JPY" => format!("{sign}¥{amount}"),
                    _ => format!("{sign}{code} {amount}"),
                }
            }
            _ => self.display(decimals),
        }
    }
}

pub fn parse_date(text: &str) -> Option<NaiveDate> {
    let text = text.trim();
    if text.len() != 10 {
        return None;
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
}

pub fn date_to_serial(date: NaiveDate) -> f64 {
    let epoch = NaiveDate::from_ymd_opt(1899, 12, 30).expect("Valid date epoch");
    (date - epoch).num_days() as f64
}

pub fn serial_to_date(serial: f64) -> Option<NaiveDate> {
    if !serial.is_finite() {
        return None;
    }
    let epoch = NaiveDate::from_ymd_opt(1899, 12, 30)?;
    let days = serial.floor();
    if days < (NaiveDate::MIN - epoch).num_days() as f64
        || days > (NaiveDate::MAX - epoch).num_days() as f64
    {
        return None;
    }
    epoch.checked_add_signed(Duration::days(days as i64))
}

#[derive(Clone, Debug, Default)]
pub struct Evaluation {
    values: BTreeMap<(u64, CellAddr), CellValue>,
}

impl Evaluation {
    pub fn get(&self, sheet: u64, addr: CellAddr) -> Option<&CellValue> {
        self.values.get(&(sheet, addr))
    }

    pub fn value(&self, sheet: u64, addr: CellAddr) -> CellValue {
        self.get(sheet, addr).cloned().unwrap_or(CellValue::Blank)
    }
}

fn numeric(value: CellValue) -> std::result::Result<f64, CellError> {
    match value {
        CellValue::Number(value) => Ok(value),
        CellValue::Blank => Ok(0.0),
        CellValue::Bool(value) => Ok(if value { 1.0 } else { 0.0 }),
        CellValue::Date(date) => Ok(date_to_serial(date)),
        CellValue::Error(error) => Err(error),
        CellValue::Text(_) => Err(CellError::Value),
    }
}

fn finite(value: f64) -> CellValue {
    if value.is_finite() {
        CellValue::Number(value)
    } else {
        CellValue::Error(CellError::Num)
    }
}

/// Plain/scientific numbers, percentages and common statement-style amounts.
/// Commas are accepted only as three-digit groups, never as decimal separators.
pub fn parse_number(raw: &str) -> Option<f64> {
    let mut text = raw.trim();
    if text.is_empty() {
        return None;
    }
    let accounting = text.starts_with('(') && text.ends_with(')');
    if accounting {
        text = text.get(1..text.len() - 1)?.trim();
    }
    let percent = text.ends_with('%');
    if percent {
        text = text.strip_suffix('%')?.trim();
    }
    let negative_prefix = text.starts_with('-');
    let positive_prefix = text.starts_with('+');
    if negative_prefix || positive_prefix {
        text = &text[1..];
    }
    text = text.trim();
    let mut currency = false;
    if let Some(first) = text.chars().next().filter(|c| "$€£¥₹".contains(*c)) {
        currency = true;
        text = text.get(first.len_utf8()..)?.trim();
    }
    for code in ["USD", "EUR", "GBP", "JPY", "CAD", "AUD", "CHF", "INR"] {
        if let Some(rest) = text.strip_prefix(code) {
            currency = true;
            text = rest.trim();
            break;
        }
        if let Some(rest) = text.strip_suffix(code) {
            currency = true;
            text = rest.trim();
            break;
        }
    }
    if currency && percent {
        return None;
    }
    if (negative_prefix || positive_prefix) && text.starts_with(['+', '-']) {
        return None;
    }
    let normalized;
    let numeric = if text.contains(',') {
        let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
        let integer = unsigned.split(['.', 'e', 'E']).next()?;
        let groups: Vec<_> = integer.split(',').collect();
        if groups.len() < 2
            || groups[0].is_empty()
            || groups[0].len() > 3
            || !groups[0].bytes().all(|c| c.is_ascii_digit())
            || groups[1..]
                .iter()
                .any(|group| group.len() != 3 || !group.bytes().all(|c| c.is_ascii_digit()))
            || unsigned.get(integer.len()..)?.contains(',')
        {
            return None;
        }
        normalized = text.replace(',', "");
        normalized.as_str()
    } else {
        text
    };
    let mut number = numeric.parse::<f64>().ok()?;
    if negative_prefix {
        number = -number;
    }
    if accounting {
        if number < 0.0 || negative_prefix || positive_prefix {
            return None;
        }
        number = -number;
    }
    if percent {
        number /= 100.0;
    }
    Some(number)
}

fn literal(raw: &str) -> Option<CellValue> {
    Some(if let Some(text) = raw.strip_prefix('\'') {
        CellValue::Text(text.into())
    } else if raw.trim_start().starts_with('=') {
        return None;
    } else if raw.is_empty() {
        CellValue::Blank
    } else if let Some(date) = parse_date(raw) {
        CellValue::Date(date)
    } else if raw.trim().eq_ignore_ascii_case("TRUE") || raw.trim().eq_ignore_ascii_case("FALSE") {
        CellValue::Bool(raw.trim().eq_ignore_ascii_case("TRUE"))
    } else {
        match parse_number(raw) {
            Some(value) => finite(value),
            None => CellValue::Text(raw.into()),
        }
    })
}

enum ExprValue {
    Scalar(CellValue),
    Range(Vec<CellValue>),
}

impl ExprValue {
    fn scalar(self) -> CellValue {
        match self {
            Self::Scalar(value) => value,
            Self::Range(_) => CellValue::Error(CellError::Value),
        }
    }
}

struct Recalculator<'a> {
    book: &'a Workbook,
    values: BTreeMap<(u64, CellAddr), CellValue>,
    active: HashSet<(u64, CellAddr)>,
    steps: usize,
    today: NaiveDate,
}

impl Recalculator<'_> {
    fn step(&mut self, depth: usize) -> std::result::Result<(), CellError> {
        self.steps += 1;
        if depth > MAX_EVAL_DEPTH || self.steps > MAX_EVAL_STEPS {
            Err(CellError::Limit)
        } else {
            Ok(())
        }
    }

    fn cell(&mut self, sheet: u64, addr: CellAddr, depth: usize) -> CellValue {
        if let Err(error) = self.step(depth) {
            return CellValue::Error(error);
        }
        if !addr.in_bounds() || self.book.sheet(sheet).is_none() {
            return CellValue::Error(CellError::Ref);
        }
        let key = (sheet, addr);
        if let Some(value) = self.values.get(&key) {
            return value.clone();
        }
        if !self.active.insert(key) {
            return CellValue::Error(CellError::Cycle);
        }
        let raw = self.book.raw(sheet, addr);
        let value = match literal(raw) {
            Some(value) => value,
            None => match sheet_formula::parse(raw) {
                Ok(expr) => self.expr(&expr, sheet, depth + 1).scalar(),
                Err(error) => CellValue::Error(error),
            },
        };
        self.active.remove(&key);
        self.values.insert(key, value.clone());
        value
    }

    fn sheet_id(&self, current: u64, reference: &Reference) -> std::result::Result<u64, CellError> {
        match &reference.sheet {
            None => Ok(current),
            Some(name) => self
                .book
                .sheets
                .iter()
                .find(|sheet| sheet.name.eq_ignore_ascii_case(name))
                .map(|sheet| sheet.id)
                .ok_or(CellError::Ref),
        }
    }

    fn expr(&mut self, expr: &Expr, sheet: u64, depth: usize) -> ExprValue {
        if let Err(error) = self.step(depth) {
            return ExprValue::Scalar(CellValue::Error(error));
        }
        let value = match expr {
            Expr::Number(value) => finite(*value),
            Expr::Text(value) => CellValue::Text(value.clone()),
            Expr::Bool(value) => CellValue::Bool(*value),
            Expr::Error(error) => CellValue::Error(*error),
            Expr::Ref(reference) => match self.sheet_id(sheet, reference) {
                Ok(sheet) => match self.cell(
                    sheet,
                    CellAddr::new(reference.row, reference.col),
                    depth + 1,
                ) {
                    CellValue::Blank => CellValue::Number(0.0),
                    value => value,
                },
                Err(error) => CellValue::Error(error),
            },
            Expr::Range(first, last) => {
                let result = self.range(sheet, first, last, depth + 1);
                return match result {
                    Ok(values) => ExprValue::Range(values),
                    Err(error) => ExprValue::Scalar(CellValue::Error(error)),
                };
            }
            Expr::Unary(op, expression) => {
                let value = self.expr(expression, sheet, depth + 1).scalar();
                match numeric(value) {
                    Ok(value) => finite(match op {
                        UnaryOp::Plus => value,
                        UnaryOp::Minus => -value,
                        UnaryOp::Percent => value / 100.0,
                    }),
                    Err(error) => CellValue::Error(error),
                }
            }
            Expr::Binary(op, left, right) => {
                let left = self.expr(left, sheet, depth + 1).scalar();
                let right = self.expr(right, sheet, depth + 1).scalar();
                Self::binary(*op, left, right)
            }
            Expr::Call(name, args) => self.function(name, args, sheet, depth + 1),
        };
        ExprValue::Scalar(value)
    }

    fn range(
        &mut self,
        current: u64,
        first: &Reference,
        last: &Reference,
        depth: usize,
    ) -> std::result::Result<Vec<CellValue>, CellError> {
        let sheet = self.sheet_id(current, first)?;
        let other = match &last.sheet {
            None => sheet,
            Some(_) => self.sheet_id(current, last)?,
        };
        if sheet != other {
            return Err(CellError::Ref);
        }
        let range = CellRange::new(
            CellAddr::new(first.row, first.col),
            CellAddr::new(last.row, last.col),
        );
        if !range.start.in_bounds() || !range.end.in_bounds() {
            return Err(CellError::Ref);
        }
        let count = range.rows() * range.cols();
        if count > MAX_EVAL_STEPS.saturating_sub(self.steps) {
            return Err(CellError::Limit);
        }
        let mut values = Vec::with_capacity(count);
        for row in range.start.row..=range.end.row {
            for col in range.start.col..=range.end.col {
                values.push(self.cell(sheet, CellAddr::new(row, col), depth + 1));
            }
        }
        Ok(values)
    }

    fn binary(op: BinaryOp, left: CellValue, right: CellValue) -> CellValue {
        if let CellValue::Error(error) = left {
            return CellValue::Error(error);
        }
        if let CellValue::Error(error) = right {
            return CellValue::Error(error);
        }
        let shifted_date = |date: NaiveDate, days: f64| {
            let value = date_to_serial(date) + days;
            if days.fract() == 0.0 {
                serial_to_date(value).map_or(CellValue::Error(CellError::Num), CellValue::Date)
            } else {
                finite(value)
            }
        };
        match (&left, &right, op) {
            (CellValue::Date(left), CellValue::Date(right), BinaryOp::Subtract) => {
                return finite((*left - *right).num_days() as f64);
            }
            (CellValue::Date(date), value, BinaryOp::Add | BinaryOp::Subtract)
                if !matches!(value, CellValue::Date(_)) =>
            {
                return match numeric(value.clone()) {
                    Ok(days) => shifted_date(
                        *date,
                        if op == BinaryOp::Subtract {
                            -days
                        } else {
                            days
                        },
                    ),
                    Err(error) => CellValue::Error(error),
                };
            }
            (value, CellValue::Date(date), BinaryOp::Add)
                if !matches!(value, CellValue::Date(_)) =>
            {
                return match numeric(value.clone()) {
                    Ok(days) => shifted_date(*date, days),
                    Err(error) => CellValue::Error(error),
                };
            }
            _ => {}
        }
        if matches!(
            op,
            BinaryOp::Equal
                | BinaryOp::NotEqual
                | BinaryOp::Less
                | BinaryOp::LessEqual
                | BinaryOp::Greater
                | BinaryOp::GreaterEqual
        ) {
            let ordering = match (&left, &right) {
                (CellValue::Text(left), CellValue::Text(right)) => {
                    Some(left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase()))
                }
                (CellValue::Text(_), _) | (_, CellValue::Text(_)) => None,
                _ => numeric(left)
                    .ok()
                    .zip(numeric(right).ok())
                    .and_then(|(left, right)| left.partial_cmp(&right)),
            };
            return CellValue::Bool(match op {
                BinaryOp::Equal => ordering.is_some_and(|order| order.is_eq()),
                BinaryOp::NotEqual => !ordering.is_some_and(|order| order.is_eq()),
                BinaryOp::Less => ordering.is_some_and(|order| order.is_lt()),
                BinaryOp::LessEqual => ordering.is_some_and(|order| order.is_le()),
                BinaryOp::Greater => ordering.is_some_and(|order| order.is_gt()),
                _ => ordering.is_some_and(|order| order.is_ge()),
            });
        }
        let (left, right) =
            match numeric(left).and_then(|left| numeric(right).map(|right| (left, right))) {
                Ok(values) => values,
                Err(error) => return CellValue::Error(error),
            };
        if matches!(op, BinaryOp::Divide) && right == 0.0 {
            return CellValue::Error(CellError::DivZero);
        }
        finite(match op {
            BinaryOp::Add => left + right,
            BinaryOp::Subtract => left - right,
            BinaryOp::Multiply => left * right,
            BinaryOp::Divide => left / right,
            BinaryOp::Power => left.powf(right),
            _ => unreachable!("Comparison handled above"),
        })
    }

    fn function(&mut self, name: &str, args: &[Expr], sheet: u64, depth: usize) -> CellValue {
        if name == "TRUE" || name == "FALSE" {
            return if args.is_empty() {
                CellValue::Bool(name == "TRUE")
            } else {
                CellValue::Error(CellError::Value)
            };
        }
        if name == "TODAY" {
            return if args.is_empty() {
                CellValue::Date(self.today)
            } else {
                CellValue::Error(CellError::Value)
            };
        }
        if matches!(name, "DAY" | "MONTH" | "YEAR") {
            if args.len() != 1 {
                return CellValue::Error(CellError::Value);
            }
            let value = self.expr(&args[0], sheet, depth + 1).scalar();
            if let CellValue::Error(error) = value {
                return CellValue::Error(error);
            }
            let Some(date) = value.as_date() else {
                return CellValue::Error(CellError::Value);
            };
            return finite(match name {
                "DAY" => date.day() as f64,
                "MONTH" => date.month() as f64,
                _ => date.year() as f64,
            });
        }
        if name == "DATE" {
            if args.len() != 3 {
                return CellValue::Error(CellError::Value);
            }
            let mut values = Vec::with_capacity(3);
            for arg in args {
                match numeric(self.expr(arg, sheet, depth + 1).scalar()) {
                    Ok(value) if value.is_finite() && value.abs() <= 3_660_000.0 => {
                        values.push(value.trunc() as i64)
                    }
                    Ok(_) => return CellValue::Error(CellError::Num),
                    Err(error) => return CellValue::Error(error),
                }
            }
            let months = values[0] * 12 + values[1] - 1;
            let year = months.div_euclid(12);
            let month = months.rem_euclid(12) + 1;
            if !(1..=9999).contains(&year) {
                return CellValue::Error(CellError::Num);
            }
            return NaiveDate::from_ymd_opt(year as i32, month as u32, 1)
                .and_then(|date| date.checked_add_signed(Duration::days(values[2] - 1)))
                .filter(|date| (1..=9999).contains(&date.year()))
                .map_or(CellValue::Error(CellError::Num), CellValue::Date);
        }
        if name == "IF" {
            if !(2..=3).contains(&args.len()) {
                return CellValue::Error(CellError::Value);
            }
            let condition = self.expr(&args[0], sheet, depth + 1).scalar();
            return match numeric(condition) {
                Ok(value) if value != 0.0 => self.expr(&args[1], sheet, depth + 1).scalar(),
                Ok(_) => args.get(2).map_or(CellValue::Bool(false), |expression| {
                    self.expr(expression, sheet, depth + 1).scalar()
                }),
                Err(error) => CellValue::Error(error),
            };
        }
        if name == "ABS" || name == "ROUND" {
            if args.len() != if name == "ABS" { 1 } else { 2 } {
                return CellValue::Error(CellError::Value);
            }
            let value = self.expr(&args[0], sheet, depth + 1).scalar();
            let value = match numeric(value) {
                Ok(value) => value,
                Err(error) => return CellValue::Error(error),
            };
            if name == "ABS" {
                return finite(value.abs());
            }
            let places = self.expr(&args[1], sheet, depth + 1).scalar();
            let places = match numeric(places) {
                Ok(value) if (-15.0..=15.0).contains(&value) => value.trunc() as i32,
                Ok(_) => return CellValue::Error(CellError::Num),
                Err(error) => return CellValue::Error(error),
            };
            let factor = 10_f64.powi(places);
            return finite((value * factor).round() / factor);
        }
        if !matches!(name, "SUM" | "AVERAGE" | "MIN" | "MAX" | "COUNT") {
            return CellValue::Error(CellError::Name);
        }
        let mut numbers = Vec::new();
        for arg in args {
            let values = match self.expr(arg, sheet, depth + 1) {
                ExprValue::Scalar(value) => vec![value],
                ExprValue::Range(values) => values,
            };
            for value in values {
                match value {
                    CellValue::Number(value) => numbers.push(value),
                    CellValue::Bool(value) => numbers.push(if value { 1.0 } else { 0.0 }),
                    CellValue::Date(date) => numbers.push(date_to_serial(date)),
                    CellValue::Error(error) if name != "COUNT" || error == CellError::Limit => {
                        return CellValue::Error(error)
                    }
                    _ => {}
                }
            }
        }
        match name {
            "COUNT" => finite(numbers.len() as f64),
            "MIN" => finite(numbers.into_iter().reduce(f64::min).unwrap_or(0.0)),
            "MAX" => finite(numbers.into_iter().reduce(f64::max).unwrap_or(0.0)),
            "AVERAGE" if numbers.is_empty() => CellValue::Error(CellError::DivZero),
            "AVERAGE" => finite(numbers.iter().sum::<f64>() / numbers.len() as f64),
            _ => finite(numbers.iter().sum()),
        }
    }
}

/// Parse bounded RFC4180-style comma or tab delimited text. Quoted fields may
/// contain delimiters, doubled quotes, and line breaks. An initial BOM is ignored.
pub fn parse_delimited(text: &str, delimiter: char) -> Result<Vec<Vec<String>>> {
    if !matches!(delimiter, ',' | '\t') {
        return Err("Use comma or tab separated values".into());
    }
    if text.len() > 2 * MAX_RAW_BYTES
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\t' | '\r' | '\n'))
    {
        return Err(
            "Delimited input is too large or contains an unsupported control character".into(),
        );
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.is_empty() {
        return Ok(vec![]);
    }
    let mut chars = text.chars().peekable();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut closed = false;
    let mut ended_row = false;
    while let Some(c) = chars.next() {
        ended_row = false;
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                field.push(c);
            }
        } else if c == delimiter {
            row.push(std::mem::take(&mut field));
            closed = false;
            if row.len() >= MAX_COLS {
                return Err("Delimited input has too many columns".into());
            }
        } else if matches!(c, '\n' | '\r') {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut row));
            closed = false;
            ended_row = true;
            if rows.len() > MAX_ROWS {
                return Err("Delimited input has too many rows".into());
            }
        } else if c == '"' && field.is_empty() && !closed {
            quoted = true;
        } else if closed || c == '"' {
            return Err("Invalid quoting in delimited text".into());
        } else {
            field.push(c);
        }
        if field.len() > MAX_CELL_BYTES {
            return Err("A cell exceeds 4096 bytes".into());
        }
    }
    if quoted {
        return Err("Unclosed quote in delimited text".into());
    }
    if !ended_row {
        row.push(field);
        rows.push(row);
    }
    if rows.len() > MAX_ROWS || rows.iter().any(|row| row.len() > MAX_COLS) {
        return Err("Delimited input exceeds the grid bounds".into());
    }
    let bytes: usize = rows.iter().flatten().map(String::len).sum();
    if bytes > MAX_RAW_BYTES {
        return Err("Delimited cell contents exceed 1 MiB".into());
    }
    Ok(rows)
}

pub fn encode_delimited(rows: &[Vec<String>], delimiter: char) -> String {
    let separator = delimiter.to_string();
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|field| {
                    if field.contains([delimiter, '"', '\r', '\n']) {
                        format!("\"{}\"", field.replace('"', "\"\""))
                    } else {
                        field.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(&separator)
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}
