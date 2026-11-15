//! Browser settings orchestration; document state and its Undo journal stay separate.
use super::*;
use crate::{settings_storage, settings_ui::SettingsAction};
use jig::app_settings::Chord;

impl BrowserApp {
    pub(super) fn shortcut_label(&self, action: AppAction) -> String {
        self.settings
            .binding(action)
            .map_or_else(|| "Unbound".into(), |chord| chord.label())
    }

    pub(super) fn consume_chord(ctx: &egui::Context, chord: Chord) -> bool {
        let Some(key) = egui::Key::from_name(chord.key.name()) else {
            return false;
        };
        ctx.input_mut(|input| {
            let mut consumed = false;
            input.events.retain(|event| {
                let matches = matches!(event, egui::Event::Key { key: pressed_key, pressed: true, modifiers, .. }
                    if *pressed_key == key && modifiers.ctrl == chord.ctrl
                        && modifiers.shift == chord.shift && modifiers.alt == chord.alt
                        && modifiers.mac_cmd == chord.meta);
                consumed |= matches;
                !matches
            });
            consumed
        })
    }

    pub(super) fn consume_shortcut(&self, ctx: &egui::Context, action: AppAction) -> bool {
        self.settings
            .binding(action)
            .is_some_and(|chord| Self::consume_chord(ctx, chord))
    }

    pub(super) fn navigation_shortcuts(&mut self, ctx: &egui::Context) {
        if self.consume_shortcut(ctx, AppAction::ToggleWorkspace) {
            self.set_workspace(!self.sheet_workspace, ctx);
        }
        if self.consume_shortcut(ctx, AppAction::Tasks) {
            self.show_tasks(ctx);
        }
        if self.consume_shortcut(ctx, AppAction::OpenLocal) {
            self.open_local();
        }
        if self.consume_shortcut(ctx, AppAction::Notes) {
            self.show_notes(ctx);
        }
        if self.consume_shortcut(ctx, AppAction::Calculator) {
            self.open_calculator(ctx);
        }
        if self.consume_shortcut(ctx, AppAction::Calendar) {
            self.toggle_calendar(ctx);
        }
        if self.consume_shortcut(ctx, AppAction::Data) {
            self.data_open = !self.data_open;
            ctx.request_repaint();
        }
    }

    pub(super) fn toggle_settings(&mut self, ctx: &egui::Context) {
        self.deferred_view = Some(DeferredView::Settings);
        ctx.request_repaint();
    }

    pub(super) fn close_settings(&mut self) {
        self.settings_open = false;
        self.ctx.memory_mut(|memory| memory.close_popup());
        self.settings_panel.cancel_capture();
        self.pending_settings = None;
        settings_storage::cancel_import();
        settings_storage::set_capture(false);
    }

    pub(super) fn settings_button(&mut self, ui: &mut Ui) {
        if ui
            .selectable_label(self.settings_open, "Settings")
            .on_hover_text(format!(
                "Preferences and shortcuts · {}",
                self.shortcut_label(AppAction::Settings)
            ))
            .clicked()
        {
            self.toggle_settings(ui.ctx());
        }
    }

    pub(super) fn commit_settings(&mut self, next: AppSettings, ctx: &egui::Context) -> bool {
        let result = next
            .to_json()
            .and_then(|json| settings_storage::save(&json));
        if let Err(error) = result {
            self.status = format!("Settings were not changed: {error}");
            return false;
        }
        let enable_backup = next.auto_backup && !self.settings.auto_backup;
        self.settings = next;
        self.trim_history();
        web_theme::apply(ctx, self.settings.theme);
        self.status = match settings_storage::configure_shortcuts(&self.settings) {
            Ok(()) => "Settings saved in this browser.".into(),
            Err(error) => {
                format!("Settings saved; browser shortcut handling could not update: {error}")
            }
        };
        if !self.settings.auto_backup {
            self.auto_backup_due = None;
            settings_storage::cancel_pending();
        } else if enable_backup && !self.demo && !self.dirty && !self.storage_error {
            self.auto_backup_due = Some(ctx.input(|input| input.time) + 300.0);
        }
        ctx.request_repaint();
        true
    }

