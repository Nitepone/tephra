//! The JSON API the web UI uses, under `/api/v1`.

use crate::display::{self, Draft};
use crate::model::{self, Device, ScreenConfig};
use crate::{SharedState, screen};
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/status", get(status))
        .route("/presets", get(|| async { Json(screen::PRESETS) }))
        .route("/devices", get(devices).post(add_device))
        .route("/devices/{id}", axum::routing::patch(update_device).delete(delete_device))
        .route("/devices/{id}/screen", get(get_screen).put(put_screen))
        .route("/devices/{id}/screen.png", get(screen_png))
        .route("/devices/{id}/preview", post(preview_png))
        .route("/apps", get(apps))
        .route("/apps/reload", post(reload_apps))
        .route("/instances", get(instances).post(add_instance))
        .route("/instances/{id}", axum::routing::put(update_instance).delete(delete_instance))
        .route("/instances/{id}/fetch", post(fetch_instance))
        .route("/log", get(log))
}

/// An error the UI shows as is: `{"error": "..."}`.
pub struct ApiError(StatusCode, String);

impl ApiError {
    fn bad(msg: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, msg.into())
    }
    fn not_found(what: &str) -> Self {
        Self(StatusCode::NOT_FOUND, format!("no such {what}"))
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!("{e:#}");
        Self(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
    }
}
impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        anyhow::Error::from(e).into()
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

fn device_or_404(state: &SharedState, id: i64) -> ApiResult<Device> {
    model::device(&state.db.conn(), id)?.ok_or(ApiError::not_found("device"))
}

#[derive(Serialize)]
struct Status {
    version: &'static str,
    devices: usize,
    instances: usize,
}

async fn status(State(s): State<SharedState>) -> ApiResult<Json<Status>> {
    let c = s.db.conn();
    Ok(Json(Status { version: env!("CARGO_PKG_VERSION"), devices: model::devices(&c)?.len(), instances: model::instances(&c)?.len() }))
}

// ---- devices ----

#[derive(Serialize)]
struct DeviceOut {
    #[serde(flatten)]
    device: Device,
    label: String,
}

async fn devices(State(s): State<SharedState>) -> ApiResult<Json<Vec<DeviceOut>>> {
    let list = model::devices(&s.db.conn())?;
    Ok(Json(list.into_iter().map(|d| DeviceOut { label: d.label(), device: d }).collect()))
}

#[derive(Deserialize)]
struct NewDevice {
    name: String,
    #[serde(default)]
    model: String,
    width: u32,
    height: u32,
}

async fn add_device(State(s): State<SharedState>, Json(n): Json<NewDevice>) -> ApiResult<Json<DeviceOut>> {
    if !(100..=4000).contains(&n.width) || !(100..=4000).contains(&n.height) {
        return Err(ApiError::bad("width and height must be between 100 and 4000 pixels"));
    }
    let d = model::add_placeholder(&s.db.conn(), n.name.trim(), &n.model, n.width, n.height)?;
    Ok(Json(DeviceOut { label: d.label(), device: d }))
}

#[derive(Deserialize)]
struct DevicePatch {
    name: Option<String>,
    refresh_min_s: Option<u32>,
    refresh_max_s: Option<u32>,
    button: Option<String>,
    /// Placeholders only: a registered device reports its own size.
    width: Option<u32>,
    height: Option<u32>,
}

const BUTTON_FUNCTIONS: &[&str] = &["identify", "send_to_me", "add_wifi", "none"];

