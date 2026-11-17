//! Application preferences and shortcut capture, independent of document edits.
use crate::{settings_storage::BackupStatus, web_theme};
use eframe::egui::{self, RichText, Ui};
use jig::app_settings::{
    Action, AppSettings, Chord, CustomTheme, ShortcutKey, Theme, FIXED_SHORTCUTS, MAX_UNDO_LEVELS,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsAction {
    Done,
    ChooseFolder,
    AuthorizeFolder,
    DisconnectFolder,
    BackupNow,
    RetryBackup,
    DownloadBackup,
    RestoreBackup,
    OpenBackupSettings,
    ImportSettings,
    ExportSettings,
}

/// Recovery stays visible until the folder backup succeeds or its document is reset.
/// The compact version fits the main footer; Settings keeps the details above its scroll area.
pub fn backup_recovery(
    ui: &mut Ui,
    backup: &BackupStatus,
    saved: bool,
    compact: bool,
) -> Option<SettingsAction> {
    let error = backup.error.as_deref()?;
    let colors = web_theme::colors(ui.ctx());
    ui.push_id(("backup-recovery", compact), |ui| {
        if compact {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("Folder backup failed")
                        .small()
                        .color(colors.error),
                );
                if backup.busy {
                    ui.label(RichText::new("Working…").small().color(colors.muted));
                }
                recovery_buttons(ui, backup, saved, true)
            })
            .inner
        } else {
            egui::Frame::default()
                .fill(colors.surface)
                .stroke(egui::Stroke::new(1.0_f32, colors.error))
                .corner_radius(0.0)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_max_width(ui.available_width().min(900.0));
                    ui.label(
                        RichText::new("Folder backup failed")
                            .strong()
                            .color(colors.error),
                    );
                    ui.label(error);
                    if backup.busy {
                        quiet(ui, "Working…");
                    } else if !saved {
                        quiet(ui, "Save your changes before retrying, or download a copy of the current work.");
                    } else if backup.folder.is_some() && backup.permission != "granted" {
                        quiet(ui, "Reconnect the folder, then retry. You can also download a copy.");
                    }
                    recovery_buttons(ui, backup, saved, false)
                })
                .inner
        }
    })
    .inner
}

fn recovery_buttons(
    ui: &mut Ui,
    backup: &BackupStatus,
    saved: bool,
    compact: bool,
) -> Option<SettingsAction> {
    let ready = backup.supported && backup.folder.is_some() && backup.permission == "granted";
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(
                ready && saved && !backup.busy,
                egui::Button::new("Retry backup").min_size(egui::vec2(0.0, 24.0)),
            )
            .on_hover_text("Back up the current document. Unfinished text stays in its editor.")
            .clicked()
        {
            action = Some(SettingsAction::RetryBackup);
        }
        if backup.supported {
            let access = if backup.folder.is_none() {
                Some(("Choose folder", SettingsAction::ChooseFolder))
            } else if backup.permission != "granted" {
                Some(("Reconnect folder", SettingsAction::AuthorizeFolder))
            } else {
                None
            };
            if let Some((label, next)) = access {
                if ui
                    .add_enabled(
                        !backup.busy,
                        egui::Button::new(label).min_size(egui::vec2(0.0, 24.0)),
                    )
                    .clicked()
                {
                    action = Some(next);
                }
            }
        }
        if ui
            .add_enabled(
                !backup.busy,
                egui::Button::new("Download backup").min_size(egui::vec2(0.0, 24.0)),
            )
            .on_hover_text("Download the current document, including changes that could not save in this browser.")
            .clicked()
        {
            action = Some(SettingsAction::DownloadBackup);
        }
        if compact
            && ui
                .add(egui::Button::new("Settings").min_size(egui::vec2(0.0, 24.0)))
                .clicked()
        {
            action = Some(SettingsAction::OpenBackupSettings);
        }
    });
    action
}

#[derive(Default)]
pub struct SettingsResponse {
    pub changed: bool,
    pub actions: Vec<SettingsAction>,
}

