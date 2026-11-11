//! Bounded chart data and shared vector scenes for the UI and document exports.
use crate::{
    model::{State, TaskColor},
    spreadsheet::{CellAddr, CellRange, CellValue, Workbook},
    timeline, Result,
};
use chrono::{Datelike, Local, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

pub const MAX_CHART_ROWS: usize = 200;
pub type Color = [u8; 3];
const PALETTE: [Color; 6] = [
    [234, 133, 173],
    [218, 185, 126],
    [148, 188, 160],
    [109, 185, 238],
    [179, 158, 207],
    [144, 161, 202],
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartKind {
    Pie,
    Bar,
    Gantt,
}

impl ChartKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pie => "Pie",
            Self::Bar => "Bar",
            Self::Gantt => "Gantt",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChartSource {
    Range { sheet: u64, range: CellRange },
    Tasks,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChartSpec {
    pub id: u64,
    pub title: String,
    pub kind: ChartKind,
    pub source: ChartSource,
}

impl ChartSpec {
    pub fn validate(&self, book: &Workbook) -> Result<()> {
        if self.id == 0
            || self.title.trim() != self.title
            || self.title.is_empty()
            || self.title.chars().count() > 120
            || self.title.chars().any(char::is_control)
        {
            return Err("Charts need a nonzero ID and a title of 1–120 characters".into());
        }
        match &self.source {
            ChartSource::Tasks if self.kind != ChartKind::Gantt => {
                Err("The task source is available for Gantt charts".into())
            }
            ChartSource::Tasks => Ok(()),
            ChartSource::Range { sheet, range } => {
                if book.sheet(*sheet).is_none()
                    || !range.start.in_bounds()
                    || !range.end.in_bounds()
                    || range.start.row > range.end.row
                    || range.start.col > range.end.col
                {
                    return Err("Chart source is outside an existing sheet".into());
                }
                let columns = if self.kind == ChartKind::Gantt { 3 } else { 2 };
                if range.cols() < columns || range.rows() > MAX_CHART_ROWS + 1 {
                    return Err(format!(
                        "{} charts need {columns} columns and at most {MAX_CHART_ROWS} data rows",
                        self.kind.label()
                    ));
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Datum {
    pub label: String,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScheduleItem {
    pub label: String,
    pub start: Option<NaiveDate>,
    pub due: NaiveDate,
    pub complete: bool,
    pub color: Color,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChartData {
    Series(Vec<Datum>),
    Schedule(Vec<ScheduleItem>),
}

pub fn chart_data<Tz: TimeZone>(
    state: &State,
    spec: &ChartSpec,
    timezone: &Tz,
) -> Result<ChartData> {
    chart_data_at(
        state,
        spec,
        timezone,
        Utc::now().with_timezone(timezone).date_naive(),
    )
}

pub fn chart_data_at<Tz: TimeZone>(
    state: &State,
    spec: &ChartSpec,
    timezone: &Tz,
    today: NaiveDate,
) -> Result<ChartData> {
    let empty = Workbook::default();
    let book = state.workbook.as_ref().unwrap_or(&empty);
    spec.validate(book)?;
    if spec.source == ChartSource::Tasks {
        let rows = timeline::rows(state, timezone);
        let mut items = Vec::new();
        for row in rows {
            if let Some(due) = row.due {
                items.push(ScheduleItem {
                    label: row.title.to_owned(),
                    start: row.start,
                    due,
                    complete: row.complete,
                    color: palette_color(row.color),
                });
            }
        }
        if items.is_empty() {
            return Err("Add a task deadline to see its Gantt chart".into());
        }
        if items.len() > MAX_CHART_ROWS {
            return Err(format!(
                "A chart can show at most {MAX_CHART_ROWS} dated tasks"
            ));
        }
        return Ok(ChartData::Schedule(items));
    }
    let ChartSource::Range { sheet, range } = spec.source else {
        unreachable!("Task sources were handled above")
    };
    let values = book.recalculate_at(today);
    let mut series = Vec::new();
    let mut schedule = Vec::new();
    for row in range.start.row..=range.end.row {
        let label = values.value(sheet, CellAddr::new(row, range.start.col));
        let first = values.value(sheet, CellAddr::new(row, range.start.col + 1));
        let label = label.csv_value();
        if spec.kind == ChartKind::Gantt {
            let last = values.value(sheet, CellAddr::new(row, range.start.col + 2));
            if label.trim().is_empty() && is_blank(&first) && is_blank(&last) {
                continue;
            }
            let start = cell_date(&first);
            let due = cell_date(&last);
            if row == range.start.row && due.is_none() && matches!(last, CellValue::Text(_)) {
                continue;
            }
            let due = due.ok_or_else(|| {
                format!(
                    "Chart row {} needs a valid due date in its third column",
                    row + 1
                )
            })?;
            if !is_blank(&first) && start.is_none() {
                return Err(format!("Chart row {} needs a valid start date", row + 1));
            }
            if start.is_some_and(|start| start > due) {
                return Err(format!("Chart row {} starts after its due date", row + 1));
            }
            schedule.push(ScheduleItem {
                label: if label.is_empty() {
                    format!("Row {}", row + 1)
                } else {
                    label
                },
                start,
                due,
                complete: false,
                color: PALETTE[schedule.len() % PALETTE.len()],
            });
        } else {
            if label.trim().is_empty() && is_blank(&first) {
                continue;
            }
            if row == range.start.row && matches!(first, CellValue::Text(_)) {
                continue;
            }
            let CellValue::Number(value) = first else {
                return Err(format!(
                    "Chart row {} needs a number in its second column",
                    row + 1
                ));
            };
            if !value.is_finite() || (spec.kind == ChartKind::Pie && value < 0.0) {
                return Err("Pie values must be nonnegative and all chart values finite".into());
            }
            series.push(Datum {
                label: if label.is_empty() {
                    format!("Row {}", row + 1)
                } else {
                    label
                },
                value,
            });
        }
    }
    if series.len().max(schedule.len()) > MAX_CHART_ROWS {
        return Err(format!("A chart can show at most {MAX_CHART_ROWS} rows"));
    }
    if spec.kind == ChartKind::Gantt {
        if schedule.is_empty() {
            return Err("Select labels, start dates and due dates for the Gantt chart".into());
        }
        Ok(ChartData::Schedule(schedule))
    } else {
        if series.is_empty() {
            return Err("Select labels and numeric values for this chart".into());
        }
        if spec.kind == ChartKind::Pie {
            let total: f64 = series.iter().map(|item| item.value).sum();
            if !total.is_finite() || total <= 0.0 {
                return Err("A pie chart needs a positive finite total".into());
            }
        }
        Ok(ChartData::Series(series))
    }
}

fn is_blank(value: &CellValue) -> bool {
    matches!(value, CellValue::Blank) || matches!(value, CellValue::Text(text) if text.is_empty())
}
fn cell_date(value: &CellValue) -> Option<NaiveDate> {
    value.as_date()
}

fn palette_color(color: Option<TaskColor>) -> Color {
    match color {
        Some(TaskColor::Rose) => PALETTE[0],
        Some(TaskColor::Amber) => PALETTE[1],
        Some(TaskColor::Sage) => PALETTE[2],
        Some(TaskColor::Blue) => PALETTE[3],
        Some(TaskColor::Lilac) => PALETTE[4],
        None => PALETTE[5],
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub color: Color,
    pub width: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextAnchor {
    Start,
    Middle,
    End,
}

/// Coordinates use top-left origin; text Y is the baseline in every backend.
#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    Rect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        stroke: Stroke,
    },
    Text {
        x: f64,
        y: f64,
        text: String,
        size: f64,
        color: Color,
        anchor: TextAnchor,
    },
    Polygon {
        points: Vec<[f64; 2]>,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChartScene {
    pub width: f64,
    pub height: f64,
    pub title: String,
    pub primitives: Vec<Primitive>,
}

#[derive(Clone, Copy)]
struct Theme {
    background: Color,
    text: Color,
    muted: Color,
    rule: Color,
}

impl Theme {
    fn new(light: bool) -> Self {
        if light {
            Self {
                background: [255, 255, 255],
                text: [31, 39, 48],
                muted: [84, 96, 112],
                rule: [215, 222, 230],
            }
        } else {
            Self {
                background: [12, 19, 27],
                text: [246, 218, 174],
                muted: [144, 161, 202],
                rule: [49, 65, 83],
            }
        }
    }
}

pub fn scene(state: &State, spec: &ChartSpec, width: f64, height: f64) -> Result<ChartScene> {
    scene_in_timezone(state, spec, width, height, false, &Local)
}

pub fn scene_light(state: &State, spec: &ChartSpec, width: f64, height: f64) -> Result<ChartScene> {
    scene_in_timezone(state, spec, width, height, true, &Local)
}

pub fn scene_in_timezone<Tz: TimeZone>(
    state: &State,
    spec: &ChartSpec,
    width: f64,
    height: f64,
    light: bool,
    timezone: &Tz,
) -> Result<ChartScene> {
    scene_at(
        state,
        spec,
        width,
        height,
        light,
        timezone,
        Utc::now().with_timezone(timezone).date_naive(),
    )
}

pub fn scene_at<Tz: TimeZone>(
    state: &State,
    spec: &ChartSpec,
    width: f64,
    height: f64,
    light: bool,
    timezone: &Tz,
    today: NaiveDate,
) -> Result<ChartScene> {
    if !width.is_finite()
        || !height.is_finite()
        || !(280.0..=4096.0).contains(&width)
        || !(180.0..=12000.0).contains(&height)
    {
        return Err("Use a finite chart width of 280–4096 and height of 180–12000".into());
    }
    let data = chart_data_at(state, spec, timezone, today)?;
    let theme = Theme::new(light);
    let rows = match &data {
        ChartData::Series(rows) => rows.len(),
        ChartData::Schedule(rows) => rows.len(),
    };
    let needed = match spec.kind {
        ChartKind::Pie => rows as f64 * 22.0 + 92.0,
        _ => rows as f64 * 30.0 + 110.0,
    };
    let mut scene = ChartScene {
        width,
        height: height.max(needed),
        title: spec.title.clone(),
        primitives: Vec::new(),
    };
    scene.primitives.push(Primitive::Rect {
        x: 0.0,
        y: 0.0,
        width,
        height: scene.height,
        fill: Some(theme.background),
        stroke: None,
    });
    text(
        &mut scene,
        18.0,
        28.0,
        &spec.title,
        16.0,
        theme.text,
        TextAnchor::Start,
    );
    match data {
        ChartData::Series(items) if spec.kind == ChartKind::Pie => {
            pie(&mut scene, &items, theme, state.preferences.decimals)
        }
        ChartData::Series(items) => bars(&mut scene, &items, theme, state.preferences.decimals),
        ChartData::Schedule(items) => gantt(&mut scene, &items, theme)?,
    }
    Ok(scene)
}

fn text(
    scene: &mut ChartScene,
    x: f64,
    y: f64,
    value: &str,
    size: f64,
    color: Color,
    anchor: TextAnchor,
) {
    scene.primitives.push(Primitive::Text {
        x,
        y,
        text: value.to_owned(),
        size,
        color,
        anchor,
    });
}

fn short(value: &str, columns: usize) -> String {
    if value.chars().count() <= columns {
        value.to_owned()
    } else {
        value
            .chars()
            .take(columns.saturating_sub(1))
            .chain(std::iter::once('…'))
            .collect()
    }
}

fn blend(color: Color, background: Color, amount: f64) -> Color {
    std::array::from_fn(|index| {
        (f64::from(color[index]) * amount + f64::from(background[index]) * (1.0 - amount)).round()
            as u8
    })
}

fn bars(scene: &mut ChartScene, items: &[Datum], theme: Theme, decimals: usize) {
    let label_width = (scene.width * 0.30).clamp(90.0, 230.0);
    let left = label_width + 20.0;
    let right = scene.width - 80.0;
    let extent = items
        .iter()
        .map(|item| item.value.abs())
        .fold(0.0_f64, f64::max);
    let extent = if extent == 0.0 { 1.0 } else { extent };
    let has_negative = items.iter().any(|item| item.value < 0.0);
    let zero = if has_negative {
        (left + right) * 0.5
    } else {
        left
    };
    let scale = if has_negative {
        (right - left) * 0.5
    } else {
        right - left
    };
    scene.primitives.push(Primitive::Line {
        x1: zero,
        y1: 50.0,
        x2: zero,
        y2: 67.0 + items.len() as f64 * 30.0,
        stroke: Stroke {
            color: theme.rule,
            width: 1.0,
        },
    });
    for (index, item) in items.iter().enumerate() {
        let y = 66.0 + index as f64 * 30.0;
        text(
            scene,
            18.0,
            y + 4.0,
            &short(&item.label, (label_width / 7.2) as usize),
            12.0,
            theme.text,
            TextAnchor::Start,
        );
        let end = zero + (item.value / extent) * scale;
        scene.primitives.push(Primitive::Rect {
            x: zero.min(end),
            y: y - 9.0,
            width: (end - zero).abs(),
            height: 18.0,
            fill: Some(PALETTE[3]),
            stroke: None,
        });
        text(
            scene,
            scene.width - 18.0,
            y + 4.0,
            &format!("{:.*}", decimals.min(10), item.value),
            12.0,
            theme.text,
            TextAnchor::End,
        );
    }
}

fn pie(scene: &mut ChartScene, items: &[Datum], theme: Theme, decimals: usize) {
    let total: f64 = items.iter().map(|item| item.value).sum();
    let compact = scene.width < 620.0;
    let radius = if compact {
        ((scene.width - 48.0) * 0.5).min(120.0)
    } else {
        (scene.width * 0.23).min(155.0)
    };
    let center = if compact {
        [scene.width * 0.5, 52.0 + radius]
    } else {
        [24.0 + radius, 58.0 + radius]
    };
    let legend_x = if compact {
        20.0
    } else {
        center[0] + radius + 32.0
    };
    let legend_y = if compact {
        center[1] + radius + 36.0
    } else {
        66.0
    };
    if compact {
        scene.height = scene
            .height
            .max(legend_y + items.len() as f64 * 22.0 + 26.0);
        if let Some(Primitive::Rect { height, .. }) = scene.primitives.first_mut() {
            *height = scene.height;
        }
    }
    let mut angle = -std::f64::consts::FRAC_PI_2;
    for (index, item) in items.iter().enumerate() {
        let fraction = item.value / total;
        let end = angle + fraction * std::f64::consts::TAU;
        if fraction > 0.0 {
            let steps = ((fraction * 120.0).ceil() as usize).max(2);
            let mut points = vec![center];
            for step in 0..=steps {
                let at = angle + (end - angle) * step as f64 / steps as f64;
                points.push([center[0] + at.cos() * radius, center[1] + at.sin() * radius]);
            }
            scene.primitives.push(Primitive::Polygon {
                points,
                fill: Some(PALETTE[index % PALETTE.len()]),
                stroke: Some(Stroke {
                    color: theme.background,
                    width: 1.0,
                }),
            });
        }
        let y = legend_y + index as f64 * 22.0;
        scene.primitives.push(Primitive::Rect {
            x: legend_x,
            y: y - 9.0,
            width: 10.0,
            height: 10.0,
            fill: Some(PALETTE[index % PALETTE.len()]),
            stroke: None,
        });
        let columns = ((scene.width - legend_x - 130.0) / 7.0).max(6.0) as usize;
        text(
            scene,
            legend_x + 18.0,
            y,
            &short(&item.label, columns),
            11.0,
            theme.text,
            TextAnchor::Start,
        );
        text(
            scene,
            scene.width - 18.0,
            y,
            &format!(
                "{:.*} · {:.1}%",
                decimals.min(10),
                item.value,
                fraction * 100.0
            ),
            11.0,
            theme.muted,
            TextAnchor::End,
        );
        angle = end;
    }
}

fn gantt(scene: &mut ChartScene, items: &[ScheduleItem], theme: Theme) -> Result<()> {
    let first = items
        .iter()
        .map(|item| item.start.unwrap_or(item.due))
        .min()
        .ok_or("No scheduled rows")?;
    let last = items
        .iter()
        .map(|item| item.due)
        .max()
        .ok_or("No scheduled rows")?;
    let days = (last - first).num_days() + 1;
    if days <= 0 || days > 36_600 {
        return Err("Gantt dates must fit within a 100-year range".into());
    }
    let label_width = (scene.width * 0.32).clamp(90.0, 250.0);
    let left = label_width + 16.0;
    let right = scene.width - 20.0;
    let cell = (right - left) / days as f64;
    let stride = ((days as f64 / ((right - left) / 72.0).max(1.0)).ceil() as i64).max(1);
    let mut offset = 0;
    while offset < days {
        let date = first
            .checked_add_signed(chrono::Duration::days(offset))
            .ok_or("Date outside supported range")?;
        let x = left + (offset as f64 + 0.5) * cell;
        scene.primitives.push(Primitive::Line {
            x1: x,
            y1: 61.0,
            x2: x,
            y2: 91.0 + items.len() as f64 * 30.0,
            stroke: Stroke {
                color: theme.rule,
                width: 0.6,
            },
        });
        let date_label = if first.year() != last.year() {
            date.format("%d %b %Y").to_string()
        } else {
            date.format("%d %b").to_string()
        };
        text(
            scene,
            x,
            56.0,
            &date_label,
            10.0,
            theme.muted,
            TextAnchor::Middle,
        );
        offset += stride;
    }
    for (index, item) in items.iter().enumerate() {
        let y = 82.0 + index as f64 * 30.0;
        let color = if item.complete {
            blend(item.color, theme.background, 0.5)
        } else {
            item.color
        };
        text(
            scene,
            18.0,
            y,
            &short(&item.label, (label_width / 7.0) as usize),
            11.0,
            if item.complete {
                theme.muted
            } else {
                theme.text
            },
            TextAnchor::Start,
        );
        text(
            scene,
            18.0,
            y + 12.0,
            if item.complete { "Done" } else { "Open" },
            8.0,
            theme.muted,
            TextAnchor::Start,
        );
        let due = (item.due - first).num_days() as f64;
        if let Some(start) = item.start {
            let start = (start - first).num_days() as f64;
            let x = left + start * cell;
            scene.primitives.push(Primitive::Rect {
                x,
                y: y - 8.0,
                width: ((due - start + 1.0) * cell).max(1.0),
                height: 11.0,
                fill: Some(if item.complete {
                    blend(color, theme.background, 0.2)
                } else {
                    color
                }),
                stroke: Some(Stroke { color, width: 0.7 }),
            });
        } else {
            let x = left + (due + 0.5) * cell;
            scene.primitives.push(Primitive::Polygon {
                points: vec![
                    [x - 4.0, y - 2.0],
                    [x, y - 7.0],
                    [x + 4.0, y - 2.0],
                    [x, y + 3.0],
                ],
                fill: Some(if item.complete {
                    theme.background
                } else {
                    color
                }),
                stroke: Some(Stroke { color, width: 0.8 }),
            });
        }
    }
    Ok(())
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn color(value: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", value[0], value[1], value[2])
}

pub fn to_svg(scene: &ChartScene) -> String {
    let mut out = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\" role=\"img\"><title>{}</title>", scene.width, scene.height, scene.width, scene.height, escape(&scene.title));
    let attributes = |fill: Option<Color>, stroke: Option<Stroke>| -> String {
        let mut attrs = format!(" fill=\"{}\"", fill.map_or_else(|| "none".into(), color));
        if let Some(stroke) = stroke {
            let _ = write!(
                attrs,
                " stroke=\"{}\" stroke-width=\"{}\"",
                color(stroke.color),
                stroke.width
            );
        }
        attrs
    };
    for item in &scene.primitives {
        match item {
            Primitive::Rect {
                x,
                y,
                width,
                height,
                fill,
                stroke,
            } => {
                let _ = write!(
                    out,
                    "<rect x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\"{}/>",
                    attributes(*fill, *stroke)
                );
            }
            Primitive::Line {
                x1,
                y1,
                x2,
                y2,
                stroke,
            } => {
                let _ = write!(out, "<line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\" stroke=\"{}\" stroke-width=\"{}\"/>", color(stroke.color), stroke.width);
            }
            Primitive::Text {
                x,
                y,
                text,
                size,
                color: ink,
                anchor,
            } => {
                let anchor = match anchor {
                    TextAnchor::Start => "start",
                    TextAnchor::Middle => "middle",
                    TextAnchor::End => "end",
                };
                let _ = write!(out, "<text x=\"{x}\" y=\"{y}\" font-family=\"monospace\" font-size=\"{size}\" text-anchor=\"{anchor}\" fill=\"{}\">{}</text>", color(*ink), escape(text));
            }
            Primitive::Polygon {
                points,
                fill,
                stroke,
            } => {
                let points = points
                    .iter()
                    .map(|point| format!("{},{}", point[0], point[1]))
                    .collect::<Vec<_>>()
                    .join(" ");
                let _ = write!(
                    out,
                    "<polygon points=\"{points}\"{}/>",
                    attributes(*fill, *stroke)
                );
            }
        }
    }
    out.push_str("</svg>");
    out
}
