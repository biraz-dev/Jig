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
    #[wasm_bindgen(catch, js_name = jigCsvImport)]
    fn csv_import() -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigCsvTake)]
    fn csv_take() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = jigCsvCancel)]
    fn csv_cancel();
    #[wasm_bindgen(catch, js_name = jigCsvExport)]
    fn csv_export(text: &str, name: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigOdsImport)]
    fn ods_import() -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigOdsTake)]
    fn ods_take() -> Result<Vec<u8>, JsValue>;
    #[wasm_bindgen(catch, js_name = jigBinaryExport)]
    fn binary_export(bytes: &[u8], name: &str, mime: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigPdfImport)]
    fn pdf_import() -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigPdfTake)]
    fn pdf_take() -> Result<Vec<u8>, JsValue>;
    #[wasm_bindgen(js_name = jigDocumentName)]
    fn document_name() -> String;
    #[wasm_bindgen(js_name = jigDocumentMime)]
    fn document_mime() -> String;
    #[wasm_bindgen(js_name = jigPdfRemaining)]
    fn pdf_remaining() -> usize;
    #[wasm_bindgen(js_name = jigPdfNext)]
    fn pdf_next();
    #[wasm_bindgen(js_name = jigDocumentCancel)]
    fn document_cancel();
    #[wasm_bindgen(js_name = jigWorkspaceMode)]
    fn workspace_mode(mode: &str);
    #[wasm_bindgen(catch, js_name = jigClipboardRead)]
    fn clipboard_read() -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = jigClipboardTake)]
    fn clipboard_take() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = jigOcrStart)]
    fn ocr_start(bytes: &[u8], name: &str, mime: &str) -> Result<u32, JsValue>;
    #[wasm_bindgen(catch, js_name = jigOcrTake)]
    fn ocr_take() -> Result<String, JsValue>;
    #[wasm_bindgen(js_name = jigOcrStatus)]
    fn ocr_status() -> String;
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

fn optional_text(value: JsValue) -> Result<Option<String>, String> {
    if value.is_null() || value.is_undefined() {
        Ok(None)
    } else {
        value
            .as_string()
            .map(Some)
            .ok_or_else(|| "Expected text".into())
    }
}

pub fn request_csv() -> Result<(), String> {
    csv_import().map_err(error)
}
pub fn take_csv() -> Result<Option<String>, String> {
    optional_text(csv_take().map_err(error)?)
}
pub fn cancel_csv() {
    csv_cancel();
}
pub fn download_csv(text: &str, name: &str) -> Result<(), String> {
    csv_export(text, name).map_err(error)
}
pub fn request_ods() -> Result<(), String> {
    ods_import().map_err(error)
}
pub fn take_ods() -> Result<Option<Vec<u8>>, String> {
    let bytes = ods_take().map_err(error)?;
    Ok((!bytes.is_empty()).then_some(bytes))
}
pub fn download_binary(bytes: &[u8], name: &str, mime: &str) -> Result<(), String> {
    binary_export(bytes, name, mime).map_err(error)
}
pub fn request_pdf() -> Result<(), String> {
    pdf_import().map_err(error)
}
pub struct ImportedFile {
    pub bytes: Vec<u8>,
    pub name: String,
    pub mime: String,
}
pub fn take_pdf() -> Result<Option<ImportedFile>, String> {
    let bytes = pdf_take().map_err(error)?;
    Ok((!bytes.is_empty()).then(|| ImportedFile {
        bytes,
        name: document_name(),
        mime: document_mime(),
    }))
}
pub fn remaining_pdf() -> usize {
    pdf_remaining()
}
pub fn next_pdf() {
    pdf_next();
}
pub fn cancel_documents() {
    document_cancel();
}
pub fn set_workspace(mode: &str) {
    workspace_mode(mode);
}
pub fn request_clipboard() -> Result<(), String> {
    clipboard_read().map_err(error)
}
pub fn take_clipboard() -> Result<Option<String>, String> {
    optional_text(clipboard_take().map_err(error)?)
}
pub fn start_ocr(file: &ImportedFile) -> Result<(), String> {
    ocr_start(&file.bytes, &file.name, &file.mime)
        .map(|_| ())
        .map_err(error)
}
pub fn take_ocr() -> Result<Option<String>, String> {
    let text = ocr_take().map_err(error)?;
    Ok((!text.is_empty()).then_some(text))
}
pub fn ocr_progress() -> String {
    ocr_status()
}