#[derive(Default)]
pub struct SettingsPanel {
    capture: Option<Action>,
    error: Option<String>,
    focus_done: bool,
    undo_preview: Option<usize>,
    reveal_colors: bool,
    color_drafts: [String; 4],
    color_source: Option<CustomTheme>,
}

impl SettingsPanel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&mut self) {
        self.cancel_capture();
        self.focus_done = true;
        self.undo_preview = None;
        self.color_source = None;
    }

    pub fn owns_keyboard(&self) -> bool {
        self.capture.is_some()
    }

    /// Escape cancels an in-progress binding before closing Settings.
    pub fn cancel_capture(&mut self) -> bool {
        let cancelled = self.capture.take().is_some();
        if cancelled {
            self.error = None;
        }
        cancelled
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        settings: &mut AppSettings,
        backup: &BackupStatus,
        saved: bool,
    ) -> SettingsResponse {
        let mut response = SettingsResponse::default();
        let mut next = settings.clone();
        let capturing = self.owns_keyboard();
        if capturing {
            self.capture_key(ui.ctx(), &mut next);
        }
        ui.horizontal(|ui| {
            ui.heading("Settings");
            let done = ui.button("Done");
            if self.focus_done {
                done.request_focus();
                self.focus_done = false;
            }
            if done.clicked() {
                self.cancel_capture();
                response.actions.push(SettingsAction::Done);
            }
        });
        if let Some(action) = backup_recovery(ui, backup, saved, false) {
            response.actions.push(action);
        }
        egui::ScrollArea::vertical()
            .id_salt("application-settings")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(900.0));
                self.appearance(ui, &mut next, &mut response.actions);
                ui.separator();
                self.history(ui, &mut next);
                ui.separator();
                self.backups(ui, &mut next, backup, saved, &mut response.actions);
                ui.separator();
                self.shortcuts(ui, &mut next);
                ui.separator();
                self.customize_colors(ui, &mut next);
            });
        if !capturing && self.owns_keyboard() {
            // A click release and the first shortcut can arrive in one pass.
            self.capture_key(ui.ctx(), &mut next);
        }
        if !response.actions.is_empty() {
            self.cancel_capture();
        }
        if next != *settings {
            match next.validate() {
                Ok(()) => {
                    *settings = next;
                    response.changed = true;
                }
                Err(error) => self.error = Some(error),
            }
        }
        response
    }

    fn appearance(
        &mut self,
        ui: &mut Ui,
        settings: &mut AppSettings,
        actions: &mut Vec<SettingsAction>,
    ) {
        ui.label(RichText::new("Appearance").strong());
        let previous = settings.theme;
        ui.horizontal(|ui| {
            ui.label("Theme");
            egui::ComboBox::from_id_salt("application-theme")
                .selected_text(settings.theme.label())
                .show_ui(ui, |ui| {
                    for theme in Theme::ALL {
                        ui.selectable_value(&mut settings.theme, theme, theme.label());
                    }
                });
        });
        if settings.theme == Theme::Custom && previous != Theme::Custom {
            settings
                .custom_theme
                .get_or_insert_with(|| custom_from_palette(ui.ctx()));
            self.reveal_colors = true;
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("Export settings").clicked() {
                actions.push(SettingsAction::ExportSettings);
            }
            if ui.button("Import settings").clicked() {
                actions.push(SettingsAction::ImportSettings);
            }
        });
        quiet(ui, "Preferences are saved in this browser. Export settings to take them to another computer.");
    }

    fn customize_colors(&mut self, ui: &mut Ui, settings: &mut AppSettings) {
        let reveal = std::mem::take(&mut self.reveal_colors);
        let section = egui::CollapsingHeader::new("Customize colors")
            .id_salt("theme-colors")
            .open(reveal.then_some(true))
            .show(ui, |ui| {
                ui.checkbox(&mut settings.system_accent, "Use system accent when available");
                if settings.system_accent && !web_theme::system_accent_available(ui.ctx()) {
                    quiet(ui, "This browser does not expose a system accent. Jig keeps your selected accent instead.");
                }
                if settings.theme != Theme::Custom {
                    if ui.button("Customize this theme").clicked() {
                        settings.custom_theme = Some(custom_from_palette(ui.ctx()));
                        settings.theme = Theme::Custom;
                        self.color_source = None;
                        self.reveal_colors = true;
                    }
                    quiet(ui, "Copy this palette into Custom, then choose colors to match your desktop. Your custom palette stays saved when you try another preset.");
                }
                if settings.theme == Theme::Custom {
                    let mut custom = settings.custom_theme.unwrap_or_default();
                    let mut colors = [custom.background, custom.surface, custom.text, custom.accent];
                    if self.color_source != Some(custom) {
                        self.color_drafts = colors.map(hex_color);
                    }
                    for (index, (label, color)) in ["Background", "Surface", "Text", "Accent"]
                        .into_iter()
                        .zip(colors.iter_mut())
                        .enumerate()
                    {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(label);
                            let picked = ui.color_edit_button_srgb(color).changed();
                            if picked {
                                self.color_drafts[index] = hex_color(*color);
                            }
                            let field = ui.add(
                                egui::TextEdit::singleline(&mut self.color_drafts[index])
                                    .id(egui::Id::new(("theme-color-hex", index)))
                                    .desired_width(100.0)
                                    .char_limit(7)
                                    .hint_text("#RRGGBB"),
                            );
                            if reveal && index == 0 {
                                field.request_focus();
                            }
                            if field.changed() {
                                if let Some(parsed) = parse_hex_color(&self.color_drafts[index]) {
                                    *color = parsed;
                                }
                            }
                            if field.gained_focus() {
                                field.scroll_to_me(None);
                            }
                        });
                    }
                    [custom.background, custom.surface, custom.text, custom.accent] = colors;
                    if custom != settings.custom_theme.unwrap_or_default() {
                        settings.custom_theme = Some(custom);
                    }
                    self.color_source = Some(custom);
                    let text_contrast = jig::palette::contrast(custom.background, custom.text)
                        .min(jig::palette::contrast(custom.surface, custom.text));
                    if text_contrast < 4.5 {
                        quiet(ui, "Text contrast is low. Choose lighter text on dark backgrounds, or darker text on light backgrounds.");
                    }
                    if settings.system_accent {
                        quiet(ui, "The system accent takes precedence while available. Turn it off to use your custom accent.");
                    }
                    if ui.button("Reset to Ember").clicked() {
                        settings.custom_theme = Some(CustomTheme::default());
                        self.color_source = None;
                    }
                }
            });
        if reveal {
            section.header_response.scroll_to_me(Some(egui::Align::Min));
        }
    }

    fn history(&mut self, ui: &mut Ui, settings: &mut AppSettings) {
        ui.label(RichText::new("History").strong());
        let mut levels = self.undo_preview.unwrap_or(settings.undo_levels);
        let response = ui
            .horizontal(|ui| {
                let slider = ui.add(
                    egui::Slider::new(&mut levels, 0..=MAX_UNDO_LEVELS)
                        .text("Undo levels")
                        .show_value(false)
                        .integer(),
                );
                let number = ui.add(
                    egui::DragValue::new(&mut levels)
                        .range(0..=MAX_UNDO_LEVELS)
                        .speed(1.0)
                        .update_while_editing(false),
                );
                slider | number
            })
            .inner;
        if response.dragged() {
            self.undo_preview = Some(levels);
        } else if response.drag_stopped() || response.changed() {
            // Do not trim application history at intermediate drag positions.
            // Numeric entry and focused arrow keys still take effect at once.
            self.undo_preview = None;
            settings.undo_levels = levels;
        }
        quiet(ui, "Undo and Redo share a 64 MiB limit and last for this session. Zero disables application history; text editors keep their own history.");
    }

    fn backups(
        &self,
        ui: &mut Ui,
        settings: &mut AppSettings,
        backup: &BackupStatus,
        saved: bool,
        actions: &mut Vec<SettingsAction>,
    ) {
        ui.label(RichText::new("Files and backups").strong());
        if backup.supported {
            if let Some(folder) = &backup.folder {
                ui.label(format!("Backup folder: {folder}"));
            } else {
                quiet(ui, "No backup folder selected.");
            }
            ui.horizontal_wrapped(|ui| {
                let choose = if backup.folder.is_some() {
                    "Change folder"
                } else {
                    "Choose folder"
                };
                if ui
                    .add_enabled(!backup.busy, egui::Button::new(choose))
                    .clicked()
                {
                    actions.push(SettingsAction::ChooseFolder);
                }
                if backup.folder.is_some()
                    && backup.permission != "granted"
                    && ui
                        .add_enabled(!backup.busy, egui::Button::new("Reconnect folder"))
                        .clicked()
                {
                    actions.push(SettingsAction::AuthorizeFolder);
                }
                if backup.folder.is_some()
                    && ui
                        .add_enabled(!backup.busy, egui::Button::new("Disconnect folder"))
                        .clicked()
                {
                    actions.push(SettingsAction::DisconnectFolder);
                }
            });
        } else {
            quiet(ui, "This browser uses downloads for backups. A Chromium browser can write directly to a folder you choose.");
        }
        let ready = backup.supported && backup.folder.is_some() && backup.permission == "granted";
        ui.add_enabled(
            ready && saved || settings.auto_backup,
            egui::Checkbox::new(&mut settings.auto_backup, "Automatic folder backups"),
        );
        quiet(ui, "After saved changes, at most once every five minutes. Folder access is requested only when you choose or reconnect it.");
        if !saved {
            quiet(ui, "Folder backups resume after your changes save. Download a backup to keep the current work.");
        } else if settings.auto_backup && !ready {
            quiet(ui, "Automatic backups are waiting for folder access.");
        }
        ui.horizontal_wrapped(|ui| {
            if backup.supported
                && ui
                    .add_enabled(
                        ready && saved && !backup.busy,
                        egui::Button::new("Back up now"),
                    )
                    .clicked()
            {
                actions.push(SettingsAction::BackupNow);
            }
            if ui
                .add_enabled(!backup.busy, egui::Button::new("Download backup"))
                .clicked()
            {
                actions.push(SettingsAction::DownloadBackup);
            }
            if ui
                .add_enabled(!backup.busy, egui::Button::new("Restore backup"))
                .clicked()
            {
                actions.push(SettingsAction::RestoreBackup);
            }
        });
        quiet(ui, "Backups contain committed tasks, notes, dates and sheets. Restore opens the existing JSON import review.");
        if let Some(time) = &backup.last_backup {
            quiet(ui, &format!("Last backup: {time}"));
        }
        if !backup.message.is_empty() && backup.error.as_deref() != Some(backup.message.as_str()) {
            ui.label(&backup.message);
        }
    }

    fn shortcuts(&mut self, ui: &mut Ui, settings: &mut AppSettings) {
        ui.label(RichText::new("Application shortcuts").strong());
        quiet(ui, "Click a shortcut, then press its replacement. Esc cancels. Clear leaves an action without a shortcut.");
        if let Some(action) = self.capture {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!("Press a shortcut for {}", action.label()))
                        .color(web_theme::colors(ui.ctx()).accent),
                );
                if ui.button("Cancel capture").clicked() {
                    self.cancel_capture();
                }
            });
        }
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).color(web_theme::colors(ui.ctx()).error));
        }
        let narrow = ui.available_width() < 600.0;
        for action in Action::ALL {
            ui.push_id(action.label(), |ui| {
                if narrow {
                    ui.label(action.label()).on_hover_text(action.description());
                    ui.horizontal_wrapped(|ui| self.shortcut_controls(ui, settings, action));
                } else {
                    ui.horizontal(|ui| {
                        ui.add_sized([220.0, 20.0], egui::Label::new(action.label()))
                            .on_hover_text(action.description());
                        self.shortcut_controls(ui, settings, action);
                    });
                }
            });
        }
        if ui.button("Reset all shortcuts").clicked() {
            settings.reset_shortcuts();
            self.cancel_capture();
            self.error = None;
        }
        ui.add_space(7.0);
        egui::CollapsingHeader::new("Hotkey cheatsheet")
            .id_salt("fixed-shortcut-cheatsheet")
            .show(ui, |ui| {
                quiet(ui, "Application shortcuts above reflect your current bindings. These editor and navigation keys stay fixed.");
                for (keys, description) in FIXED_SHORTCUTS {
                    ui.label(RichText::new(*keys).strong());
                    quiet(ui, description);
                }
            });
    }

    fn shortcut_controls(&mut self, ui: &mut Ui, settings: &mut AppSettings, action: Action) {
        let binding = settings.binding(action);
        let label = if self.capture == Some(action) {
            "Press keys…".into()
        } else {
            binding.map_or_else(|| "Unassigned".into(), Chord::label)
        };
        let change = ui
            .add_sized([140.0, 22.0], egui::Button::new(label))
            .on_hover_text(format!("Change {} shortcut", action.label()));
        if change.clicked() {
            if !change.clicked_by(egui::PointerButton::Primary) {
                // Keep the key that activates this button out of the binding.
                ui.input_mut(|input| {
                    input.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
                    input.consume_key(egui::Modifiers::NONE, egui::Key::Space);
                });
            }
            self.capture = Some(action);
            self.error = None;
            ui.ctx().request_repaint();
        }
        if ui
            .add_enabled(
                binding != Some(action.default_binding()),
                egui::Button::new("Reset"),
            )
            .clicked()
        {
            match settings.reset_binding(action) {
                Ok(()) => {
                    self.cancel_capture();
                    self.error = None;
                }
                Err(error) => self.error = Some(error),
            }
        }
        if ui
            .add_enabled(binding.is_some(), egui::Button::new("Clear"))
            .clicked()
        {
            if let Err(error) = settings.set_binding(action, None) {
                self.error = Some(error);
            } else {
                self.cancel_capture();
                self.error = None;
            }
        }
    }

    fn capture_key(&mut self, ctx: &egui::Context, settings: &mut AppSettings) {
        let Some(action) = self.capture else {
            return;
        };
        let event = ctx.input(|input| {
            input.events.iter().find_map(|event| {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } = event
                {
                    Some((*key, *modifiers))
                } else {
                    None
                }
            })
        });
        ctx.input_mut(|input| {
            input
                .events
                .retain(|event| !matches!(event, egui::Event::Key { .. } | egui::Event::Text(_)))
        });
        let Some((key, modifiers)) = event else {
            return;
        };
        if key == egui::Key::Escape {
            self.cancel_capture();
            return;
        }
        let Some(key) = ShortcutKey::from_name(key.name()) else {
            self.error = Some("Choose a letter, number, function key or punctuation key.".into());
            return;
        };
        let chord = Chord {
            key,
            ctrl: modifiers.ctrl,
            shift: modifiers.shift,
            alt: modifiers.alt,
            meta: modifiers.mac_cmd,
        };
        match settings.set_binding(action, Some(chord)) {
            Ok(()) => {
                self.capture = None;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
}

fn quiet(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(12.0)
            .color(web_theme::colors(ui.ctx()).muted),
    );
}

fn custom_from_palette(ctx: &egui::Context) -> CustomTheme {
    let colors = web_theme::resolved(ctx);
    CustomTheme {
        background: colors.background,
        surface: colors.surface,
        text: colors.text,
        accent: colors.accent,
    }
}

fn hex_color(color: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2])
}

fn parse_hex_color(text: &str) -> Option<[u8; 3]> {
    let text = text.trim().strip_prefix('#').unwrap_or(text.trim());
    if text.len() != 6 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some([
        u8::from_str_radix(&text[0..2], 16).ok()?,
        u8::from_str_radix(&text[2..4], 16).ok()?,
        u8::from_str_radix(&text[4..6], 16).ok()?,
    ])
}
