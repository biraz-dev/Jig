//! Compact calendar projections. Browsing never changes task records.
use crate::web::task_color;
use crate::web_theme;
use chrono::{Datelike, Duration, Local, NaiveDate};
use eframe::egui::{self, Align2, FontId, RichText, Stroke, Ui};
use jig::{calendar_strip::Viewport, model::State, timeline};
use std::collections::BTreeMap;

const BAND_HEIGHT: f32 = 90.0;
const MONTH_HEIGHT: f32 = 22.0;
const MONTH_CELL_HEIGHT: f32 = 38.0;
const MONTH_MIN_WIDTH: f32 = 7.0 * 30.0;
const MONTH_MAX_WIDTH: f32 = 480.0;

pub struct CalendarPanel {
    view: Viewport,
    month_view: bool,
    month: NaiveDate,
    last_width: f32,
    focus_requested: bool,
    keyboard_ids: Vec<egui::Id>,
    pub focused: bool,
}

impl CalendarPanel {
    pub fn new(today: NaiveDate) -> Self {
        Self {
            view: Viewport::new(today),
            month_view: false,
            month: today.with_day(1).unwrap(),
            last_width: 320.0,
            focus_requested: false,
            keyboard_ids: Vec::new(),
            focused: false,
        }
    }

    pub fn reveal(&mut self, date: NaiveDate) {
        self.view.reveal(date, self.last_width);
        self.month = date.with_day(1).unwrap();
    }

    pub fn month_view(&self) -> bool {
        self.month_view
    }

    pub fn set_month_view(&mut self, enabled: bool, selected: NaiveDate) {
        self.month_view = enabled;
        self.reveal(selected);
    }

    pub fn request_focus(&mut self) {
        self.focus_requested = true;
    }

