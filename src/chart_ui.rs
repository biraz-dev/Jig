//! Inline chart gallery. Rendering and data interpretation share the core scene.
use crate::web_theme::{ACCENT, BG, ERROR, MUTED, RULE, TEXT};
use chrono::{Local, NaiveDate};
use eframe::egui::{self, Color32, FontId, RichText, Stroke, TextEdit, Ui};
use jig::{
    charts::{self, ChartScene, ChartSource, Primitive, TextAnchor},
    model::State,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub enum ChartAction {
    Rename { id: u64, title: String },
    Remove(u64),
    BackToGrid,
}

#[derive(Default)]
pub struct ChartPanel {
    title_edit: Option<(u64, String)>,
    focus_title: bool,
    error: Option<String>,
    scenes: BTreeMap<u64, (f64, Result<ChartScene, String>)>,
    scenes_day: Option<NaiveDate>,
}

impl ChartPanel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn invalidate(&mut self) {
        self.scenes.clear();
    }

    pub fn reject_action(&mut self, action: &ChartAction, error: String) {
        self.error = Some(error);
        if let ChartAction::Rename { id, title } = action {
            self.title_edit = Some((*id, title.clone()));
            self.focus_title = true;
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &State) -> Vec<ChartAction> {
        let today = Local::now().date_naive();
        if self.scenes_day != Some(today) {
            self.invalidate();
            self.scenes_day = Some(today);
        }
        let mut actions = Vec::new();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Back to cells").clicked() {
                actions.push(ChartAction::BackToGrid);
            }
            ui.label(RichText::new("Charts").color(ACCENT));
        });
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).color(ERROR).size(12.0));
        }
        let Some(book) = &state.workbook else {
            ui.label(
                RichText::new("Select cells and use Insert → Chart to create a chart.")
                    .color(MUTED),
            );
            return actions;
        };
        if book.charts.is_empty() {
            ui.label(
                RichText::new("Select cells and use Insert → Chart to create a chart.")
                    .color(MUTED),
            );
            return actions;
        }
        egui::ScrollArea::vertical()
            .id_salt("chart-gallery")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for chart in &book.charts {
                    ui.push_id(("chart-card", chart.id), |ui| {
                        egui::Frame::default()
                            .fill(BG)
                            .stroke(Stroke::new(0.6_f32, RULE))
                            .inner_margin(egui::Margin::same(9))
                            .show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        RichText::new(chart.kind.label()).color(ACCENT).size(12.0),
                                    );
                                    let source = match chart.source {
                                        ChartSource::Tasks => "Live tasks".into(),
                                        ChartSource::Range { sheet, range } => {
                                            let name = book
                                                .sheet(sheet)
                                                .map_or("Missing sheet", |sheet| {
                                                    sheet.name.as_str()
                                                });
                                            format!(
                                                "{name} · {}:{}",
                                                range.start.label(),
                                                range.end.label()
                                            )
                                        }
                                    };
                                    ui.label(RichText::new(source).color(MUTED).size(11.0));
                                    if ui.small_button("Rename").clicked() {
                                        self.title_edit = Some((chart.id, chart.title.clone()));
                                        self.focus_title = true;
                                        self.error = None;
                                    }
                                    if ui
                                        .small_button("del")
                                        .on_hover_text("Remove this chart; Undo restores it")
                                        .clicked()
                                    {
                                        actions.push(ChartAction::Remove(chart.id));
                                    }
                                });
                                if self
                                    .title_edit
                                    .as_ref()
                                    .is_some_and(|(id, _)| *id == chart.id)
                                {
                                    let mut commit = false;
                                    let mut cancel = false;
                                    ui.horizontal_wrapped(|ui| {
                                        let field = egui::Id::new(("chart-title", chart.id));
                                        if self.focus_title {
                                            ui.input_mut(|input| {
                                                input.consume_key(
                                                    egui::Modifiers::NONE,
                                                    egui::Key::Enter,
                                                );
                                                input.consume_key(
                                                    egui::Modifiers::NONE,
                                                    egui::Key::Space,
                                                );
                                            });
                                            ui.memory_mut(|memory| memory.request_focus(field));
                                            self.focus_title = false;
                                        }
                                        let (_, title) = self.title_edit.as_mut().unwrap();
                                        let response = ui.add(
                                            TextEdit::singleline(title)
                                                .id(field)
                                                .return_key(None)
                                                .char_limit(120)
                                                .desired_width(280.0),
                                        );
                                        commit = ui.button("Save title").clicked()
                                            || (response.has_focus()
                                                && ui.input_mut(|input| {
                                                    input.consume_key(
                                                        egui::Modifiers::NONE,
                                                        egui::Key::Enter,
                                                    )
                                                }));
                                        cancel = ui.small_button("Cancel").clicked()
                                            || ((response.has_focus() || response.lost_focus())
                                                && ui.input_mut(|input| {
                                                    input.consume_key(
                                                        egui::Modifiers::NONE,
                                                        egui::Key::Escape,
                                                    )
                                                }));
                                    });
                                    if commit {
                                        if let Some((id, title)) = self.title_edit.take() {
                                            actions.push(ChartAction::Rename {
                                                id,
                                                title: title.trim().to_owned(),
                                            });
                                        }
                                    } else if cancel {
                                        self.title_edit = None;
                                    }
                                }
                                let width = f64::from(ui.available_width().clamp(280.0, 2000.0));
                                let cached = self.scenes.entry(chart.id).or_insert_with(|| {
                                    (width, charts::scene(state, chart, width, 280.0))
                                });
                                if cached.0 != width {
                                    *cached = (width, charts::scene(state, chart, width, 280.0));
                                }
                                match &cached.1 {
                                    Ok(scene) => paint_scene(ui, scene),
                                    Err(error) => {
                                        ui.label(RichText::new(&chart.title).color(TEXT));
                                        ui.label(RichText::new(error).color(ERROR).size(12.0));
                                    }
                                }
                            });
                    });
                    ui.add_space(10.0);
                }
            });
        actions
    }
}

