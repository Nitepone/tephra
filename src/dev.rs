//! The developer API, under `/api/v1/dev`: runs app code that isn't saved anywhere and reports
//! back in detail. For people and agents writing apps, from scripts.
//!
//! It runs arbitrary Lua with network access, so it only answers requests from this machine.
//! The reference is the "Testing from the command line" page of the Lua docs (luadoc/dev-api.md).

use crate::apps::{self, LuaFailure, Slot};
use crate::model::StoredApp;
use crate::render::layout::{Dir, LayoutEntry, Node};
use crate::render::{BLACK, Canvas, Rect};
use crate::screen::{self, SlotDraw};
use crate::SharedState;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::time::Instant;

const RENDER_LIMIT_MS: u64 = 2_000;
const FETCH_LIMIT_MS: u64 = 60_000;
use crate::render::OG_IMAGE_LIMIT;
const SIZES: [&str; 4] = ["full", "large", "half", "small"];

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/render", post(render))
        .route("/fetch", post(fetch))
        .route("/apps", post(save_app))
        .route("/apps/{id}/export", get(export_app))
        .layer(DefaultBodyLimit::max(32 << 20))
        .route_layer(middleware::from_fn(local_only))
}

async fn local_only(ConnectInfo(peer): ConnectInfo<SocketAddr>, req: Request, next: Next) -> Response {
    if peer.ip().to_canonical().is_loopback() {
        next.run(req).await
    } else {
        let msg = "the developer API only answers requests from the machine tephra runs on";
        (StatusCode::FORBIDDEN, Json(json!({ "error": msg }))).into_response()
    }
}

/// The app being tried: its code, optional config check, and an instance config.
#[derive(Deserialize)]
struct AppCode {
    code: String,
    /// The config check. Runs when not empty.
    #[serde(default)]
    validate: String,
    /// The instance's config (Lua).
    #[serde(default)]
    config: String,
}

impl AppCode {
    fn meta(&self) -> apps::AppMeta {
        apps::stored_meta(&StoredApp {
            id: "dev".into(),
            name: "dev".into(),
            description: String::new(),
            code: self.code.clone(),
            validate_code: self.validate.clone(),
            validate_enabled: !self.validate.trim().is_empty(),
            prototype: false,
        })
    }
}

#[derive(Serialize)]
struct Failure {
    /// Where it failed: `load` (the code itself), `fetch` or `render`.
    stage: &'static str,
    #[serde(flatten)]
    failure: LuaFailure,
}

fn load_failure(meta: &apps::AppMeta) -> Option<Response> {
    let e = meta.error.as_ref()?;
    let body = json!({ "ok": false, "error": Failure { stage: "load", failure: LuaFailure::parse(e) } });
    Some((StatusCode::UNPROCESSABLE_ENTITY, Json(body)).into_response())
}

#[derive(Serialize)]
struct FetchReport {
    ms: u64,
    limit_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Failure>,
}

fn timed_fetch(meta: &apps::AppMeta, config: &str, data_dir: &std::path::Path) -> (Value, FetchReport) {
    let t = Instant::now();
    let result = apps::fetch_detailed(meta, config, data_dir);
    let ms = t.elapsed().as_millis() as u64;
    match result {
        Ok(data) => (data, FetchReport { ms, limit_ms: FETCH_LIMIT_MS, error: None }),
        Err(f) => (Value::Null, FetchReport { ms, limit_ms: FETCH_LIMIT_MS, error: Some(Failure { stage: "fetch", failure: f }) }),
    }
}

// ---- POST /dev/fetch ----

/// Runs config, check and fetch; returns the data, to keep and pass to `render` as `data`.
async fn fetch(State(s): State<SharedState>, Json(app): Json<AppCode>) -> Response {
    let meta = app.meta();
    if let Some(r) = load_failure(&meta) {
        return r;
    }
    if !meta.has_fetch {
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "ok": false, "error": { "stage": "load", "message": "the code has no fetch function" } })))
            .into_response();
    }
    let data_dir = s.cfg.data_dir.clone();
    let Ok((data, report)) = tokio::task::spawn_blocking(move || timed_fetch(&meta, &app.config, &data_dir)).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let ok = report.error.is_none();
    let status = if ok { StatusCode::OK } else { StatusCode::UNPROCESSABLE_ENTITY };
    (status, Json(json!({ "ok": ok, "data": data, "fetch": report }))).into_response()
}

// ---- POST /dev/render ----

