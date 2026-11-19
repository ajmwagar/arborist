//! The fleet wall in a browser tab.
//!
//! The same `arborist_wall::Wall` the Spore renders, mounted on `gpui_web`
//! instead of `soil:present`. GPUI draws into a WebGPU canvas the page owns;
//! everything else — layout, colours, text — is the shared view, unchanged.
//!
//! This is the half the web bridge was built for: the module ships to the
//! browser (`GET /v1/spore/module.wasm`) and runs there, and the host
//! capabilities it needs arrive over HTTP+SSE rather than through wasmtime.
//! The wall needs none of them yet — it draws a report compiled in — which is
//! why this host is only a mount point and a boot.

use arborist_wall::{Wall, report_from};
use gpui::prelude::*;
use wasm_bindgen::prelude::*;

/// The report this build draws.
///
/// Compiled in, for the same reason the Spore compiles one in: the question
/// this host answers today is "does the wall run in a browser", and where the
/// document comes from is the next question. Swapping this for a fetch over the
/// bridge is a change to these two lines.
const SNAPSHOT: &str = include_str!("../../../fixtures/fleet.json");

/// The rendering backend a page may ask for.
///
/// `Auto` tries WebGPU and falls back to WebGL, which is right for a browser
/// that has both. Safari has `navigator.gpu` and an adapter, and still cannot
/// run this pipeline — the wall asks for texture-compression features WebKit
/// does not expose — so the page can name WebGL and get a picture instead of a
/// trap. A wrong name is `Auto`: a typo should render, not refuse.
fn backend(name: Option<&str>) -> gpui_platform::WebBackendPreference {
    match name {
        Some("webgl") | Some("gl") => gpui_platform::WebBackendPreference::WebGl,
        Some("webgpu") => gpui_platform::WebBackendPreference::WebGpu,
        _ => gpui_platform::WebBackendPreference::Auto,
    }
}

/// Boot the wall on the report the page fetched, or on the compiled snapshot.
///
/// `#[wasm_bindgen(start)]` used to run this at module init, which meant the
/// page could never hand it anything: the fixture was in the binary and the
/// binary started itself. The page now fetches `report.json` first and calls
/// this, and passes `None` when the fetch failed so the wall still draws
/// something rather than a blank rectangle.
#[wasm_bindgen]
pub fn boot(report: Option<String>, rendering: Option<String>) {
    console_error_panic_hook::set_once();
    let drawn = report
        .as_deref()
        .and_then(arborist_wall::report_from_document)
        .unwrap_or_else(|| report_from(SNAPSHOT));
    gpui_platform::web_init();
    let _handle = gpui_platform::application_with_web_backend(backend(rendering.as_deref()))
    .run_embedded(move |cx: &mut gpui::App| {
        // The report has to be *owned* by the window closure, not borrowed from
        // this stack frame: `open_window` keeps its builder past the call that
        // made it, so a borrow here would not outlive `boot`.
        let drawn = drawn.clone();
        // `run_embedded` takes a closure that builds the windows and returns
        // nothing; the handle the app hands back is the host's to keep alive.
        let _ = cx.open_window(Default::default(), move |_, cx| {
            cx.new(move |_| Wall::new(drawn))
        });
    });
}