async fn update_device(State(s): State<SharedState>, Path(id): Path<i64>, Json(p): Json<DevicePatch>) -> ApiResult<StatusCode> {
    let d = device_or_404(&s, id)?;
    let min = p.refresh_min_s.unwrap_or(d.refresh_min_s);
    let max = p.refresh_max_s.unwrap_or(d.refresh_max_s);
    if min < 60 || max < min {
        return Err(ApiError::bad("refresh bounds: at least 60 s, and the longest no shorter than the shortest"));
    }
    if let Some(b) = &p.button
        && !BUTTON_FUNCTIONS.contains(&b.as_str())
    {
        return Err(ApiError::bad(format!("button: one of {}", BUTTON_FUNCTIONS.join(", "))));
    }
    if (p.width.is_some() || p.height.is_some()) && !d.placeholder {
        return Err(ApiError::bad("a connected device reports its own size"));
    }
    let c = s.db.conn();
    c.execute(
        "UPDATE devices SET name = ?, refresh_min_s = ?, refresh_max_s = ?, button = ?, width = ?, height = ? WHERE id = ?",
        rusqlite::params![
            p.name.as_deref().map(str::trim).unwrap_or(&d.name),
            min,
            max,
            p.button.as_deref().unwrap_or(&d.button),
            p.width.unwrap_or(d.width),
            p.height.unwrap_or(d.height),
            id
        ],
    )?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_device(State(s): State<SharedState>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let n = s.db.conn().execute("DELETE FROM devices WHERE id = ?", [id])?;
    if n == 0 {
        return Err(ApiError::not_found("device"));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---- screens ----

#[derive(Serialize)]
struct ScreenOut {
    #[serde(flatten)]
    screen: ScreenConfig,
    refresh_s: u64,
}

async fn get_screen(State(s): State<SharedState>, Path(id): Path<i64>) -> ApiResult<Json<ScreenOut>> {
    let d = device_or_404(&s, id)?;
    let screen = model::screen(&s.db.conn(), id)?;
    Ok(Json(ScreenOut { screen, refresh_s: display::refresh_s(&s, &d)? }))
}

fn check_screen(s: &SharedState, mut cfg: ScreenConfig) -> ApiResult<ScreenConfig> {
    if !screen::PRESETS.iter().any(|p| p.id == cfg.preset) {
        return Err(ApiError::bad(format!("unknown layout {:?}", cfg.preset)));
    }
    let n = screen::preset(&cfg.preset).slots.len();
    cfg.slots.resize(n, model::SlotConfig { instance_id: None, title_bar: true });
    let c = s.db.conn();
    for id in cfg.slots.iter().filter_map(|s| s.instance_id) {
        if model::instance(&c, id)?.is_none() {
            return Err(ApiError::bad(format!("no app instance {id}")));
        }
    }
    Ok(cfg)
}

async fn put_screen(State(s): State<SharedState>, Path(id): Path<i64>, Json(cfg): Json<ScreenConfig>) -> ApiResult<StatusCode> {
    device_or_404(&s, id)?;
    let cfg = check_screen(&s, cfg)?;
    model::save_screen(&mut s.db.conn(), id, &cfg)?;
    Ok(StatusCode::NO_CONTENT)
}

fn png(bytes: Vec<u8>) -> Response {
    ([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "no-store")], bytes).into_response()
}

/// The image the device would get right now.
async fn screen_png(State(s): State<SharedState>, Path(id): Path<i64>) -> ApiResult<Response> {
    let d = device_or_404(&s, id)?;
    Ok(png(display::render_png(&s, &d, None).await?))
}

/// The image with unsaved edits applied.
async fn preview_png(State(s): State<SharedState>, Path(id): Path<i64>, Json(mut draft): Json<Draft>) -> ApiResult<Response> {
    let d = device_or_404(&s, id)?;
    if let Some(cfg) = draft.screen.take() {
        draft.screen = Some(check_screen(&s, cfg)?);
    }
    Ok(png(display::render_png(&s, &d, Some(draft)).await?))
}

// ---- apps and their instances ----

async fn apps(State(s): State<SharedState>) -> Json<Vec<std::sync::Arc<crate::apps::AppMeta>>> {
    Json(s.apps.list())
}

async fn reload_apps(State(s): State<SharedState>) -> Json<Vec<std::sync::Arc<crate::apps::AppMeta>>> {
    let s2 = s.clone();
    let _ = tokio::task::spawn_blocking(move || s2.apps.reload()).await;
    Json(s.apps.list())
}

async fn instances(State(s): State<SharedState>) -> ApiResult<Json<Vec<model::Instance>>> {
    Ok(Json(model::instances(&s.db.conn())?))
}

#[derive(Deserialize)]
struct InstanceIn {
    #[serde(default)]
    app: String,
    name: String,
    #[serde(default)]
    settings: serde_json::Value,
}

/// The instance, after the fetch that saving it triggers.
#[derive(Serialize)]
struct InstanceSaved {
    instance: model::Instance,
    /// Why the fetch failed, if it did. The instance is saved either way.
    fetch_error: Option<String>,
}

async fn saved(s: &SharedState, id: i64) -> ApiResult<Json<InstanceSaved>> {
    let fetch_error = display::fetch_instance(s, id).await?.err();
    let instance = model::instance(&s.db.conn(), id)?.ok_or(ApiError::not_found("app instance"))?;
    Ok(Json(InstanceSaved { instance, fetch_error }))
}

fn settings_object(v: serde_json::Value) -> ApiResult<serde_json::Value> {
    match v {
        serde_json::Value::Null => Ok(json!({})),
        v @ serde_json::Value::Object(_) => Ok(v),
        _ => Err(ApiError::bad("settings must be an object")),
    }
}

async fn add_instance(State(s): State<SharedState>, Json(i): Json<InstanceIn>) -> ApiResult<Json<InstanceSaved>> {
    let app = s.apps.get(&i.app).ok_or_else(|| ApiError::bad(format!("no app {:?}", i.app)))?;
    let name = if i.name.trim().is_empty() { app.name.clone() } else { i.name.trim().to_string() };
    let id = model::add_instance(&s.db.conn(), &app.id, &name, &settings_object(i.settings)?)?;
    saved(&s, id).await
}

async fn update_instance(State(s): State<SharedState>, Path(id): Path<i64>, Json(i): Json<InstanceIn>) -> ApiResult<Json<InstanceSaved>> {
    let name = i.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad("a name is needed"));
    }
    if !model::update_instance(&s.db.conn(), id, name, &settings_object(i.settings)?)? {
        return Err(ApiError::not_found("app instance"));
    }
    saved(&s, id).await
}

async fn delete_instance(State(s): State<SharedState>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if s.db.conn().execute("DELETE FROM instances WHERE id = ?", [id])? == 0 {
        return Err(ApiError::not_found("app instance"));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn fetch_instance(State(s): State<SharedState>, Path(id): Path<i64>) -> ApiResult<Json<InstanceSaved>> {
    if model::instance(&s.db.conn(), id)?.is_none() {
        return Err(ApiError::not_found("app instance"));
    }
    saved(&s, id).await
}

// ---- log ----

#[derive(Deserialize)]
struct LogQuery {
    limit: Option<u32>,
}

async fn log(State(s): State<SharedState>, Query(q): Query<LogQuery>) -> ApiResult<Json<Vec<model::LogEntry>>> {
    Ok(Json(model::recent_log(&s.db.conn(), q.limit.unwrap_or(200).min(2000))?))
}
