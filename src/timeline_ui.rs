//! A compact, day-based projection of the ordinary checklist and its dates.
use crate::web::task_color;
use chrono::{Datelike, Local, NaiveDate};
use eframe::egui::{self, Color32, FontId, RichText, Stroke, Ui};
use jig::{
    model::State,
    timeline::{self, DateRange, Scale, TimelineRow},
};

const BG: Color32 = Color32::from_rgb(12, 19, 27);
const CREAM: Color32 = Color32::from_rgb(246, 218, 174);
const MUTED: Color32 = Color32::from_rgb(144, 161, 202);
const ACCENT: Color32 = Color32::from_rgb(109, 185, 238);
const RULE: Color32 = Color32::from_rgb(49, 65, 83);
const WEEKEND: Color32 = Color32::from_rgb(16, 25, 35);
const ROW_HEIGHT: f32 = 36.0;

#[derive(Clone, Copy)]
pub enum TimelineAction {
    EditTask(u64),
    EditDeadline(u64),
}

pub struct TimelinePanel {
    anchor: NaiveDate,
    scale: Scale,
    selected: Option<(Option<u64>, Option<u64>)>,
    focus_requested: bool,
    pub focused: bool,
}

impl TimelinePanel {
    pub fn new(today: NaiveDate) -> Self {
        Self {
            anchor: today,
            scale: Scale::Week,
            selected: None,
            focus_requested: false,
            focused: false,
        }
    }

    pub fn reveal(&mut self, date: NaiveDate) {
        self.anchor = date;
    }

