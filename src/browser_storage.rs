//! Browser-only adapter. The shared core never touches DOM, storage or files.
use jig::model::State;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = jigSetUnsaved)]
    fn mark_unsaved(unsaved: bool);
    #[wasm_bindgen(catch, js_name = jigLoad)]
    fn load_json() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = jigSave)]
    fn save_json(text: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigClear)]
    fn clear_json() -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigExport)]
    fn export_json(text: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigImport)]
    fn import_json() -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigTakeImport)]
    fn take_json() -> Result<JsValue, JsValue>;
}
fn error(value: JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| format!("Browser storage error: {value:?}"))
}
fn decode(value: JsValue) -> Result<Option<State>, String> {
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    let text = value.as_string().ok_or("Expected JSON text")?;
    let state = State::from_json(&text)?;
    Ok(Some(state))
}
pub fn load() -> Result<Option<State>, String> {
    decode(load_json().map_err(error)?)
}
pub fn save(state: &State) -> Result<(), String> {
    state.validate()?;
    save_json(&serde_json::to_string_pretty(state).map_err(|e| e.to_string())?).map_err(error)
}
pub fn clear() -> Result<(), String> {
    clear_json().map_err(error)
}
pub fn download(state: &State) -> Result<(), String> {
    state.validate()?;
    export_json(&serde_json::to_string_pretty(state).map_err(|e| e.to_string())?).map_err(error)
}
pub fn request_import() -> Result<(), String> {
    import_json().map_err(error)
}
pub fn take_import() -> Result<Option<State>, String> {
    decode(take_json().map_err(error)?)
}

pub fn set_unsaved(unsaved: bool) {
    mark_unsaved(unsaved);
}
