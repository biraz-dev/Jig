//! A compact, day-based projection of the ordinary checklist and its dates.
use crate::web::task_color;
use crate::web_theme::{task_list_fill, ACCENT, BG, MUTED, RULE, TEXT, WEEKEND};
use chrono::{Datelike, Duration, Local, NaiveDate};
use eframe::egui::emath::GuiRounding;
use eframe::egui::{self, FontId, RichText, Stroke, Ui};
use jig::{
    model::State,
    timeline::{self, DateRange, ResizeEdge, Scale, TimelineRow},
};

const ROW_HEIGHT: f32 = 36.0;

pub fn label_split_id() -> egui::Id {
    egui::Id::new("timeline-label-divider")
}

fn edge_id(task: u64, edge: ResizeEdge) -> egui::Id {
    egui::Id::new(("timeline-resize", task, matches!(edge, ResizeEdge::Due)))
}

#[derive(Clone)]
struct GestureView {
    header: egui::Rect,
    clip: egui::Rect,
    window: DateRange,
    data: Vec<u8>,
}

impl GestureView {
    fn new(header: egui::Rect, clip: egui::Rect, window: DateRange, state: &State) -> Self {
        Self {
            header,
            clip,
            window,
            data: serde_json::to_vec(&(
                &state.tasks,
                &state.deadlines,
                &state.task_lists,
                state.preferences.timeline_label_percent,
            ))
            .unwrap_or_default(),
        }
    }

    fn matches(
        &self,
        header: egui::Rect,
        clip: egui::Rect,
        window: DateRange,
        state: &State,
    ) -> bool {
        self.header == header
            && self.clip == clip
            && self.window == window
            && self.data == Self::new(header, clip, window, state).data
    }
}

struct BarDrag {
    task: u64,
    edge: ResizeEdge,
    original: NaiveDate,
    preview: NaiveDate,
    pointer_origin: f32,
    row_y: f32,
    cell: f32,
    view: GestureView,
}

enum Gesture {
    Label {
        percent: f32,
        original: f32,
        view: GestureView,
    },
    Bar(BarDrag),
}

struct ResizeHandle {
    task: u64,
    edge: ResizeEdge,
    start: NaiveDate,
    due: NaiveDate,
    hit: egui::Rect,
    cap: egui::Rect,
    clip: egui::Rect,
    cell: f32,
}

#[derive(Clone, Copy)]
pub enum TimelineAction {
    EditTask(u64),
    EditDeadline(u64),
    LabelWidth(Option<u8>),
    ResizeTask {
        task: u64,
        edge: ResizeEdge,
        date: NaiveDate,
    },
}

pub struct TimelinePanel {
    anchor: NaiveDate,
    scale: Scale,
    selected: Option<(Option<u64>, Option<u64>)>,
    focus_requested: bool,
    gesture: Option<Gesture>,
    keyboard_ids: Vec<egui::Id>,
    visible_lists: Vec<u64>,
    pub focused: bool,
}

impl TimelinePanel {
    pub fn new(today: NaiveDate) -> Self {
        Self {
            anchor: today,
            scale: Scale::Week,
            selected: None,
            focus_requested: false,
            gesture: None,
            keyboard_ids: Vec::new(),
            visible_lists: Vec::new(),
            focused: false,
        }
    }

    pub fn reveal(&mut self, date: NaiveDate) {
        self.cancel_gesture();
        self.anchor = date;
    }

    pub fn request_focus(&mut self) {
        self.focus_requested = true;
    }

    pub fn cancel_gesture(&mut self) -> bool {
        self.gesture.take().is_some()
    }