#[derive(Deserialize)]
struct RenderIn {
    #[serde(flatten)]
    app: AppCode,
    /// Data to render with instead of running fetch.
    data: Option<Value>,
    /// One size, or several: `["full", "small"]` or `"all"`.
    #[serde(default)]
    size: Option<Value>,
    #[serde(default)]
    sizes: Option<Value>,
    #[serde(default = "og_w")]
    w: u32,
    #[serde(default = "og_h")]
    h: u32,
    /// Draw the slot's title bar, as on a device. Changes the space the app gets.
    #[serde(default = "yes")]
    title_bar: bool,
    /// 2 (black and white) or 4, as a device's Grays setting.
    #[serde(default = "two")]
    grays: u8,
    #[serde(default)]
    format: Option<String>,
}
fn two() -> u8 {
    2
}
fn og_w() -> u32 {
    800
}
fn og_h() -> u32 {
    480
}
fn yes() -> bool {
    true
}

#[derive(Deserialize)]
struct FormatQuery {
    format: Option<String>,
}

#[derive(Serialize)]
struct RenderReport {
    size: &'static str,
    /// The space the app drew into, in panel pixels.
    slot: Rect,
    ms: u64,
    limit_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Failure>,
    /// Every node: where it went and what of it didn't fit.
    layout: Vec<LayoutEntry>,
    /// The things in `layout` (and elsewhere) worth a look, in words.
    problems: Vec<String>,
    png_bytes: usize,
    /// 1, or 2 when the screen shows gray (and fits the OG's limit as 2-bit).
    png_bits: u8,
    /// The device image, base64 PNG. Left out with `format=png`.
    #[serde(skip_serializing_if = "Option::is_none")]
    png: Option<String>,
    #[serde(skip)]
    canvas: Option<Canvas>,
}

fn parse_sizes(v: &Option<Value>) -> Result<Vec<&'static str>, String> {
    let pick = |s: &str| SIZES.iter().find(|k| **k == s).copied().ok_or_else(|| format!("unknown size {s:?}: full, large, half or small"));
    match v {
        None => Ok(vec!["full"]),
        Some(Value::String(s)) if s == "all" => Ok(SIZES.to_vec()),
        Some(Value::String(s)) => Ok(vec![pick(s)?]),
        Some(Value::Array(a)) if !a.is_empty() => a.iter().map(|v| pick(v.as_str().unwrap_or(""))).collect(),
        _ => Err("size(s): a size, a list of sizes, or \"all\"".into()),
    }
}

fn preset_for(size: &str) -> &'static screen::Preset {
    screen::preset(match size {
        "large" => "two-thirds",
        "half" => "halves",
        "small" => "quadrants",
        _ => "full",
    })
}

#[allow(clippy::too_many_arguments)]
fn render_one(meta: &apps::AppMeta, config: &str, data: &Value, size: &'static str, w: u32, h: u32, grays: u8, title_bar: bool, data_dir: &std::path::Path) -> RenderReport {
    let p = preset_for(size);
    let bars: Vec<bool> = (0..p.slots.len()).map(|i| i == 0 && title_bar).collect();
    let geoms = screen::geometry(p, w, h, &bars);
    let g = geoms[0];
    let t = Instant::now();
    let result = apps::render_detailed(meta, config, data, Slot { w: g.content.w, h: g.content.h, size: g.size, grays }, data_dir);
    let ms = t.elapsed().as_millis() as u64;

    let mut cv = Canvas::new(w, h).with_grays(grays);
    let frames: Vec<SlotDraw> = geoms.iter().enumerate().map(|(i, g)| SlotDraw { geom: *g, content: None, title: if i == 0 { "App".into() } else { String::new() } }).collect();
    screen::compose_into(&mut cv, p.slots.len() > 1, &frames);
    let mut problems = Vec::new();
    let (error, layout) = match result {
        Ok(node) => {
            cv.report = Some(Vec::new());
            let clip = cv.push_clip(g.content.inset(-screen::PADDING));
            node.draw(&mut cv, g.content, BLACK, Dir::Column);
            cv.pop_clip(clip);
            (None, cv.report.take().unwrap_or_default())
        }
        Err(f) => {
            screen::error_node("App", &f.message).draw(&mut cv, g.content, BLACK, Dir::Column);
            problems.push(format!("render failed: {}", f.message));
            (Some(Failure { stage: "render", failure: f }), Vec::new())
        }
    };
    for e in &layout {
        let what = match &e.text {
            Some(t) => format!("{} {} {:?}", e.path, e.kind, t.chars().take(40).collect::<String>()),
            None => format!("{} {}", e.path, e.kind),
        };
        if !e.cut.is_empty() {
            let missing = e.missing.as_ref().map(|m| format!(" (no font has {m}; drawn as empty boxes)")).unwrap_or_default();
            problems.push(format!("{what}: {}{missing}", e.cut.join(", ")));
        }
        if let Some(px) = e.overflow_px {
            problems.push(format!("{what}: its children need {px}px more than it has"));
        }
    }
    if ms * 2 > RENDER_LIMIT_MS {
        problems.push(format!("render took {ms} ms of its {RENDER_LIMIT_MS} ms limit"));
    }
    let png = cv.to_device_png(OG_IMAGE_LIMIT).unwrap_or_default();
    let png_bits = if cv.has_gray() && png.len() > 25 && png[24] == 2 { 2 } else { 1 };
    if cv.has_gray() && png_bits == 1 {
        let gray_bytes = cv.to_png().map(|p| p.len()).unwrap_or_default();
        problems.push(format!(
            "the gray screen is {gray_bytes} bytes as a 2-bit image, more than an OG accepts ({OG_IMAGE_LIMIT}); it is sent dithered to black and white instead"
        ));
    }
    if png.len() > OG_IMAGE_LIMIT {
        problems.push(format!("the image is {} bytes; an OG accepts at most {OG_IMAGE_LIMIT}", png.len()));
    }
    use base64::Engine;
    RenderReport {
        size,
        slot: g.content,
        ms,
        limit_ms: RENDER_LIMIT_MS,
        error,
        layout,
        problems,
        png_bytes: png.len(),
        png_bits,
        png: Some(base64::engine::general_purpose::STANDARD.encode(&png)),
        canvas: Some(cv),
    }
}

