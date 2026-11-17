//! The JSON API the web UI uses, under `/api/v1`.

use crate::display::{self, Draft};
use crate::apps::AppMeta;
use crate::model::{self, Device, ScreenConfig, StoredApp};
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
        .route("/apps", get(apps).post(new_app))
        .route("/apps/reload", post(reload_apps))
        .route("/apps/import", post(import_app))
        .route("/apps/{id}", get(get_app).put(update_app).delete(delete_app))
        .route("/apps/{id}/fork", post(fork_app))
        .route("/apps/{id}/export", get(export_app))
        .route("/apps/{id}/try", post(try_app))
        .route("/instances", get(instances).post(add_instance))
        .route("/instances/{id}", get(get_instance).put(update_instance).delete(delete_instance))
        .route("/instances/{id}/fetch", post(fetch_instance))
        .route("/log", get(log))
        .nest("/dev", crate::dev::router())
}

/// An error the UI shows as is: `{"error": "..."}`.
pub struct ApiError(pub StatusCode, pub String);

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

pub(crate) type ApiResult<T> = Result<T, ApiError>;

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
    grays: Option<u8>,
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
    if p.grays.is_some_and(|g| g != 2 && g != 4) {
        return Err(ApiError::bad("grays: 2 or 4"));
    }
    if (p.width.is_some() || p.height.is_some()) && !d.placeholder {
        return Err(ApiError::bad("a connected device reports its own size"));
    }
    let c = s.db.conn();
    c.execute(
        "UPDATE devices SET name = ?, refresh_min_s = ?, refresh_max_s = ?, button = ?, width = ?, height = ?, grays = ? WHERE id = ?",
        rusqlite::params![
            p.name.as_deref().map(str::trim).unwrap_or(&d.name),
            min,
            max,
            p.button.as_deref().unwrap_or(&d.button),
            p.width.unwrap_or(d.width),
            p.height.unwrap_or(d.height),
            p.grays.unwrap_or(d.grays),
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

// ---- apps ----

/// An app as the UI sees it.
#[derive(Serialize)]
pub(crate) struct AppOut {
    #[serde(flatten)]
    pub(crate) app: std::sync::Arc<AppMeta>,
    instances: i64,
    /// The Lua a new instance starts with.
    config_template: String,
}

fn app_out(s: &SharedState, app: std::sync::Arc<AppMeta>) -> ApiResult<AppOut> {
    let instances = model::instance_count(&s.db.conn(), &app.id)?;
    Ok(AppOut { config_template: app.config_template(), instances, app })
}

pub(crate) fn app_or_404(s: &SharedState, id: &str) -> ApiResult<std::sync::Arc<AppMeta>> {
    s.apps.get(id).ok_or(ApiError::not_found("app"))
}

/// A written app, or why it can't be changed.
fn editable(s: &SharedState, id: &str) -> ApiResult<std::sync::Arc<AppMeta>> {
    let app = app_or_404(s, id)?;
    if app.builtin {
        return Err(ApiError(StatusCode::FORBIDDEN, "built-in apps can't be changed: fork it to make your own copy".into()));
    }
    Ok(app)
}

async fn apps(State(s): State<SharedState>) -> ApiResult<Json<Vec<AppOut>>> {
    Ok(Json(s.apps.list().into_iter().map(|a| app_out(&s, a)).collect::<ApiResult<_>>()?))
}

async fn get_app(State(s): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<AppOut>> {
    Ok(Json(app_out(&s, app_or_404(&s, &id)?)?))
}

async fn reload_apps(State(s): State<SharedState>) -> ApiResult<Json<Vec<AppOut>>> {
    let stored = model::stored_apps(&s.db.conn())?;
    let s2 = s.clone();
    let _ = tokio::task::spawn_blocking(move || s2.apps.reload(&stored)).await;
    apps(State(s)).await
}

/// Saves a written app under a name no other app has, and loads it.
pub(crate) fn store_app(s: &SharedState, mut a: StoredApp, rename_if_taken: bool) -> ApiResult<AppOut> {
    a.name = a.name.trim().to_string();
    if a.name.is_empty() {
        return Err(ApiError::bad("an app needs a name"));
    }
    if s.apps.name_taken(&a.name, Some(&a.id)) {
        if !rename_if_taken {
            return Err(ApiError::bad(format!("another app is already called {:?}", a.name)));
        }
        a.name = s.apps.copy_name(&a.name);
    }
    model::save_app(&s.db.conn(), &a)?;
    let meta = s.apps.put(&a);
    app_out(s, meta)
}

pub(crate) fn new_app_id() -> String {
    format!("u{}", model::random_hex(4))
}

const NEW_APP_CODE: &str = r#"-- An app returns a table. render draws it; fetch (optional) gets its data.
-- The instance's config is run first and reaches the app as `config`.
return {
  refresh = "15m",
  settings = {
    { key = "message", label = "Message", default = "Hello from tephra" },
  },

  -- fetch = function(config)
  --   local r = http.get("https://example.com/data.json")
  --   return json.decode(r.body)
  -- end,

  render = function(data, slot, config)
    return ui.column {
      justify = "center",
      ui.text { config.message, style = "title", align = "center", wrap = true },
    }
  end,
}
"#;

async fn new_app(State(s): State<SharedState>) -> ApiResult<Json<AppOut>> {
    let a = StoredApp {
        id: new_app_id(),
        name: "New app".into(),
        description: String::new(),
        code: NEW_APP_CODE.into(),
        validate_code: String::new(),
        validate_enabled: false,
        prototype: false,
    };
    Ok(Json(store_app(&s, a, true)?))
}

#[derive(Deserialize)]
struct AppIn {
    name: String,
    #[serde(default)]
    description: String,
    code: String,
    #[serde(default)]
    validate_code: String,
    #[serde(default)]
    validate_enabled: bool,
    /// Left out: unchanged.
    prototype: Option<bool>,
}

async fn update_app(State(s): State<SharedState>, Path(id): Path<String>, Json(a): Json<AppIn>) -> ApiResult<Json<AppOut>> {
    let old = editable(&s, &id)?;
    let a = StoredApp {
        id,
        name: a.name,
        description: a.description.trim().into(),
        code: a.code,
        validate_code: a.validate_code,
        validate_enabled: a.validate_enabled,
        prototype: a.prototype.unwrap_or(old.prototype),
    };
    Ok(Json(store_app(&s, a, false)?))
}

async fn delete_app(State(s): State<SharedState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    editable(&s, &id)?;
    let n = model::instance_count(&s.db.conn(), &id)?;
    if n > 0 {
        return Err(ApiError::bad(format!("delete its {n} instance{} first", if n == 1 { "" } else { "s" })));
    }
    model::delete_app(&s.db.conn(), &id)?;
    s.apps.remove(&id);
    Ok(StatusCode::NO_CONTENT)
}

async fn fork_app(State(s): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<AppOut>> {
    let src = app_or_404(&s, &id)?;
    let a = StoredApp {
        id: new_app_id(),
        name: s.apps.copy_name(&src.name),
        description: src.description.clone(),
        code: src.code.clone(),
        validate_code: src.validate_code.clone(),
        validate_enabled: src.validate_enabled,
        prototype: false,
    };
    Ok(Json(store_app(&s, a, true)?))
}

/// What an export holds: base64 of this as JSON.
#[derive(Serialize, Deserialize)]
struct Exported {
    format: String,
    version: u32,
    name: String,
    #[serde(default)]
    description: String,
    code: String,
    #[serde(default)]
    validate_code: String,
    #[serde(default)]
    validate_enabled: bool,
}
const EXPORT_FORMAT: &str = "tephra-app";

/// An app as an export file: its name and contents (base64 text).
pub(crate) fn export_file(a: &AppMeta) -> ApiResult<(String, String)> {
    use base64::Engine;
    let e = Exported {
        format: EXPORT_FORMAT.into(),
        version: 1,
        name: a.name.clone(),
        description: a.description.clone(),
        code: a.code.clone(),
        validate_code: a.validate_code.clone(),
        validate_enabled: a.validate_enabled,
    };
    let data = base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&e).map_err(anyhow::Error::from)?);
    let file: String = a.name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    Ok((format!("{}.tephra-app", file.trim_matches('-')), data))
}

async fn export_app(State(s): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<serde_json::Value>> {
    let (filename, data) = export_file(&*app_or_404(&s, &id)?)?;
    Ok(Json(json!({ "filename": filename, "data": data })))
}

#[derive(Deserialize)]
struct ImportIn {
    data: String,
}

async fn import_app(State(s): State<SharedState>, Json(i): Json<ImportIn>) -> ApiResult<Json<AppOut>> {
    use base64::Engine;
    let compact: String = i.data.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(compact)
        .map_err(|_| ApiError::bad("that isn't an exported app: it isn't base64"))?;
    let e: Exported = serde_json::from_slice(&bytes).map_err(|e| ApiError::bad(format!("that isn't an exported app: {e}")))?;
    if e.format != EXPORT_FORMAT {
        return Err(ApiError::bad("that isn't an exported tephra app"));
    }
    let a = StoredApp {
        id: new_app_id(),
        name: e.name,
        description: e.description,
        code: e.code,
        validate_code: e.validate_code,
        validate_enabled: e.validate_enabled,
        prototype: false,
    };
    Ok(Json(store_app(&s, a, true)?))
}

#[derive(Deserialize)]
struct TryIn {
    /// Unsaved code; ignored for built-in apps, which run as installed.
    code: Option<String>,
    validate_code: Option<String>,
    validate_enabled: Option<bool>,
    #[serde(default)]
    config: String,
    #[serde(default = "full")]
    size: String,
    #[serde(default = "og_w")]
    width: u32,
    #[serde(default = "og_h")]
    height: u32,
    #[serde(default = "two")]
    grays: u8,
}
fn two() -> u8 {
    2
}
fn full() -> String {
    "full".into()
}
fn og_w() -> u32 {
    800
}
fn og_h() -> u32 {
    480
}

/// Runs an app (with unsaved code, for a written app) and returns what it draws.
async fn try_app(State(s): State<SharedState>, Path(id): Path<String>, Json(t): Json<TryIn>) -> ApiResult<Response> {
    let app = app_or_404(&s, &id)?;
    let meta = if app.builtin {
        (*app).clone()
    } else {
        crate::apps::stored_meta(&StoredApp {
            id: app.id.clone(),
            name: app.name.clone(),
            description: app.description.clone(),
            code: t.code.unwrap_or_else(|| app.code.clone()),
            validate_code: t.validate_code.unwrap_or_else(|| app.validate_code.clone()),
            validate_enabled: t.validate_enabled.unwrap_or(app.validate_enabled),
            prototype: false,
        })
    };
    let size = match t.size.as_str() {
        "large" => "large",
        "half" => "half",
        "small" => "small",
        _ => "full",
    };
    let (w, h) = (t.width.clamp(100, 4000), t.height.clamp(100, 4000));
    Ok(png(display::try_app(&s, meta, t.config, w, h, size, t.grays).await?))
}

// ---- instances ----

async fn instances(State(s): State<SharedState>) -> ApiResult<Json<Vec<model::Instance>>> {
    Ok(Json(model::instances(&s.db.conn())?))
}

async fn get_instance(State(s): State<SharedState>, Path(id): Path<i64>) -> ApiResult<Json<model::Instance>> {
    Ok(Json(model::instance(&s.db.conn(), id)?.ok_or(ApiError::not_found("app instance"))?))
}

#[derive(Deserialize)]
struct InstanceIn {
    #[serde(default)]
    app: String,
    #[serde(default)]
    name: String,
    /// Lua. Omitted on create: the app's template.
    config: Option<String>,
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

/// A config that doesn't run, or doesn't pass the app's check, is refused.
async fn check(app: std::sync::Arc<AppMeta>, config: String) -> ApiResult<()> {
    let r = tokio::task::spawn_blocking(move || crate::apps::check_config(&app, &config)).await.map_err(anyhow::Error::from)?;
    r.map_err(|e| ApiError::bad(format!("the config isn't valid: {e}")))
}

async fn add_instance(State(s): State<SharedState>, Json(i): Json<InstanceIn>) -> ApiResult<Json<InstanceSaved>> {
    let app = s.apps.get(&i.app).ok_or_else(|| ApiError::bad(format!("no app {:?}", i.app)))?;
    let name = if i.name.trim().is_empty() { app.name.clone() } else { i.name.trim().to_string() };
    let config = i.config.unwrap_or_else(|| app.config_template());
    if app.error.is_none() {
        check(app.clone(), config.clone()).await?;
    }
    let id = model::add_instance(&s.db.conn(), &app.id, &name, &config)?;
    saved(&s, id).await
}

async fn update_instance(State(s): State<SharedState>, Path(id): Path<i64>, Json(i): Json<InstanceIn>) -> ApiResult<Json<InstanceSaved>> {
    let name = i.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad("a name is needed"));
    }
    let inst = model::instance(&s.db.conn(), id)?.ok_or(ApiError::not_found("app instance"))?;
    let config = i.config.unwrap_or(inst.config);
    if let Some(app) = s.apps.get(&inst.app).filter(|a| a.error.is_none()) {
        check(app, config.clone()).await?;
    }
    model::update_instance(&s.db.conn(), id, name, &config)?;
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