    pub fn owns_keyboard(&self, ctx: &egui::Context) -> bool {
        ctx.memory(|memory| {
            memory
                .focused()
                .is_some_and(|id| id == label_split_id() || self.keyboard_ids.contains(&id))
        })
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        state: &State,
        today: NaiveDate,
        rows_height: f32,
        selected_lists: &[u64],
    ) -> Option<TimelineAction> {
        self.focused = false;
        self.keyboard_ids.clear();
        if self.visible_lists.as_slice() != selected_lists {
            // A changed filter owns a different row layout. Never finish an
            // old duration gesture against a task that has become hidden.
            self.cancel_gesture();
            self.selected = None;
            self.visible_lists = selected_lists.to_vec();
        }
        if ui.input(|input| {
            !input.focused
                || input.key_pressed(egui::Key::Escape)
                || input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Touch {
                            phase: egui::TouchPhase::Cancel,
                            ..
                        }
                    )
                })
        }) {
            self.cancel_gesture();
        }
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
        let mut rows = timeline::rows_for_lists(state, &Local, selected_lists);
        let combined = selected_lists.len() > 1;
        // Keep checklist order within each group, without reordering records.
        // Dated work remains immediately visible even with many undated notes.
        rows.sort_by_key(|row| row.due.is_none());
        let width = (ui.available_width() - 16.0).max(1.0);
        let (header, _) = ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::hover());
        if self.gesture.as_ref().is_some_and(|gesture| {
            let view = match gesture {
                Gesture::Label { view, .. } => view,
                Gesture::Bar(drag) => &drag.view,
            };
            !view.matches(header, ui.clip_rect(), window, state)
        }) {
            self.cancel_gesture();
        }
        let default_label = (width * 0.40).clamp(88.0, 190.0).min(width * 0.52);
        let label_percent = match &self.gesture {
            Some(Gesture::Label { percent, .. }) => Some(*percent),
            _ => state.preferences.timeline_label_percent.map(f32::from),
        };
        let label_width = label_percent
            .map_or(default_label, |percent| width * percent / 100.0)
            .clamp(88.0_f32.min(width * 0.5), (width - 90.0).max(width * 0.5));
        self.header(ui, header, label_width, window, today);
        if rows.is_empty() {
            ui.label(RichText::new("No tasks yet").color(MUTED).size(12.0));
            return self.label_splitter(ui, header, header.bottom(), label_width, window, state);
        }
        let mut action = None;
        let mut handles = Vec::new();
        let bottom = ui
            .scope(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let scroll = egui::ScrollArea::vertical()
                    .id_salt("timeline-rows")
                    .max_height(rows_height.max(ROW_HEIGHT))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let mut bottom = header.bottom();
                        for row in &rows {
                            let key = (row.task_id, row.deadline_id);
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(width, ROW_HEIGHT),
                                egui::Sense::hover(),
                            );
                            bottom = rect.bottom();
                            let response = ui.interact(
                                rect,
                                ui.id().with(("timeline-row", key)),
                                egui::Sense::click(),
                            );
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    ui.is_enabled(),
                                    format!(
                                        "{} · {} · {} · {}",
                                        row.title,
                                        state
                                            .task_list(row.list_id)
                                            .map_or("Tasks", |list| list.name.as_str()),
                                        status(row),
                                        dates(row)
                                    ),
                                )
                            });
                            self.focused |= response.has_focus();
                            if response.gained_focus() {
                                response.scroll_to_me(None);
                            }
                            let selected = self.selected == Some(key);
                            let mut display_row = *row;
                            if let Some(Gesture::Bar(drag)) = &self.gesture {
                                if Some(drag.task) == row.task_id {
                                    match drag.edge {
                                        ResizeEdge::Start => display_row.start = Some(drag.preview),
                                        ResizeEdge::Due => display_row.due = Some(drag.preview),
                                    }
                                }
                            }
                            let list = state.task_list(row.list_id);
                            let shade = if combined {
                                let index = state
                                    .task_lists
                                    .iter()
                                    .position(|list| list.id == row.list_id)
                                    .unwrap_or(0);
                                task_list_fill(index)
                            } else {
                                BG
                            };
                            self.row(
                                ui,
                                rect,
                                label_width,
                                window,
                                today,
                                &display_row,
                                selected,
                                combined.then_some(list.map_or("Tasks", |list| list.name.as_str())),
                                shade,
                            );
                            self.resize_handles(
                                rect,
                                ui.clip_rect(),
                                label_width,
                                window,
                                row,
                                &display_row,
                                &mut handles,
                            );
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
                                "{}\n{} · {} · {}",
                                row.title,
                                list.map_or("Tasks", |list| list.name.as_str()),
                                status(row),
                                dates(row)
                            ));
                        }
                        bottom
                    });
                // The scroll viewport can exceed its visible row content.
                // Keep the divider and its hit area inside the actual chart.
                scroll
                    .inner
                    .min(scroll.inner_rect.bottom())
                    .min(ui.clip_rect().bottom())
            })
            .inner;
        // Keep the established toolbar-to-row Tab order. Resize controls follow
        // all row buttons and take pointer priority over their broad hitboxes.
        if let Some(Gesture::Bar(drag)) = &self.gesture {
            if !handles.iter().any(|handle| {
                handle.task == drag.task
                    && handle.edge == drag.edge
                    && handle.cap.center().y == drag.row_y
            }) {
                self.cancel_gesture();
            }
        }
        for handle in handles {
            if let Some(resize) = self.resize_handle(ui, &handle, header, window, state) {
                action = Some(resize);
            }
        }
        if let Some(split) = self.label_splitter(ui, header, bottom, label_width, window, state) {
            action = Some(split);
        }
        if let Some(Gesture::Bar(drag)) = &self.gesture {
            if let Some(row) = rows.iter().find(|row| row.task_id == Some(drag.task)) {
                let mut preview = *row;
                match drag.edge {
                    ResizeEdge::Start => preview.start = Some(drag.preview),
                    ResizeEdge::Due => preview.due = Some(drag.preview),
                }
                ui.label(
                    RichText::new(format!("Preview · {}", dates(&preview)))
                        .color(ACCENT)
                        .size(11.0),
                );
            }
        }
        if let Some(selected) = rows
            .iter()
            .find(|row| self.selected == Some((row.task_id, row.deadline_id)))
        {
            ui.add_space(4.0);
            ui.label(
                RichText::new(selected.title)
                    .color(if selected.complete { MUTED } else { TEXT })
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

    fn label_splitter(
        &mut self,
        ui: &mut Ui,
        header: egui::Rect,
        bottom: f32,
        label_width: f32,
        window: DateRange,
        state: &State,
    ) -> Option<TimelineAction> {
        let boundary = header.left() + label_width;
        let hit = egui::Rect::from_min_max(
            egui::pos2(boundary - 8.0, header.top()),
            egui::pos2(boundary, bottom.max(header.bottom())),
        );
        let response = ui.interact(hit, label_split_id(), egui::Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
            .on_hover_text("Resize timeline titles · Left/Right arrows · Shift for larger steps · Double-click to reset");
        self.focused |= response.has_focus();
        lock_horizontal(ui, &response);
        if response.clicked() || response.drag_started() {
            response.request_focus();
        }
        let width = header.width();
        let bounded = |desired: f32| {
            desired.clamp(
                (width * 0.1).max(88.0_f32.min(width * 0.5)),
                (width * 0.8).min((width - 90.0).max(width * 0.5)),
            )
        };
        let mut action = None;
        if response.double_clicked() {
            self.cancel_gesture();
            if state.preferences.timeline_label_percent.is_some() {
                action = Some(TimelineAction::LabelWidth(None));
            }
        } else {
            if response.drag_started() {
                let percent = label_width / width * 100.0;
                self.gesture = Some(Gesture::Label {
                    percent,
                    original: percent,
                    view: GestureView::new(header, ui.clip_rect(), window, state),
                });
            }
            if response.dragged() || response.drag_stopped() {
                if let Some(Gesture::Label { percent, .. }) = &mut self.gesture {
                    // A release event can carry the final pointer movement.
                    let delta = ui.input(|input| input.pointer.delta().x);
                    *percent = bounded(label_width + delta) / width * 100.0;
                    ui.ctx().request_repaint();
                }
            }
            if response.drag_stopped() {
                if let Some(Gesture::Label {
                    percent, original, ..
                }) = self.gesture.take()
                {
                    let value = percent.round().clamp(10.0, 80.0) as u8;
                    if ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary))
                        && (percent - original).abs() > 0.01
                        && state.preferences.timeline_label_percent != Some(value)
                    {
                        action = Some(TimelineAction::LabelWidth(Some(value)));
                    }
                }
            }
            if response.has_focus() && self.gesture.is_none() && !response.drag_stopped() {
                let delta = arrow_delta(ui, 5);
                if delta != 0 {
                    let next = bounded(label_width + delta as f32 * width / 100.0);
                    let value = (next / width * 100.0).round().clamp(10.0, 80.0) as u8;
                    if (next - label_width).abs() > 0.01
                        && state.preferences.timeline_label_percent != Some(value)
                    {
                        action = Some(TimelineAction::LabelWidth(Some(value)));
                    }
                }
            }
        }
        response.widget_info(|| {
            egui::WidgetInfo::slider(
                ui.is_enabled(),
                f64::from(label_width / width * 100.0),
                "Timeline title width",
            )
        });
        let active = response.hovered() || response.has_focus() || response.dragged();
        let color = if active { ACCENT } else { RULE };
        let x = boundary.round_to_pixel_center(ui.painter().pixels_per_point());
        ui.painter()
            .vline(x, hit.y_range(), Stroke::new(1.0_f32, color));
        let grip = egui::Rect::from_center_size(
            egui::pos2(boundary - 4.0, header.center().y),
            egui::vec2(8.0, 18.0),
        );
        ui.painter().rect_filled(grip, 2.0, BG);
        for dy in [-3.0, 0.0, 3.0] {
            ui.painter().hline(
                (boundary - 6.0)..=(boundary - 2.0),
                header.center().y + dy,
                Stroke::new(1.0_f32, if active { ACCENT } else { MUTED }),
            );
        }
        if response.has_focus() {
            ui.painter().rect_stroke(
                grip,
                2.0,
                Stroke::new(1.0_f32, ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        action
    }

    #[allow(clippy::too_many_arguments)]
    fn resize_handles(
        &self,
        rect: egui::Rect,
        clip: egui::Rect,
        label_width: f32,
        window: DateRange,
        row: &TimelineRow<'_>,
        display: &TimelineRow<'_>,
        handles: &mut Vec<ResizeHandle>,
    ) {
        let (Some(task), Some(start), Some(due), Some(span)) = (
            row.task_id,
            row.start,
            row.due,
            timeline::clipped(row, window),
        ) else {
            return;
        };
        if start > due {
            return;
        }
        let lane =
            egui::Rect::from_min_max(egui::pos2(rect.left() + label_width, rect.top()), rect.max);
        let cell = lane.width() / window.days() as f32;
        let inset = (cell * 0.15).min(0.5);
        let base_left = lane.left() + span.offset as f32 * cell + inset;
        let base_right = lane.left() + (span.offset + span.days) as f32 * cell - inset;
        let middle = (base_left + base_right) * 0.5;
        for (edge, visible, original_x) in [
            (ResizeEdge::Start, !span.clipped_start, base_left),
            (ResizeEdge::Due, !span.clipped_end, base_right),
        ] {
            if !visible {
                continue;
            }
            let date = match edge {
                ResizeEdge::Start => display.start.unwrap_or(start),
                ResizeEdge::Due => display.due.unwrap_or(due),
            };
            let offset = (date - window.start).num_days() as f32
                + if edge == ResizeEdge::Due { 1.0 } else { 0.0 };
            let cap_x = (lane.left()
                + offset * cell
                + if edge == ResizeEdge::Start {
                    inset
                } else {
                    -inset
                })
            .clamp(lane.left(), lane.right());
            let mut hit = egui::Rect::from_center_size(
                egui::pos2(original_x, rect.center().y),
                egui::vec2(12.0, 24.0),
            );
            match edge {
                ResizeEdge::Start => hit.max.x = hit.max.x.min(middle),
                ResizeEdge::Due => hit.min.x = hit.min.x.max(middle),
            };
            hit = hit.intersect(lane).intersect(clip);
            if !hit.is_positive() {
                continue;
            }
            handles.push(ResizeHandle {
                task,
                edge,
                start,
                due,
                hit,
                cap: egui::Rect::from_center_size(
                    egui::pos2(cap_x, rect.center().y),
                    egui::vec2(2.0, 12.0),
                ),
                clip: clip.intersect(lane),
                cell,
            });
        }
    }

    fn resize_handle(
        &mut self,
        ui: &mut Ui,
        handle: &ResizeHandle,
        header: egui::Rect,
        window: DateRange,
        state: &State,
    ) -> Option<TimelineAction> {
        let id = edge_id(handle.task, handle.edge);
        self.keyboard_ids.push(id);
        let response = ui
            .interact(handle.hit, id, egui::Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        self.focused |= response.has_focus();
        lock_horizontal(ui, &response);
        let original = match handle.edge {
            ResizeEdge::Start => handle.start,
            ResizeEdge::Due => handle.due,
        };
        let clamp = |date: NaiveDate| match handle.edge {
            ResizeEdge::Start => date.min(handle.due),
            ResizeEdge::Due => date.max(handle.start),
        };
        if response.clicked() || response.drag_started() {
            response.request_focus();
        }
        if response.drag_started() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let origin = ui
                    .input(|input| input.pointer.press_origin())
                    .unwrap_or(pointer);
                self.gesture = Some(Gesture::Bar(BarDrag {
                    task: handle.task,
                    edge: handle.edge,
                    original,
                    preview: original,
                    pointer_origin: origin.x,
                    row_y: handle.cap.center().y,
                    cell: handle.cell,
                    view: GestureView::new(header, ui.clip_rect(), window, state),
                }));
            }
        }
        let mut action = None;
        if let Some(Gesture::Bar(drag)) = &mut self.gesture {
            if drag.task == handle.task && drag.edge == handle.edge {
                if response.dragged() || response.drag_stopped() {
                    if let Some(pointer) = response
                        .interact_pointer_pos()
                        .or_else(|| ui.input(|input| input.pointer.interact_pos()))
                    {
                        let days = ((pointer.x - drag.pointer_origin) / drag.cell).round() as i64;
                        if let Some(date) = drag.original.checked_add_signed(Duration::days(days)) {
                            drag.preview = clamp(date);
                        }
                        ui.ctx().request_repaint();
                    }
                }
                if response.drag_stopped() {
                    if ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary))
                        && drag.preview != drag.original
                    {
                        action = Some(TimelineAction::ResizeTask {
                            task: drag.task,
                            edge: drag.edge,
                            date: drag.preview,
                        });
                    }
                    self.gesture = None;
                    ui.ctx().request_repaint();
                }
            }
        }
        if response.has_focus() && self.gesture.is_none() && !response.drag_stopped() {
            let days = arrow_delta(ui, 7);
            if days != 0 {
                if let Some(date) = original.checked_add_signed(Duration::days(days)) {
                    let date = clamp(date);
                    if date != original {
                        action = Some(TimelineAction::ResizeTask {
                            task: handle.task,
                            edge: handle.edge,
                            date,
                        });
                    }
                }
            }
        }
        let date = match &self.gesture {
            Some(Gesture::Bar(drag)) if drag.task == handle.task && drag.edge == handle.edge => {
                drag.preview
            }
            _ => original,
        };
        let label = format!(
            "Resize {} date for {}",
            if handle.edge == ResizeEdge::Start {
                "start"
            } else {
                "due"
            },
            state
                .tasks
                .iter()
                .find(|task| task.id == handle.task)
                .map_or("task", |task| task.text.as_str())
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &label)
        });
        let active = response.hovered() || response.dragged() || response.has_focus();
        let painter = ui.painter().with_clip_rect(handle.clip);
        painter.rect_filled(handle.cap, 0.5, if active { ACCENT } else { TEXT });
        if response.has_focus() {
            painter.rect_stroke(
                handle.hit.shrink(1.0),
                1.0,
                Stroke::new(1.0_f32, ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        response.on_hover_text(format!(
            "{label}\n{date} · Drag by days · Left/Right arrows · Shift for one week"
        ));
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
        let lane =
            egui::Rect::from_min_max(egui::pos2(rect.left() + label_width, rect.top()), rect.max);
        self.grid(ui, lane, window, today, BG);
        let cell = lane.width() / window.days() as f32;
        let stride = if cell >= 15.0 {
            1
        } else if cell >= 8.0 {
            3
        } else {
            7
        };
        for offset in 0..window.days() {
            let Some(day) = window
                .start
                .checked_add_days(chrono::Days::new(offset.into()))
            else {
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

    fn grid(
        &self,
        ui: &Ui,
        rect: egui::Rect,
        window: DateRange,
        today: NaiveDate,
        background: egui::Color32,
    ) {
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, background);
        let cell = rect.width() / window.days() as f32;
        for offset in 0..window.days() {
            let Some(day) = window
                .start
                .checked_add_days(chrono::Days::new(offset.into()))
            else {
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
                    if background == BG {
                        WEEKEND
                    } else {
                        background.lerp_to_gamma(WEEKEND, 0.45)
                    },
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
        list_name: Option<&str>,
        background: egui::Color32,
    ) {
        let lane =
            egui::Rect::from_min_max(egui::pos2(rect.left() + label_width, rect.top()), rect.max);
        ui.painter().rect_filled(rect, 0.0, background);
        self.grid(ui, lane, window, today, background);
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
            if row.complete { MUTED } else { TEXT },
        );
        if row.complete {
            painter.hline(
                title_x..=(title_x + title.chars().count() as f32 * 6.6).min(lane.left() - 4.0),
                title_y,
                Stroke::new(0.7_f32, MUTED),
            );
        }
        let detail = row.due.map_or_else(
            || format!("{} · No date", status(row)),
            |date| format!("{} · {}", status(row), date.format("%d %b")),
        );
        let detail = if let Some(name) = list_name {
            format!("{detail} · {name}")
        } else {
            detail
        };
        painter.with_clip_rect(label_clip).text(
            egui::pos2(title_x, rect.top() + 26.0),
            egui::Align2::LEFT_CENTER,
            shortened(&detail, columns + 3),
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

fn lock_horizontal(ui: &Ui, response: &egui::Response) {
    if response.has_focus() {
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    ..Default::default()
                },
            )
        });
    }
}

fn arrow_delta(ui: &mut Ui, coarse: i64) -> i64 {
    ui.input_mut(|input| {
        if input.pointer.any_down() {
            return 0;
        }
        let arrow = input.events.iter().find_map(|event| match event {
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if matches!(key, egui::Key::ArrowLeft | egui::Key::ArrowRight)
                && !modifiers.ctrl
                && !modifiers.alt
                && !modifiers.command =>
            {
                Some((*key, *modifiers))
            }
            _ => None,
        });
        let Some((key, modifiers)) = arrow else {
            return 0;
        };
        if !input.consume_key(modifiers, key) {
            return 0;
        }
        let amount = if modifiers.shift { coarse } else { 1 };
        if key == egui::Key::ArrowLeft {
            -amount
        } else {
            amount
        }
    })
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
        (Some(start), Some(due)) => {
            format!("{} → {}", start.format("%d %b %Y"), due.format("%d %b %Y"))
        }
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