    pub(super) fn process_settings_events(&mut self) {
        match settings_storage::take_import() {
            Ok(Some(text)) => match AppSettings::from_json(&text) {
                Ok(settings) => {
                    self.pending_settings = Some(settings);
                    self.settings_open = true;
                    self.settings_panel.open();
                    self.status = "Settings file read locally. Review before applying.".into();
                }
                Err(error) => self.status = format!("Settings import rejected: {error}"),
            },
            Ok(None) => {}
            Err(error) => self.status = format!("Settings import rejected: {error}"),
        }
        if !self.settings_open {
            settings_storage::set_capture(false);
        }
    }

    pub(super) fn settings_workspace(&mut self, ui: &mut Ui) {
        if let Some(imported) = self.pending_settings.clone() {
            ui.group(|ui| {
                ui.label(format!(
                    "Replace settings? Theme: {} · Undo levels: {} · {} shortcut overrides",
                    imported.theme.label(),
                    imported.undo_levels,
                    imported.shortcuts.len()
                ));
                ui.horizontal(|ui| {
                    if ui.button("Apply imported settings").clicked()
                        && self.commit_settings(imported, ui.ctx())
                    {
                        self.pending_settings = None;
                    }
                    if ui.button("Cancel import").clicked() {
                        self.pending_settings = None;
                    }
                });
            });
            ui.add_space(8.0);
        }
        let mut next = self.settings.clone();
        let backup = settings_storage::status();
        let response = self.settings_panel.ui(
            ui,
            &mut next,
            &backup,
            !self.demo,
            !self.dirty && !self.storage_error,
        );
        if response.changed {
            self.commit_settings(next, ui.ctx());
        }
        for action in response.actions {
            let result = match action {
                SettingsAction::Done => {
                    self.close_settings();
                    Ok(())
                }
                SettingsAction::ChooseFolder => settings_storage::choose_folder(),
                SettingsAction::AuthorizeFolder => settings_storage::authorize_folder(),
                SettingsAction::DisconnectFolder => {
                    self.auto_backup_due = None;
                    settings_storage::disconnect_folder()
                }
                SettingsAction::BackupNow => {
                    if self.demo || self.dirty || self.storage_error {
                        Err("Save your personal data before making a folder backup.".into())
                    } else {
                        self.auto_backup_due = None;
                        settings_storage::backup(&self.state, false, true)
                    }
                }
                SettingsAction::DownloadBackup => {
                    if self.demo {
                        Err("Open your local data before making a backup.".into())
                    } else {
                        browser_storage::download(&self.state)
                    }
                }
                SettingsAction::RestoreBackup => {
                    self.close_settings();
                    self.data_open = true;
                    browser_storage::request_import()
                }
                SettingsAction::ImportSettings => settings_storage::request_import(),
                SettingsAction::ExportSettings => self
                    .settings
                    .to_json()
                    .and_then(|json| settings_storage::download(&json)),
            };
            if let Err(error) = result {
                self.status = error;
            }
        }
        settings_storage::set_capture(self.settings_open && self.settings_panel.owns_keyboard());
    }

    pub(super) fn run_automatic_backup(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.initial_backup_pending)
            && self.settings.auto_backup
            && !self.demo
            && !self.dirty
            && !self.storage_error
        {
            // The constructor runs before egui has its first monotonic clock.
            self.auto_backup_due
                .get_or_insert(ctx.input(|input| input.time) + 300.0);
        }
        let Some(due) = self.auto_backup_due else {
            return;
        };
        if !self.settings.auto_backup || self.demo || self.storage_error {
            self.auto_backup_due = None;
            return;
        }
        if self.dirty || ctx.input(|input| input.time) < due {
            return;
        }
        let backup = settings_storage::status();
        if backup.busy {
            return;
        }
        self.auto_backup_due = None;
        if let Err(error) = settings_storage::backup(&self.state, true, true) {
            self.status = format!("Automatic backup could not start: {error}");
        }
    }
}
