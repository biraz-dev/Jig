//! Browser preferences and explicit, user-selected backup files.
use jig::{
    app_settings::{Action, AppSettings},
    model::State,
};
use serde::Deserialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_name = jigSettingsLoad)]
    fn load_text() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = jigSettingsSave)]
    fn save_text(text: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigSettingsExport)]
    fn download_text(text: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigSettingsImport)]
    fn import_text() -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigSettingsTakeImport)]
    fn take_text() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = jigSettingsCancelImport)]
    fn cancel_import_js();
    #[wasm_bindgen(catch, js_name = jigSettingsShortcuts)]
    fn shortcuts_js(text: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(js_name = jigSettingsCapture)]
    fn capture_js(capturing: bool);
    #[wasm_bindgen(js_name = jigBackupStatus)]
    fn backup_status() -> String;
    #[wasm_bindgen(catch, js_name = jigBackupChooseFolder)]
    fn choose_folder_js() -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigBackupAuthorizeFolder)]
    fn authorize_folder_js() -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigBackupDisconnectFolder)]
    fn disconnect_folder_js() -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigBackupWrite)]
    fn write_backup(text: &str, automatic: bool) -> Result<(), JsValue>;
    #[wasm_bindgen(js_name = jigBackupCancel)]
    fn cancel_backup();
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BackupStatus {
    pub supported: bool,
    pub busy: bool,
    pub folder: Option<String>,
    pub permission: String,
    pub message: String,
    pub last_backup: Option<String>,
}

fn error(value: JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| format!("Browser settings error: {value:?}"))
}

fn optional_text(value: JsValue) -> Result<Option<String>, String> {
    if value.is_null() || value.is_undefined() {
        Ok(None)
    } else {
        value
            .as_string()
            .map(Some)
            .ok_or("Expected settings JSON text".into())
    }
}

pub fn load() -> Result<Option<String>, String> {
    optional_text(load_text().map_err(error)?)
}

pub fn save(text: &str) -> Result<(), String> {
    save_text(text).map_err(error)
}

pub fn download(text: &str) -> Result<(), String> {
    download_text(text).map_err(error)
}

pub fn request_import() -> Result<(), String> {
    import_text().map_err(error)
}

pub fn take_import() -> Result<Option<String>, String> {
    optional_text(take_text().map_err(error)?)
}

pub fn cancel_import() {
    cancel_import_js();
}

pub fn configure_shortcuts(settings: &AppSettings) -> Result<(), String> {
    settings.validate()?;
    let shortcuts: Vec<_> = Action::ALL
        .into_iter()
        .filter_map(|action| settings.binding(action))
        .collect();
    let text = serde_json::to_string(&shortcuts).map_err(|error| error.to_string())?;
    shortcuts_js(&text).map_err(error)
}

pub fn set_capture(capturing: bool) {
    capture_js(capturing);
}

pub fn status() -> BackupStatus {
    serde_json::from_str(&backup_status()).unwrap_or_else(|error| BackupStatus {
        message: format!("Could not read backup status: {error}"),
        ..BackupStatus::default()
    })
}

pub fn choose_folder() -> Result<(), String> {
    choose_folder_js().map_err(error)
}

pub fn authorize_folder() -> Result<(), String> {
    authorize_folder_js().map_err(error)
}

pub fn disconnect_folder() -> Result<(), String> {
    disconnect_folder_js().map_err(error)
}

pub fn backup(state: &State, automatic: bool) -> Result<(), String> {
    state.validate()?;
    let text = serde_json::to_string_pretty(state).map_err(|error| error.to_string())?;
    write_backup(&text, automatic).map_err(error)
}

pub fn cancel_pending() {
    cancel_backup();
}