/// Several renders side by side, two to a row, each labelled with its size.
fn sheet(renders: &[RenderReport], w: u32, h: u32) -> Canvas {
    const GAP: i32 = 24;
    const LABEL: i32 = 30;
    let (w, h) = (w as i32, h as i32);
    let cols = renders.len().min(2) as i32;
    let rows = (renders.len() as i32 + 1) / 2;
    let mut out = Canvas::new((cols * w + (cols + 1) * GAP) as u32, (rows * (h + LABEL + GAP) + GAP) as u32).with_grays(4);
    for (i, r) in renders.iter().enumerate() {
        let (cx, cy) = (i as i32 % 2, i as i32 / 2);
        let x = GAP + cx * (w + GAP);
        let y = GAP + cy * (h + LABEL + GAP);
        let label = format!("{}: slot {}×{}, {} ms{}", r.size, r.slot.w, r.slot.h, r.ms, if r.problems.is_empty() { String::new() } else { format!(", {} problems", r.problems.len()) });
        Node::text(label, "label").draw(&mut out, Rect::new(x, y, w, LABEL), BLACK, Dir::Column);
        out.border(Rect::new(x - 1, y + LABEL - 1, w + 2, h + 2), 1, BLACK);
        if let Some(cv) = &r.canvas {
            out.blit(cv, x, y + LABEL);
        }
    }
    out
}

