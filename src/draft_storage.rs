//! Native browser text editing; document ownership and history stay in Rust.
use eframe::egui;
use jig::draft::Draft;
use serde::Deserialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = jigDraftInit)]
    fn init(wake: &JsValue);
    #[wasm_bindgen(catch, js_name = jigDraftSync)]
    fn sync_json(json: &str, force: bool, saved: bool) -> Result<(), JsValue>;
    #[wasm_bindgen(js_name = jigDraftLayout)]
    fn layout_js(x: f32, y: f32, width: f32, height: f32, dark: bool, visible: bool, colors: &str);
    #[wasm_bindgen(js_name = jigDraftFocus)]
    pub fn focus();
    #[wasm_bindgen(catch, js_name = jigDraftTake)]
    fn take_json() -> Result<String, JsValue>;
    #[wasm_bindgen(js_name = jigDraftPending)]
    pub fn pending() -> bool;
    #[wasm_bindgen(js_name = jigDraftError)]
    pub fn show_error(message: &str);
    #[wasm_bindgen(catch, js_name = jigDraftResetRecovery)]
    fn reset_recovery_js() -> Result<(), JsValue>;
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Change { draft: Draft, coalesce: bool },
    Undo,
    Redo,
    Note { text: String },
    Task { text: String },
}

pub fn install(ctx: egui::Context) {
    let wake = Closure::<dyn Fn()>::new(move || ctx.request_repaint());
    init(wake.as_ref());
    // One callback belongs to the browser application's entire lifetime.
    wake.forget();
}

pub fn sync(draft: &Draft, force: bool, saved: bool) -> Result<(), String> {
    let json = serde_json::to_string(draft).map_err(|error| error.to_string())?;
    sync_json(&json, force, saved).map_err(error)
}

pub fn reset_recovery() -> Result<(), String> {
    reset_recovery_js().map_err(error)
}

pub fn take() -> Result<Vec<Event>, String> {
    let json = take_json().map_err(error)?;
    if json.len() > 4 * 1024 * 1024 {
        return Err(
            "Draft input exceeds the local edit limit. Copy the text before reloading.".into(),
        );
    }
    serde_json::from_str(&json).map_err(|error| format!("Could not read Draft edits: {error}"))
}

pub fn layout(rect: Option<egui::Rect>, zoom: f32, dark: bool, colors: crate::web_theme::Palette) {
    let visible = rect.is_some();
    let rect = rect.unwrap_or(egui::Rect::ZERO);
    let colors = [
        colors.bg,
        colors.surface,
        colors.text,
        colors.muted,
        colors.rule,
        colors.accent,
        colors.selected,
    ]
    .map(crate::web_theme::srgb);
    let colors = serde_json::to_string(&colors).unwrap_or_default();
    layout_js(
        rect.left() * zoom,
        rect.top() * zoom,
        rect.width() * zoom,
        rect.height() * zoom,
        dark,
        visible,
        &colors,
    );
}

fn error(value: JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| format!("Draft: {value:?}"))
}
