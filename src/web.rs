//! The web UI: two workspaces over one stylesheet and shared scripts, embedded in the binary.
//! "Screens" (`/`) arranges and configures apps; "Devices" (`/devices`) sets up the hardware.

use crate::SharedState;
use axum::Router;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;

const SCREENS_HTML: &str = include_str!("../web/index.html");
const DEVICES_HTML: &str = include_str!("../web/devices.html");
const APP_CSS: &str = include_str!("../web/app.css");
const UI_JS: &str = include_str!("../web/ui.js");
const SCREENS_JS: &str = include_str!("../web/screens.js");
const DEVICES_JS: &str = include_str!("../web/devices.js");

const HTML: &str = "text/html; charset=utf-8";
const CSS: &str = "text/css; charset=utf-8";
const JS: &str = "text/javascript; charset=utf-8";

/// One of the UI's own files. They change with the server, so browsers check back each time
/// (`no-cache`) instead of keeping an old page's script.
fn file(content_type: &'static str, body: &'static str) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
}

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/", get(|| async { file(HTML, SCREENS_HTML) }))
        .route("/devices", get(|| async { file(HTML, DEVICES_HTML) }))
        .route("/static/app.css", get(|| async { file(CSS, APP_CSS) }))
        .route("/static/ui.js", get(|| async { file(JS, UI_JS) }))
        .route("/static/screens.js", get(|| async { file(JS, SCREENS_JS) }))
        .route("/static/devices.js", get(|| async { file(JS, DEVICES_JS) }))
}