    pub fn owns_keyboard(&self, ctx: &egui::Context) -> bool {
        ctx.memory(|memory| {
            memory
                .focused()
                .is_some_and(|id| self.keyboard_ids.contains(&id))
        })
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        state: &State,
        today: NaiveDate,
        selected_date: NaiveDate,
        selected_lists: &[u64],
    ) -> Option<NaiveDate> {
        let colors = web_theme::colors(ui.ctx());
        self.focused = false;
        self.keyboard_ids.clear();
        if self.month_view {
            return self.month_ui(ui, state, today, selected_date, selected_lists);
        }
        let width = ui.available_width().max(1.0);
        let selected_left = self.view.x_for(selected_date);
        if width < self.last_width
            && selected_left < self.last_width
            && selected_left + self.view.day_width() > 0.0
        {
            // Keep a visible selection when the panel narrows, without
            // pulling it back after the user deliberately panned elsewhere.
            self.view.ensure_visible(selected_date, width);
        }
        self.last_width = width;
        let mut selected = None;
        let selected_x = self.view.x_for(selected_date) + self.view.day_width() * 0.5;
        let zoom_anchor = if (0.0..=width).contains(&selected_x) {
            selected_x
        } else {
            width * 0.5
        };
        ui.horizontal_wrapped(|ui| {
            let today_button = ui.small_button("Today");
            self.track(&today_button);
            if today_button.clicked() {
                self.view.reveal(today, width);
                selected = Some(today);
            }
            for (label, direction) in [("‹", 1.0), ("›", -1.0)] {
                let response = ui.small_button(label).on_hover_text(if direction > 0.0 {
                    "Earlier dates · Drag the date strip to pan"
                } else {
                    "Later dates · Drag the date strip to pan"
                });
                self.track(&response);
                if response.clicked() {
                    self.view.pan(width * 0.8 * direction, width);
                }
            }
            for (label, factor) in [("−", 0.8), ("+", 1.25)] {
                let response = ui.small_button(label).on_hover_text(if factor < 1.0 {
                    "Show more days · Ctrl-wheel or pinch to zoom"
                } else {
                    "Larger days · Ctrl-wheel or pinch to zoom"
                });
                self.track(&response);
                if response.clicked() {
                    self.view.zoom(factor, zoom_anchor, width);
                }
            }
            let dates = self.view.visible(width);
            if let (Some((first, _)), Some((last, _))) = (dates.first(), dates.last()) {
                let label = if first.year() == last.year() && first.month() == last.month() {
                    first.format("%b %Y").to_string()
                } else {
                    format!("{} – {}", first.format("%b %Y"), last.format("%b %Y"))
                };
                ui.label(RichText::new(label).small().color(colors.muted));
            }
        });
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(width, BAND_HEIGHT), egui::Sense::hover());
        let id = ui.id().with("calendar-date-band");
        if self.focus_requested {
            ui.memory_mut(|memory| memory.request_focus(id));
            self.focus_requested = false;
            ui.ctx().request_repaint();
        }
        let response = ui.interact(rect, id, egui::Sense::click_and_drag());
        self.track(&response);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                format!(
                    "Dates · {} · Left/Right select a day · Page Up/Down pan · Plus/Minus zoom · Home today",
                    selected_date.format("%A %d %B %Y")
                ),
            )
        });
        if response.gained_focus() {
            response.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
        }
        if ui.input(|input| {
            !input.focused
                || input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Touch {
                            phase: egui::TouchPhase::Cancel,
                            ..
                        }
                    )
                })
        }) && ui.ctx().dragged_id() == Some(id)
        {
            ui.ctx().stop_dragging();
        }
        let touch = ui.input(|input| input.multi_touch());
        if let Some(touch) = touch.filter(|touch| rect.contains(touch.start_pos)) {
            self.view.pan(touch.translation_delta.x, width);
            self.view
                .zoom(touch.zoom_delta, touch.center_pos.x - rect.left(), width);
        } else if response.dragged() {
            self.view.pan(response.drag_delta().x, width);
        }
        if ui.rect_contains_pointer(rect) && touch.is_none() {
            let (pan, zoom, anchor) = ui.input_mut(|input| {
                let zoom = input.zoom_delta();
                let pan = if zoom != 1.0 || input.modifiers.ctrl || input.modifiers.command {
                    0.0
                } else if input.smooth_scroll_delta.x != 0.0 {
                    input.smooth_scroll_delta.x
                } else if input.modifiers.shift {
                    input.smooth_scroll_delta.y
                } else {
                    0.0
                };
                if pan != 0.0 {
                    input.raw_scroll_delta.x = 0.0;
                    input.smooth_scroll_delta.x = 0.0;
                    if input.modifiers.shift {
                        input.raw_scroll_delta.y = 0.0;
                        input.smooth_scroll_delta.y = 0.0;
                    }
                }
                (
                    pan,
                    zoom,
                    input
                        .pointer
                        .hover_pos()
                        .map_or(width * 0.5, |p| p.x - rect.left()),
                )
            });
            self.view.pan(pan, width);
            self.view.zoom(zoom, anchor, width);
        }
        if response.clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                selected = self.view.date_at(pointer.x - rect.left(), width);
            }
        }
        if response.has_focus() {
            let page = (width / self.view.day_width()).floor().max(1.0) * self.view.day_width();
            let (days, pan, home, zoom) = ui.input_mut(|input| {
                let none = egui::Modifiers::NONE;
                let days = if input.consume_key(none, egui::Key::ArrowLeft) {
                    -1
                } else if input.consume_key(none, egui::Key::ArrowRight) {
                    1
                } else {
                    0
                };
                let pan = if input.consume_key(none, egui::Key::PageUp) {
                    page
                } else if input.consume_key(none, egui::Key::PageDown) {
                    -page
                } else {
                    0.0
                };
                let zoom = if input.consume_key(none, egui::Key::Plus)
                    || input.consume_key(none, egui::Key::Equals)
                    || input.consume_key(egui::Modifiers::SHIFT, egui::Key::Plus)
                    || input.consume_key(egui::Modifiers::SHIFT, egui::Key::Equals)
                {
                    1.25
                } else if input.consume_key(none, egui::Key::Minus) {
                    0.8
                } else {
                    1.0
                };
                (days, pan, input.consume_key(none, egui::Key::Home), zoom)
            });
            self.view.pan(pan, width);
            self.view.zoom(zoom, zoom_anchor, width);
            if home {
                self.view.reveal(today, width);
                selected = Some(today);
            } else if days != 0 {
                if let Some(date) = selected_date
                    .checked_add_signed(Duration::days(days))
                    .filter(|date| (1..=9999).contains(&date.year()))
                {
                    self.view.ensure_visible(date, width);
                    selected = Some(date);
                }
            }
        }
        self.paint(
            ui,
            rect,
            state,
            today,
            selected.unwrap_or(selected_date),
            selected_lists,
        );
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect,
                0,
                Stroke::new(1.0_f32, colors.accent),
                egui::StrokeKind::Inside,
            );
        }
        response.on_hover_text("Drag to pan · Shift-wheel pans · Ctrl-wheel or pinch zooms · Left/Right selects a date");
        selected
    }

    fn shifted_month(&self, delta: i32) -> Option<NaiveDate> {
        let (year, month) =
            jig::calendar::shift(self.month.year(), self.month.month(), delta).ok()?;
        (1..=9999)
            .contains(&year)
            .then(|| NaiveDate::from_ymd_opt(year, month, 1))
            .flatten()
    }

    fn month_ui(
        &mut self,
        ui: &mut Ui,
        state: &State,
        today: NaiveDate,
        selected_date: NaiveDate,
        selected_lists: &[u64],
    ) -> Option<NaiveDate> {
        let colors = web_theme::colors(ui.ctx());
        let mut selected = None;
        ui.horizontal_wrapped(|ui| {
            let response = ui.small_button("Today");
            self.track(&response);
            if response.clicked() {
                self.reveal(today);
                selected = Some(today);
            }
            for (label, delta) in [("‹", -1), ("›", 1)] {
                let next = self.shifted_month(delta);
                let response = ui
                    .add_enabled(next.is_some(), egui::Button::new(label).small())
                    .on_hover_text(if delta < 0 {
                        "Previous month"
                    } else {
                        "Next month"
                    });
                self.track(&response);
                if response.clicked() {
                    self.month = next.unwrap();
                }
            }
            ui.label(
                RichText::new(self.month.format("%B %Y").to_string())
                    .small()
                    .color(colors.muted),
            );
        });
        let width = ui.available_width().clamp(MONTH_MIN_WIDTH, MONTH_MAX_WIDTH);
        let id = ui.id().with("calendar-date-band");
        let mut focus_rect = None;
        egui::ScrollArea::horizontal()
            .id_salt("calendar-month-grid-scroll")
            .max_width(MONTH_MAX_WIDTH)
            .auto_shrink([true, true])
            .show(ui, |ui| {
                let cells = jig::calendar::month(self.month.year(), self.month.month())
                    .unwrap_or_default();
                let anchor = self.month.with_day(selected_date.day()).unwrap_or_else(|| {
                    cells.iter().flatten().next_back().copied().unwrap_or(self.month)
                });
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(width, MONTH_HEIGHT + 6.0 * MONTH_CELL_HEIGHT),
                    egui::Sense::hover(),
                );
                let grid = egui::Rect::from_min_max(
                    rect.left_top() + egui::vec2(0.0, MONTH_HEIGHT),
                    rect.right_bottom(),
                );
                let response = ui.interact(grid, id, egui::Sense::click());
                let pointer_click = if response.clicked_by(egui::PointerButton::Primary)
                    && ui.input(|input| input.pointer.any_released())
                {
                    response.interact_pointer_pos().filter(|p| {
                        grid.contains(*p) && p.x < grid.right() && p.y < grid.bottom()
                    })
                } else {
                    None
                };
                // TouchEnd removes the pointer before egui handles focus.
                // A blank cell still belongs to the grid's keyboard field.
                let requested_focus = self.focus_requested
                    || (pointer_click.is_some() && !response.has_focus());
                if requested_focus {
                    response.request_focus();
                    self.focus_requested = false;
                    ui.ctx().request_repaint();
                }
                self.track(&response);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        ui.is_enabled(),
                        format!(
                            "Calendar · {} · Arrows select dates · Page Up/Down browse months · Home today",
                            selected_date.format("%A %d %B %Y")
                        ),
                    )
                });
                if response.gained_focus() || requested_focus {
                    if response.gained_focus() {
                        // The filter can only lock a widget focused in the
                        // preceding pass. Prime it before the next key arrives.
                        ui.ctx().request_discard("Prime month calendar arrow-key focus");
                    }
                    response.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
                    focus_rect = Some(grid);
                    ui.ctx().request_repaint();
                }
                if let Some(pointer) = pointer_click {
                    let column = ((pointer.x - grid.left()) / (width / 7.0)).floor() as usize;
                    let row = ((pointer.y - grid.top()) / MONTH_CELL_HEIGHT).floor() as usize;
                    selected = cells.get(row * 7 + column).copied().flatten();
                }
                if response.has_focus() {
                    ui.memory_mut(|memory| {
                        memory.set_focus_lock_filter(id, egui::EventFilter {
                            horizontal_arrows: true,
                            vertical_arrows: true,
                            ..Default::default()
                        });
                    });
                    let (days, page, home, enter) = ui.input_mut(|input| {
                        let none = egui::Modifiers::NONE;
                        let days = if input.consume_key(none, egui::Key::ArrowLeft) { -1 }
                            else if input.consume_key(none, egui::Key::ArrowRight) { 1 }
                            else if input.consume_key(none, egui::Key::ArrowUp) { -7 }
                            else if input.consume_key(none, egui::Key::ArrowDown) { 7 }
                            else { 0 };
                        let page = if input.consume_key(none, egui::Key::PageUp) { -1 }
                            else if input.consume_key(none, egui::Key::PageDown) { 1 }
                            else { 0 };
                        (days, page, input.consume_key(none, egui::Key::Home),
                            input.consume_key(none, egui::Key::Enter) || input.consume_key(none, egui::Key::Space))
                    });
                    if page != 0 {
                        if let Some(month) = self.shifted_month(page) {
                            self.month = month;
                        }
                    } else if home {
                        self.reveal(today);
                        selected = Some(today);
                    } else if days != 0 {
                        if let Some(date) = anchor.checked_add_signed(Duration::days(days))
                            .filter(|date| (1..=9999).contains(&date.year()))
                        {
                            self.month = date.with_day(1).unwrap();
                            selected = Some(date);
                        }
                    } else if enter {
                        selected = Some(anchor);
                    }
                    if days != 0 || page != 0 || home || enter {
                        ui.ctx().request_repaint();
                    }
                }
                self.paint_month(ui, rect, state, today, selected.unwrap_or(selected_date), selected_lists);
                if response.has_focus() {
                    ui.painter().rect_stroke(grid, 0, Stroke::new(1.0_f32, colors.accent), egui::StrokeKind::Inside);
                }
                response.on_hover_text("Click a day · Arrows select dates · Page Up/Down browse months · Home today");
            });
        if let Some(rect) = focus_rect {
            // The horizontal area consumes both scroll targets. Repeat this
            // request on its parent so the dashboard can reveal the grid too.
            ui.scroll_to_rect_animation(rect, None, egui::style::ScrollAnimation::none());
        }
        selected
    }

    fn paint_month(
        &self,
        ui: &Ui,
        rect: egui::Rect,
        state: &State,
        today: NaiveDate,
        selected: NaiveDate,
        lists: &[u64],
    ) {
        let colors = web_theme::colors(ui.ctx());
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        painter.rect_filled(rect, 0, colors.bg);
        let cell_width = rect.width() / 7.0;
        for (column, name) in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
            .into_iter()
            .enumerate()
        {
            painter.text(
                egui::pos2(
                    rect.left() + (column as f32 + 0.5) * cell_width,
                    rect.top() + 11.0,
                ),
                Align2::CENTER_CENTER,
                name,
                FontId::monospace(11.0),
                colors.muted,
            );
        }
        let mut deadlines = BTreeMap::<_, Vec<_>>::new();
        for row in timeline::rows_for_lists(state, &Local, lists) {
            if let Some(due) = row
                .due
                .filter(|due| due.year() == self.month.year() && due.month() == self.month.month())
            {
                deadlines.entry(due).or_default().push(row);
            }
        }
        let cells = jig::calendar::month(self.month.year(), self.month.month()).unwrap_or_default();
        let hover = ui.input(|input| input.pointer.hover_pos());
        for (index, date) in cells.into_iter().enumerate() {
            let column = index % 7;
            let row = index / 7;
            let cell = egui::Rect::from_min_size(
                egui::pos2(
                    rect.left() + column as f32 * cell_width,
                    rect.top() + MONTH_HEIGHT + row as f32 * MONTH_CELL_HEIGHT,
                ),
                egui::vec2(cell_width, MONTH_CELL_HEIGHT),
            );
            let fill = if date == Some(selected) {
                colors.selected
            } else if date.is_some() && hover.is_some_and(|point| cell.contains(point)) {
                colors.hover
            } else if column >= 5 {
                colors.weekend
            } else {
                colors.bg
            };
            painter.rect_filled(cell.shrink(1.0), 0, fill);
            painter.rect_stroke(
                cell,
                0,
                Stroke::new(0.5_f32, colors.rule),
                egui::StrokeKind::Inside,
            );
            if let Some(date) = date {
                painter.text(
                    egui::pos2(cell.center().x, cell.top() + 12.0),
                    Align2::CENTER_CENTER,
                    date.day(),
                    FontId::monospace(14.0),
                    if date == today {
                        colors.accent
                    } else {
                        colors.text
                    },
                );
                if date == today {
                    painter.hline(
                        (cell.left() + 8.0)..=(cell.right() - 8.0),
                        cell.top() + 23.0,
                        Stroke::new(1.0_f32, colors.accent),
                    );
                }
                if let Some(items) = deadlines.get(&date) {
                    paint_marks(&painter, cell, items, colors);
                }
            }
        }
    }

    fn track(&mut self, response: &egui::Response) {
        self.keyboard_ids.push(response.id);
        self.focused |= response.has_focus() || response.clicked() || response.dragged();
    }

    fn paint(
        &self,
        ui: &Ui,
        rect: egui::Rect,
        state: &State,
        today: NaiveDate,
        selected: NaiveDate,
        lists: &[u64],
    ) {
        let colors = web_theme::colors(ui.ctx());
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        painter.rect_filled(rect, 0, colors.bg);
        let dates = self.view.visible(rect.width());
        let mut deadlines = BTreeMap::<_, Vec<_>>::new();
        if let (Some((first, _)), Some((last, _))) = (dates.first(), dates.last()) {
            for row in timeline::rows_for_lists(state, &Local, lists) {
                if let Some(due) = row.due.filter(|due| due >= first && due <= last) {
                    deadlines.entry(due).or_default().push(row);
                }
            }
        }
        for (index, (date, x)) in dates.iter().enumerate() {
            let left = rect.left() + x;
            let right = left + self.view.day_width();
            if index == 0 || date.day() == 1 {
                let next_month = date
                    .with_day(1)
                    .unwrap()
                    .checked_add_months(chrono::Months::new(1));
                let month_right = next_month.map_or(rect.right(), |next| {
                    (rect.left() + self.view.x_for(next)).min(rect.right())
                });
                let month_left = left.max(rect.left());
                if month_right - month_left > 28.0 {
                    painter.text(
                        egui::pos2(month_left + 4.0, rect.top() + 11.0),
                        Align2::LEFT_CENTER,
                        date.format(if month_right - month_left > 76.0 {
                            "%b %Y"
                        } else {
                            "%b"
                        }),
                        FontId::monospace(11.0),
                        colors.muted,
                    );
                }
            }
            let body = egui::Rect::from_min_max(
                egui::pos2(left + 1.0, rect.top() + MONTH_HEIGHT + 1.0),
                egui::pos2(right - 1.0, rect.bottom() - 1.0),
            );
            let fill = if *date == selected {
                colors.selected
            } else if body
                .contains(ui.input(|input| input.pointer.hover_pos().unwrap_or(egui::Pos2::ZERO)))
            {
                colors.hover
            } else if date.weekday().num_days_from_monday() >= 5 {
                colors.weekend
            } else {
                colors.bg
            };
            painter.rect_filled(body, 0, fill);
            painter.vline(
                left,
                (if date.day() == 1 {
                    rect.top()
                } else {
                    body.top()
                })..=rect.bottom(),
                Stroke::new(if date.day() == 1 { 1.2_f32 } else { 0.5_f32 }, colors.rule),
            );
            let center = (left + right) * 0.5;
            painter.text(
                egui::pos2(center, body.top() + 12.0),
                Align2::CENTER_CENTER,
                date.format("%a"),
                FontId::monospace(10.0),
                colors.muted,
            );
            painter.text(
                egui::pos2(center, body.top() + 32.0),
                Align2::CENTER_CENTER,
                date.day(),
                FontId::monospace(17.0),
                if *date == today {
                    colors.accent
                } else {
                    colors.text
                },
            );
            if *date == today {
                painter.hline(
                    (left + 7.0)..=(right - 7.0),
                    body.top() + 44.0,
                    Stroke::new(1.0_f32, colors.accent),
                );
            }
            if let Some(items) = deadlines.get(date) {
                paint_marks(
                    &painter,
                    egui::Rect::from_min_max(
                        egui::pos2(left, body.top()),
                        egui::pos2(right, body.bottom()),
                    ),
                    items,
                    colors,
                );
            }
        }
        painter.hline(
            rect.x_range(),
            rect.bottom(),
            Stroke::new(0.5_f32, colors.rule),
        );
    }
}