pub fn paint_scene(ui: &mut Ui, scene: &ChartScene) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(scene.width as f32, scene.height as f32),
        egui::Sense::focusable_noninteractive(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), &scene.title)
    });
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let point = |x: f64, y: f64| rect.min + egui::vec2(x as f32, y as f32);
    let ink = |color: [u8; 3]| Color32::from_rgb(color[0], color[1], color[2]);
    for primitive in &scene.primitives {
        match primitive {
            Primitive::Rect {
                x,
                y,
                width,
                height,
                fill,
                stroke,
            } => {
                let bounds = egui::Rect::from_min_size(
                    point(*x, *y),
                    egui::vec2(*width as f32, *height as f32),
                );
                if let Some(fill) = fill {
                    painter.rect_filled(bounds, 0.0, ink(*fill));
                }
                if let Some(stroke) = stroke {
                    let stroke = Stroke::new(stroke.width as f32, ink(stroke.color));
                    painter.line_segment([bounds.left_top(), bounds.right_top()], stroke);
                    painter.line_segment([bounds.right_top(), bounds.right_bottom()], stroke);
                    painter.line_segment([bounds.right_bottom(), bounds.left_bottom()], stroke);
                    painter.line_segment([bounds.left_bottom(), bounds.left_top()], stroke);
                }
            }
            Primitive::Line {
                x1,
                y1,
                x2,
                y2,
                stroke,
            } => {
                painter.line_segment(
                    [point(*x1, *y1), point(*x2, *y2)],
                    Stroke::new(stroke.width as f32, ink(stroke.color)),
                );
            }
            Primitive::Text {
                x,
                y,
                text,
                size,
                color,
                anchor,
            } => {
                let align = match anchor {
                    TextAnchor::Start => egui::Align2::LEFT_BOTTOM,
                    TextAnchor::Middle => egui::Align2::CENTER_BOTTOM,
                    TextAnchor::End => egui::Align2::RIGHT_BOTTOM,
                };
                painter.text(
                    point(*x, *y + *size * 0.18),
                    align,
                    text,
                    FontId::monospace(*size as f32),
                    ink(*color),
                );
            }
            Primitive::Polygon {
                points,
                fill,
                stroke,
            } => {
                let points = points.iter().map(|p| point(p[0], p[1])).collect::<Vec<_>>();
                if let Some(fill) = fill {
                    let mut mesh = egui::epaint::Mesh::default();
                    for p in &points {
                        mesh.colored_vertex(*p, ink(*fill));
                    }
                    for index in 1..points.len().saturating_sub(1) {
                        mesh.add_triangle(0, index as u32, index as u32 + 1);
                    }
                    painter.add(egui::Shape::mesh(mesh));
                }
                if let Some(stroke) = stroke {
                    painter.add(egui::Shape::closed_line(
                        points,
                        Stroke::new(stroke.width as f32, ink(stroke.color)),
                    ));
                }
            }
        }
    }
}
