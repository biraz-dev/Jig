#[cfg(target_arch = "wasm32")]
#[path = "../browser_storage.rs"]
mod browser_storage;
#[cfg(target_arch = "wasm32")]
#[path = "../web.rs"]
mod web;

#[cfg(target_arch = "wasm32")]
#[path = "../stl_ui.rs"]
mod stl_ui;

#[cfg(target_arch = "wasm32")]
#[path = "../timeline_ui.rs"]
mod timeline_ui;

#[cfg(target_arch = "wasm32")]
#[path = "../weather_client.rs"]
mod weather_client;
#[cfg(target_arch = "wasm32")]
#[path = "../weather_ui.rs"]
mod weather_ui;

#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::{JsCast, JsValue};
    wasm_bindgen_futures::spawn_local(async {
        let result = async {
            let document = web_sys::window()
                .ok_or("No browser window")?
                .document()
                .ok_or("No document")?;
            let canvas = document
                .get_element_by_id("jig_canvas")
                .ok_or("Missing dashboard canvas")?
                .dyn_into::<web_sys::HtmlCanvasElement>()?;
            let runner = eframe::WebRunner::new();
            runner
                .start(
                    canvas,
                    eframe::WebOptions::default(),
                    Box::new(|cc| Ok(Box::new(web::BrowserApp::new(cc)))),
                )
                .await?;
            // Keep the runner alive for the lifetime of this browser page.
            std::mem::forget(runner);
            started();
            Ok::<(), JsValue>(())
        }
        .await;
        if let Err(error) = result {
            failed(&format!("{error:?}"));
        }
    });
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = jigStarted)]
    fn started();
    #[wasm_bindgen(js_name = jigFailed)]
    fn failed(message: &str);
}
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("Build the browser preview with: trunk serve --public-url / (see docs/preview.md)");
}