async fn render(State(s): State<SharedState>, Query(q): Query<FormatQuery>, Json(req): Json<RenderIn>) -> Response {
    let bad = |m: String| (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": { "stage": "request", "message": m } }))).into_response();
    let sizes = match parse_sizes(if req.sizes.is_some() { &req.sizes } else { &req.size }) {
        Ok(s) => s,
        Err(e) => return bad(e),
    };
    if !(100..=4000).contains(&req.w) || !(100..=4000).contains(&req.h) {
        return bad("w and h: 100 to 4000 pixels".into());
    }
    if req.grays != 2 && req.grays != 4 {
        return bad("grays: 2 or 4".into());
    }
    let png_out = q.format.or(req.format.clone()).as_deref() == Some("png");
    let meta = req.app.meta();
    if let Some(r) = load_failure(&meta) {
        return r;
    }
    let data_dir = s.cfg.data_dir.clone();
    let joined = tokio::task::spawn_blocking(move || {
        let (data, fetch) = match (&req.data, meta.has_fetch) {
            (Some(d), _) => (d.clone(), None),
            (None, true) => {
                let (d, r) = timed_fetch(&meta, &req.app.config, &data_dir);
                (d, Some(r))
            }
            (None, false) => (Value::Null, None),
        };
        let renders: Vec<RenderReport> = sizes.iter().map(|size| render_one(&meta, &req.app.config, &data, size, req.w, req.h, req.grays, req.title_bar, &data_dir)).collect();
        (fetch, renders, req.w, req.h)
    })
    .await;
    let Ok((fetch, mut renders, w, h)) = joined else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let failed = fetch.as_ref().is_some_and(|f| f.error.is_some()) || renders.iter().any(|r| r.error.is_some());

    if png_out && !failed {
        let png = if renders.len() == 1 {
            renders[0].canvas.as_ref().and_then(|c| c.to_device_png(OG_IMAGE_LIMIT).ok())
        } else {
            sheet(&renders, w, h).to_png().ok()
        };
        return match png {
            Some(png) => ([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "no-store")], png).into_response(),
            None => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
    }
    if png_out {
        for r in &mut renders {
            r.png = None;
        }
    }
    let status = if failed { StatusCode::UNPROCESSABLE_ENTITY } else { StatusCode::OK };
    (status, Json(json!({ "ok": !failed, "fetch": fetch, "renders": renders }))).into_response()
}

// ---- POST /dev/apps, GET /dev/apps/{id}/export ----

#[derive(Deserialize)]
struct SaveIn {
    name: String,
    #[serde(default)]
    description: String,
    code: String,
    /// The config check; turned on when not empty.
    #[serde(default)]
    validate: String,
    /// Overwrite the app already called `name` instead of answering 409.
    #[serde(default)]
    replace: bool,
    #[serde(default = "yes")]
    prototype: bool,
}

fn request_error(status: StatusCode, message: String) -> Response {
    (status, Json(json!({ "ok": false, "error": { "stage": "request", "message": message } }))).into_response()
}

/// Saves an app as if written in the UI, after checking that its code loads and its config check
/// compiles.
async fn save_app(State(s): State<SharedState>, headers: axum::http::HeaderMap, Json(req): Json<SaveIn>) -> Response {
    let name = req.name.trim().to_string();
    let existing = s.apps.list().into_iter().find(|a| a.name.to_lowercase() == name.to_lowercase());
    let id = match (&existing, req.replace) {
        (Some(a), _) if a.builtin => {
            return request_error(StatusCode::CONFLICT, format!("{:?} is a built-in app and can't be replaced; choose another name", a.name));
        }
        (Some(a), true) => a.id.clone(),
        (Some(a), false) => {
            return request_error(
                StatusCode::CONFLICT,
                format!("an app is already called {:?} (id {}); send \"replace\": true to overwrite it", a.name, a.id),
            );
        }
        (None, _) => crate::control_api::new_app_id(),
    };
    let app = StoredApp {
        id,
        name,
        description: req.description.trim().into(),
        code: req.code,
        validate_enabled: !req.validate.trim().is_empty(),
        validate_code: req.validate,
        prototype: req.prototype,
    };
    let meta = apps::stored_meta(&app);
    if let Some(r) = load_failure(&meta) {
        return r;
    }
    if meta.validate_enabled {
        let lua = mlua::Lua::new();
        if let Err(e) = lua.load(&meta.validate_code).set_name("=config check").into_function() {
            let failure = LuaFailure::parse(&apps::short_error(&e));
            let body = json!({ "ok": false, "error": Failure { stage: "load", failure } });
            return (StatusCode::UNPROCESSABLE_ENTITY, Json(body)).into_response();
        }
    }
    let replaced = existing.is_some();
    match crate::control_api::store_app(&s, app, false) {
        Ok(out) => {
            let id = out.app.id.clone();
            let base = if s.cfg.public_url.is_empty() {
                let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("localhost");
                format!("http://{host}")
            } else {
                s.cfg.public_url.trim_end_matches('/').to_string()
            };
            let url = format!("{base}/#/apps/app/{}", urlencode(&id));
            Json(json!({ "ok": true, "id": id, "name": out.app.name, "url": url, "replaced": replaced, "prototype": out.app.prototype }))
                .into_response()
        }
        Err(e) => request_error(e.0, e.1),
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// The app's export file, exactly as the UI's Export button saves it.
async fn export_app(State(s): State<SharedState>, Path(id): Path<String>) -> Response {
    let Some(app) = s.apps.get(&id) else {
        return request_error(StatusCode::NOT_FOUND, "no such app".into());
    };
    match crate::control_api::export_file(&app) {
        Ok((filename, data)) => (
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{filename}\""))],
            data,
        )
            .into_response(),
        Err(e) => request_error(e.0, e.1),
    }
}
