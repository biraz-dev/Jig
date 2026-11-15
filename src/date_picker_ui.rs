//! A small calendar and local-time popover. Changes stay in the caller's draft.
use crate::web_theme::{ACCENT, ERROR, MUTED, TEXT};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveTime, Utc};
use eframe::egui::{self, RichText, Stroke, Ui};
use jig::{calendar, task_dates};

#[derive(Clone)]
struct Picker {
    year: i32,
    month: u32,
    selected: NaiveDate,
    time: String,
    error: Option<String>,
}

/// Draw beside a date text field. Returns true when Use date accepts its draft,
/// including an unchanged date, so the caller can restore keyboard focus.
/// Opening, browsing, cancelling and closing do not edit application data.
pub fn button(
    ui: &mut Ui,
    id: egui::Id,
    draft: &mut String,
    existing: Option<DateTime<Utc>>,
    default_time: NaiveTime,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 26.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            "Choose date and time",
        )
    });
    let visuals = ui.style().interact(&response);
    ui.painter().rect(
        rect,
        2.0,
        visuals.bg_fill,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let icon = rect.shrink2(egui::vec2(7.0, 5.0));
    let color = if response.has_focus() { ACCENT } else { TEXT };
    ui.painter().rect_stroke(
        icon,
        1.0,
        Stroke::new(1.0_f32, color),
        egui::StrokeKind::Inside,
    );
    ui.painter().hline(
        icon.x_range(),
        icon.top() + 4.0,
        Stroke::new(1.0_f32, color),
    );
    for x in [icon.left() + 3.0, icon.right() - 3.0] {
        ui.painter().vline(
            x,
            (icon.top() - 2.0)..=(icon.top() + 2.0),
            Stroke::new(1.0_f32, color),
        );
    }
    for y in [icon.top() + 7.0, icon.top() + 11.0] {
        for x in [icon.left() + 3.0, icon.left() + 7.0, icon.left() + 11.0] {
            ui.painter().circle_filled(egui::pos2(x, y), 0.7, color);
        }
    }
    let response = response.on_hover_text("Choose a date and local time");
    let state_id = id.with("draft");
    if response.clicked() {
        let local = task_dates::resolve(draft, existing, default_time, &Local)
            .ok()
            .or(existing)
            .map(|instant| instant.with_timezone(&Local));
        let selected = local
            .as_ref()
            .map_or_else(|| Local::now().date_naive(), DateTime::date_naive);
        let time = local.as_ref().map_or(default_time, DateTime::time);
        ui.data_mut(|data| {
            data.insert_temp(
                state_id,
                Picker {
                    year: selected.year(),
                    month: selected.month(),
                    selected,
                    time: time.format("%H:%M").to_string(),
                    error: None,
                },
            );
        });
        ui.memory_mut(|memory| memory.toggle_popup(id));
    }
    let mut applied = false;
    if let Some(mut picker) = ui.data_mut(|data| data.get_temp::<Picker>(state_id)) {
        egui::popup::popup_below_widget(
            ui,
            id,
            &response,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                ui.set_width(236.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            calendar::shift(picker.year, picker.month, -1).is_ok(),
                            egui::Button::new("‹"),
                        )
                        .clicked()
                    {
                        if let Ok((year, month)) = calendar::shift(picker.year, picker.month, -1) {
                            picker.year = year;
                            picker.month = month;
                        }
                    }
                    let month = NaiveDate::from_ymd_opt(picker.year, picker.month, 1).unwrap();
                    ui.label(month.format("%B %Y").to_string());
                    if ui
                        .add_enabled(
                            calendar::shift(picker.year, picker.month, 1).is_ok(),
                            egui::Button::new("›"),
                        )
                        .clicked()
                    {
                        if let Ok((year, month)) = calendar::shift(picker.year, picker.month, 1) {
                            picker.year = year;
                            picker.month = month;
                        }
                    }
                });
                let time_id = id.with("time");
                if !ui.memory(|memory| memory.has_focus(time_id)) {
                    let delta = ui.input_mut(|input| {
                        if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                            -1
                        } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                            1
                        } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                            -7
                        } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                            7
                        } else {
                            0
                        }
                    });
                    if delta != 0 {
                        if let Some(date) =
                            picker.selected.checked_add_signed(Duration::days(delta))
                        {
                            picker.selected = date;
                            picker.year = date.year();
                            picker.month = date.month();
                        }
                    }
                }
                egui::Grid::new(id.with("month"))
                    .spacing(egui::vec2(3.0, 3.0))
                    .show(ui, |ui| {
                        for label in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
                            ui.label(RichText::new(label).color(MUTED).small());
                        }
                        ui.end_row();
                        if let Ok(days) = calendar::month(picker.year, picker.month) {
                            for (index, date) in days.into_iter().enumerate() {
                                if let Some(date) = date {
                                    let label = RichText::new(date.day().to_string()).color(
                                        if date == Local::now().date_naive() {
                                            ACCENT
                                        } else {
                                            TEXT
                                        },
                                    );
                                    let day = ui.add_sized(
                                        [29.0, 26.0],
                                        egui::Button::new(label).selected(date == picker.selected),
                                    );
                                    if day.clicked() {
                                        picker.selected = date;
                                        picker.error = None;
                                    }
                                } else {
                                    ui.allocate_space(egui::vec2(29.0, 26.0));
                                }
                                if index % 7 == 6 {
                                    ui.end_row();
                                }
                            }
                        }
                    });
                ui.separator();
                let mut enter = false;
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Time").color(MUTED));
                    let time = ui.add(
                        egui::TextEdit::singleline(&mut picker.time)
                            .id(time_id)
                            .desired_width(65.0)
                            .char_limit(5)
                            .hint_text("HH:MM")
                            .return_key(None),
                    );
                    enter = time.has_focus()
                        && ui.input_mut(|input| {
                            input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                        });
                    ui.label(RichText::new("local · 24h").small().color(MUTED));
                });
                ui.horizontal(|ui| {
                    if ui.small_button("Today").clicked() {
                        let date = Local::now().date_naive();
                        picker.selected = date;
                        picker.year = date.year();
                        picker.month = date.month();
                        picker.error = None;
                    }
                    if ui.button("Use date").clicked() || enter {
                        let local = format!("{}T{}", picker.selected, picker.time.trim());
                        match task_dates::resolve(&local, existing, default_time, &Local) {
                            Ok(instant) => {
                                let next = instant.to_rfc3339();
                                applied = true;
                                *draft = next;
                                ui.memory_mut(|memory| memory.close_popup());
                            }
                            Err(error) => picker.error = Some(error),
                        }
                    }
                    if ui.small_button("Cancel").clicked() {
                        ui.memory_mut(|memory| memory.close_popup());
                    }
                });
                if let Some(error) = &picker.error {
                    ui.label(RichText::new(error).small().color(ERROR));
                }
            },
        );
        ui.data_mut(|data| data.insert_temp(state_id, picker));
    }
    applied
}