    pub fn request_focus(&mut self) {
        self.focus_requested = true;
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &State, today: NaiveDate) -> Option<TimelineAction> {
        self.focused = false;
        ui.horizontal_wrapped(|ui| {
            if self.focus_requested {
                let id = ui.next_auto_id();
                ui.memory_mut(|memory| memory.request_focus(id));
                self.focus_requested = false;
                ui.ctx().request_repaint();
            }
            for (label, scale) in [("Week", Scale::Week), ("Month", Scale::Month)] {
                let response = ui.selectable_label(self.scale == scale, label);
                self.focused |= response.has_focus();
                if response.clicked() {
                    self.scale = scale;
                }
            }
            for (label, delta) in [("‹", -1), ("›", 1)] {
                let response = ui.button(label).on_hover_text(if delta < 0 {
                    "Previous period"
                } else {
                    "Next period"
                });
                self.focused |= response.has_focus();
                if response.clicked() {
                    if let Ok(anchor) = timeline::shift(self.anchor, self.scale, delta) {
                        self.anchor = anchor;
                    }
                }
            }
            let response = ui.small_button("Today");
            self.focused |= response.has_focus();
            if response.clicked() {
                self.anchor = today;
            }
        });
        let Ok(window) = timeline::window(self.anchor, self.scale) else {
            ui.label(RichText::new("Date outside supported range").color(MUTED));
            return None;
        };
        let mut rows = timeline::rows(state, &Local);
        // Keep checklist order within each group, without reordering records.
        // Dated work remains immediately visible even with many undated notes.
        rows.sort_by_key(|row| row.due.is_none());
        let width = (ui.available_width() - 16.0).max(1.0);
        let label_width = (width * 0.34).clamp(88.0, 190.0).min(width * 0.52);
        let (header, _) = ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::hover());
        self.header(ui, header, label_width, window, today);
        if rows.is_empty() {
            ui.label(RichText::new("No tasks yet").color(MUTED).size(12.0));
            return None;
        }
        let mut action = None;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical()
                .id_salt("timeline-rows")
                .max_height(if width >= 760.0 { 420.0 } else { 240.0 })
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for row in &rows {
                        let key = (row.task_id, row.deadline_id);
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(width, ROW_HEIGHT),
                            egui::Sense::hover(),
                        );
                        let response = ui.interact(
                            rect,
                            ui.id().with(("timeline-row", key)),
                            egui::Sense::click(),
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                ui.is_enabled(),
                                format!("{} · {} · {}", row.title, status(row), dates(row)),
                            )
                        });
                        self.focused |= response.has_focus();
                        if response.gained_focus() {
                            response.scroll_to_me(None);
                        }
                        let selected = self.selected == Some(key);
                        self.row(ui, rect, label_width, window, today, row, selected);
                        if response.hovered() || response.has_focus() {
                            ui.painter().hline(
                                rect.x_range(),
                                rect.bottom() - 1.0,
                                Stroke::new(0.8_f32, ACCENT),
                            );
                        }
                        if response.clicked() {
                            self.selected = Some(key);
                            ui.ctx().request_repaint();
                            action = row
                                .task_id
                                .map(TimelineAction::EditTask)
                                .or_else(|| row.deadline_id.map(TimelineAction::EditDeadline));
                        }
                        response.on_hover_text(format!(
                            "{}\n{} · {}",
                            row.title,
                            status(row),
                            dates(row)
                        ));
                    }
                });
        });
        if let Some(selected) = rows
            .iter()
            .find(|row| self.selected == Some((row.task_id, row.deadline_id)))
        {
            ui.add_space(4.0);
            ui.label(
                RichText::new(selected.title)
                    .color(if selected.complete { MUTED } else { CREAM })
                    .size(12.0),
            );
            ui.label(
                RichText::new(format!("{} · {}", status(selected), dates(selected)))
                    .color(MUTED)
                    .size(11.0),
            );
        } else {
            self.selected = None;
        }
        action
    }

    fn header(
        &self,
        ui: &Ui,
        rect: egui::Rect,
        label_width: f32,
        window: DateRange,
        today: NaiveDate,
    ) {
        let label = if self.scale == Scale::Month {
            window.start.format("%b %Y").to_string()
        } else {
            format!(
                "{}\n{}–{}",
                window.start.format("%b %Y"),
                window.start.format("%d"),
                window.end.format("%d %b")
            )
        };
        ui.painter().with_clip_rect(rect).text(
            egui::pos2(rect.left() + 3.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::monospace(9.0),
            MUTED,
        );
        let lane = egui::Rect::from_min_max(
            egui::pos2(rect.left() + label_width, rect.top()),
            rect.max,
        );
        self.grid(ui, lane, window, today);
        let cell = lane.width() / window.days() as f32;
        let stride = if cell >= 15.0 {
            1
        } else if cell >= 8.0 {
            3
        } else {
            7
        };
        for offset in 0..window.days() {
            let Some(day) = window.start.checked_add_days(chrono::Days::new(offset.into())) else {
                continue;
            };
            let x = lane.left() + (offset as f32 + 0.5) * cell;
            let color = if day == today { ACCENT } else { MUTED };
            if cell >= 25.0 {
                ui.painter().text(
                    egui::pos2(x, rect.top() + 7.0),
                    egui::Align2::CENTER_CENTER,
                    day.format("%a").to_string(),
                    FontId::monospace(8.0),
                    color,
                );
            }
            if offset % stride == 0 {
                ui.painter().text(
                    egui::pos2(x, rect.bottom() - 7.0),
                    egui::Align2::CENTER_CENTER,
                    day.day().to_string(),
                    FontId::monospace(10.0),
                    color,
                );
            }
        }
    }

    fn grid(&self, ui: &Ui, rect: egui::Rect, window: DateRange, today: NaiveDate) {
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, BG);
        let cell = rect.width() / window.days() as f32;
        for offset in 0..window.days() {
            let Some(day) = window.start.checked_add_days(chrono::Days::new(offset.into())) else {
                continue;
            };
            let x = rect.left() + offset as f32 * cell;
            if day.weekday().number_from_monday() >= 6 {
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(x, rect.top()),
                        egui::vec2(cell, rect.height()),
                    ),
                    0.0,
                    WEEKEND,
                );
            }
            if self.scale == Scale::Week || day.weekday().number_from_monday() == 1 {
                painter.vline(x, rect.y_range(), Stroke::new(0.5_f32, RULE));
            }
            if day == today {
                painter.vline(
                    x + cell * 0.5,
                    rect.y_range(),
                    Stroke::new(1.0_f32, ACCENT.gamma_multiply(0.65)),
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn row(
        &self,
        ui: &Ui,
        rect: egui::Rect,
        label_width: f32,
        window: DateRange,
        today: NaiveDate,
        row: &TimelineRow<'_>,
        selected: bool,
    ) {
        let lane = egui::Rect::from_min_max(
            egui::pos2(rect.left() + label_width, rect.top()),
            rect.max,
        );
        self.grid(ui, lane, window, today);
        let painter = ui.painter();
        let color = task_color(row.color, row.complete);
        let title_y = rect.top() + 10.0;
        if row.complete {
            painter.circle_stroke(
                egui::pos2(rect.left() + 5.0, title_y),
                2.5,
                Stroke::new(0.8_f32, color),
            );
        } else {
            painter.circle_filled(egui::pos2(rect.left() + 5.0, title_y), 2.5, color);
        }
        let title_x = rect.left() + 13.0;
        let columns = ((label_width - 18.0) / 7.0).floor().max(1.0) as usize;
        let title = shortened(row.title, columns);
        let label_clip =
            egui::Rect::from_min_max(rect.min, egui::pos2(lane.left() - 3.0, rect.bottom()));
        painter.with_clip_rect(label_clip).text(
            egui::pos2(title_x, title_y),
            egui::Align2::LEFT_CENTER,
            &title,
            FontId::monospace(11.0),
            if row.complete { MUTED } else { CREAM },
        );
        if row.complete {
            painter.hline(
                title_x..=(title_x + title.chars().count() as f32 * 6.6).min(lane.left() - 4.0),
                title_y,
                Stroke::new(0.7_f32, MUTED),
            );
        }
        painter.with_clip_rect(label_clip).text(
            egui::pos2(title_x, rect.top() + 26.0),
            egui::Align2::LEFT_CENTER,
            row.due.map_or_else(
                || format!("{} · No date", status(row)),
                |date| format!("{} · {}", status(row), date.format("%d %b")),
            ),
            FontId::monospace(9.0),
            MUTED,
        );
        if let Some(span) = timeline::clipped(row, window) {
            let cell = lane.width() / window.days() as f32;
            let center_y = lane.center().y;
            let left = lane.left() + span.offset as f32 * cell;
            if span.milestone {
                let center = egui::pos2(left + cell * 0.5, center_y);
                let radius = (cell * 0.42).clamp(2.0, 5.0);
                let points = vec![
                    center + egui::vec2(-radius, 0.0),
                    center + egui::vec2(0.0, -5.0),
                    center + egui::vec2(radius, 0.0),
                    center + egui::vec2(0.0, 5.0),
                ];
                painter.add(egui::Shape::convex_polygon(
                    points,
                    if row.complete { BG } else { color },
                    Stroke::new(0.8_f32, color),
                ));
            } else {
                let inset = (cell * 0.15).min(0.5);
                let bar = egui::Rect::from_min_max(
                    egui::pos2(left + inset, center_y - 5.0),
                    egui::pos2(left + span.days as f32 * cell - inset, center_y + 5.0),
                );
                painter.rect_filled(
                    bar,
                    1.5,
                    if row.complete {
                        color.gamma_multiply(0.2)
                    } else {
                        color
                    },
                );
                if row.complete {
                    painter.hline(bar.x_range(), bar.top(), Stroke::new(0.8_f32, color));
                    painter.hline(bar.x_range(), bar.bottom(), Stroke::new(0.8_f32, color));
                }
                for (clipped, x, direction) in [
                    (span.clipped_start, bar.left() + 3.0, -1.0),
                    (span.clipped_end, bar.right() - 3.0, 1.0),
                ] {
                    if clipped {
                        painter.line_segment(
                            [
                                egui::pos2(x - direction * 2.0, center_y - 2.5),
                                egui::pos2(x + direction * 2.0, center_y),
                            ],
                            Stroke::new(1.0_f32, BG),
                        );
                        painter.line_segment(
                            [
                                egui::pos2(x + direction * 2.0, center_y),
                                egui::pos2(x - direction * 2.0, center_y + 2.5),
                            ],
                            Stroke::new(1.0_f32, BG),
                        );
                    }
                }
            }
        } else if let Some(due) = row.due {
            painter.with_clip_rect(lane).text(
                egui::pos2(lane.center().x, lane.center().y),
                egui::Align2::CENTER_CENTER,
                format!(
                    "{} {}",
                    if due < window.start { "‹" } else { "›" },
                    due.format("%d %b")
                ),
                FontId::monospace(10.0),
                MUTED,
            );
        }
        if today >= window.start && today <= window.end {
            let offset = (today - window.start).num_days() as f32;
            let x = lane.left() + (offset + 0.5) * lane.width() / window.days() as f32;
            painter.vline(
                x,
                lane.y_range(),
                Stroke::new(1.0_f32, ACCENT.gamma_multiply(0.65)),
            );
        }
        painter.hline(rect.x_range(), rect.bottom(), Stroke::new(0.4_f32, RULE));
        if selected {
            painter.vline(rect.left(), rect.y_range(), Stroke::new(1.0_f32, ACCENT));
        }
    }
}

fn status(row: &TimelineRow<'_>) -> &'static str {
    if row.complete {
        "Done"
    } else {
        "Open"
    }
}

fn dates(row: &TimelineRow<'_>) -> String {
    match (row.start, row.due) {
        (Some(start), Some(due)) => format!(
            "{} → {}",
            start.format("%d %b %Y"),
            due.format("%d %b %Y")
        ),
        (_, Some(due)) => format!("Due {}", due.format("%d %b %Y")),
        _ => "No date".into(),
    }
}

fn shortened(text: &str, columns: usize) -> String {
    let mut chars = text.chars();
    let mut result: String = chars.by_ref().take(columns).collect();
    if chars.next().is_some() {
        result.pop();
        result.push('…');
    }
    result
}