/// Keep color and completion marks identical in both calendar projections.
fn paint_marks(
    painter: &egui::Painter,
    rect: egui::Rect,
    items: &[timeline::TimelineRow<'_>],
    colors: web_theme::Palette,
) {
    let left = rect.left();
    let right = rect.right();
    let body = rect;
    let capacity = ((rect.width() - 16.0) / 7.0).floor().max(1.0) as usize;
    let count = items.len().min(capacity);
    // Give each color/completion combination a place before a
    // repeated mark can use the remaining room in this date.
    let mut marks = Vec::<usize>::with_capacity(count);
    for (index, item) in items.iter().enumerate() {
        if marks.iter().all(|&shown| {
            items[shown].color != item.color || items[shown].complete != item.complete
        }) {
            marks.push(index);
            if marks.len() == count {
                break;
            }
        }
    }
    if marks.len() < count {
        for index in 0..items.len() {
            if !marks.contains(&index) {
                marks.push(index);
                if marks.len() == count {
                    break;
                }
            }
        }
    }
    for (index, &item) in marks.iter().enumerate() {
        let item = &items[item];
        let center = egui::pos2(left + 6.0 + index as f32 * 7.0, body.bottom() - 10.0);
        let color = task_color(item.color, item.complete);
        if item.complete {
            painter.circle_stroke(center, 2.3, Stroke::new(1.0_f32, color));
        } else {
            painter.circle_filled(center, 2.3, color);
        }
    }
    if items.len() > count {
        let text = format!("+{}", items.len() - count);
        let label_left = left + 10.0 + (count - 1) as f32 * 7.0;
        let available = (right - 2.0 - label_left).max(1.0);
        let mut label = painter.layout_no_wrap(text.clone(), FontId::monospace(8.0), colors.muted);
        if label.size().x > available {
            label = painter.layout_no_wrap(
                text,
                FontId::monospace(8.0 * available / label.size().x),
                colors.muted,
            );
        }
        painter.galley(
            egui::pos2(label_left, body.bottom() - 10.0 - label.size().y * 0.5),
            label,
            colors.muted,
        );
    }
}
