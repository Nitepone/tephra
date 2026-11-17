//! Lua apps: built-in ones loaded from `apps/<id>/app.lua`, and ones written, forked or
//! imported in the UI, stored in the database. Running one means, in a fresh sandboxed Lua state:
//! the instance's config (plain Lua), the app's defaults for anything it left out, the app's
//! optional config check, and then the app itself.
//!
//! A fresh state per call means apps can't keep state between calls or interfere with each
//! other, and a crashed or runaway script costs nothing beyond its own call. Calls block; run
//! them with `spawn_blocking`.

use crate::model::StoredApp;
use crate::render::decode_with_alpha;
use crate::render::font::{Face, TextStyle};
use crate::render::layout::{Align, Common, Dir, Edges, Fill, Fit, Justify, Node, TextAlign};
use mlua::{HookTriggers, Lua, LuaOptions, LuaSerdeExt, StdLib, Table, Value, VmState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// A setting an app documents: its default fills in for a config that leaves it out, and it
/// makes up the starting config of a new instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingField {
    pub key: String,
    #[serde(default)]
    pub label: String,
    /// `string`, `text`, `number`, `bool` or `choice`: documentation only, since configs are Lua.
    /// `duration` ("6h"), `zone` ("Europe/London") and `time` ("16:04") are also checked before
    /// the app runs.
    #[serde(rename = "type", default = "default_type")]
    pub kind: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub default: serde_json::Value,
    #[serde(default)]
    pub help: String,
}
fn default_type() -> String {
    "string".into()
}

#[derive(Debug, Clone, Serialize)]
pub struct AppMeta {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Shipped as files in the apps directory: read-only in the UI.
    pub builtin: bool,
    pub code: String,
    /// Lua that checks `config` (with `assert`/`error`) before the app runs.
    pub validate_code: String,
    pub validate_enabled: bool,
    /// Pushed by an agent through the developer API, and marked so in the UI until someone unticks it.
    pub prototype: bool,
    /// How often `fetch` runs; also the shortest refresh it asks of a device showing it.
    pub refresh_s: u64,
    pub settings: Vec<SettingField>,
    pub has_fetch: bool,
    /// Why the app's code could not be loaded. Such an app is listed but cannot run.
    pub error: Option<String>,
    /// A built-in app's directory, where its image files are.
    #[serde(skip)]
    pub dir: Option<PathBuf>,
}

impl AppMeta {
    /// The Lua a new instance starts from: each documented setting at its default.
    pub fn config_template(&self) -> String {
        if self.settings.is_empty() {
            return "-- This app has no documented settings.\n".into();
        }
        let mut out = String::new();
        for f in &self.settings {
            let mut about = if f.label.is_empty() { f.key.clone() } else { f.label.clone() };
            if !f.options.is_empty() {
                about += &format!(" (one of: {})", f.options.join(", "));
            }
            if !f.help.is_empty() {
                about += &format!(". {}", f.help);
            }
            out += &format!("-- {about}\n");
            if f.default.is_null() {
                out += &format!("-- {} = \"\"\n", lua_key(&f.key));
            } else {
                out += &format!("{} = {}\n", lua_key(&f.key), lua_literal(&f.default));
            }
        }
        out
    }
}

/// The installed apps: the apps directory's, then the database's.
pub struct Apps {
    dir: PathBuf,
    map: RwLock<BTreeMap<String, Arc<AppMeta>>>,
}

impl Apps {
    pub fn load(dir: &Path, stored: &[StoredApp]) -> Self {
        let apps = Self { dir: dir.to_path_buf(), map: RwLock::default() };
        apps.reload(stored);
        apps
    }

    /// Reads the apps directory again and rebuilds every app.
    pub fn reload(&self, stored: &[StoredApp]) {
        let mut map = BTreeMap::new();
        let entries = std::fs::read_dir(&self.dir).map(|rd| rd.flatten().collect::<Vec<_>>()).unwrap_or_else(|e| {
            tracing::warn!("cannot read the apps directory {}: {e}", self.dir.display());
            Vec::new()
        });
        for e in entries {
            let dir = e.path();
            if dir.join("app.lua").is_file() {
                let meta = builtin_meta(&dir);
                if let Some(err) = &meta.error {
                    tracing::warn!(app = meta.id, "{err}");
                }
                map.insert(meta.id.clone(), Arc::new(meta));
            }
        }
        let builtin = map.len();
        for s in stored {
            map.insert(s.id.clone(), Arc::new(stored_meta(s)));
        }
        tracing::info!("{builtin} built-in apps from {}, {} written here", self.dir.display(), stored.len());
        *self.map.write().unwrap() = map;
    }

    /// Rebuilds one written app after it was saved.
    pub fn put(&self, s: &StoredApp) -> Arc<AppMeta> {
        let meta = Arc::new(stored_meta(s));
        self.map.write().unwrap().insert(s.id.clone(), meta.clone());
        meta
    }

    pub fn remove(&self, id: &str) {
        self.map.write().unwrap().remove(id);
    }

    pub fn get(&self, id: &str) -> Option<Arc<AppMeta>> {
        self.map.read().unwrap().get(id).cloned()
    }

    pub fn list(&self) -> Vec<Arc<AppMeta>> {
        let mut list: Vec<_> = self.map.read().unwrap().values().cloned().collect();
        list.sort_by_key(|a| (!a.builtin, a.name.to_lowercase()));
        list
    }

    /// Whether another app (not `except`) already has this name, ignoring case.
    pub fn name_taken(&self, name: &str, except: Option<&str>) -> bool {
        let name = name.trim().to_lowercase();
        self.map.read().unwrap().values().any(|a| a.name.to_lowercase() == name && Some(a.id.as_str()) != except)
    }

    /// "Clock" -> "Clock Copy 1" (or the first free number). "Clock Copy 1" -> "Clock Copy 2".
    pub fn copy_name(&self, name: &str) -> String {
        let base = match name.rsplit_once(" Copy ") {
            Some((b, n)) if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() => b,
            _ => name,
        };
        (1..).map(|n| format!("{base} Copy {n}")).find(|c| !self.name_taken(c, None)).unwrap()
    }
}

/// What `render` is told about the space it draws into.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Slot {
    pub w: i32,
    pub h: i32,
    pub size: &'static str,
    /// 2 or 4: how many grays the screen has.
    pub grays: u8,
}

const MEMORY_LIMIT: usize = 64 << 20;
const FETCH_TIME: Duration = Duration::from_secs(60);
const RENDER_TIME: Duration = Duration::from_secs(2);
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const HTTP_MAX_BODY: u64 = 16 << 20;

/// The `ui` constructors: each returns a plain table tagged with its kind.
const PRELUDE: &str = r#"
ui = {}
for _, kind in ipairs { "column", "row", "box", "layers", "first", "text", "image", "rule", "spacer" } do
  ui[kind] = function(t)
    if type(t) ~= "table" then t = { t } end
    t.kind = kind
    return t
  end
end
"#;

/// A sandboxed Lua state for one app (`label` names it in the log). `network`: whether `http`
/// is available (fetch only).
/// How a call runs, beyond the app and its config: for the developer API.
#[derive(Debug, Clone, Default)]
pub struct RunOpts {
    /// The time `time.now()`, `os.time()` and `os.date()` give, instead of the clock's.
    pub now: Option<i64>,
    /// Where `log()` and `print()` also write, line by line.
    pub log: Option<Arc<std::sync::Mutex<Vec<String>>>>,
}

impl RunOpts {
    fn now(&self) -> i64 {
        self.now.unwrap_or_else(|| chrono::Utc::now().timestamp())
    }
}

fn new_lua(label: &str, data_dir: &Path, network: bool, time: Duration, opts: &RunOpts) -> mlua::Result<Lua> {
    let lua = Lua::new_with(StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8 | StdLib::OS, LuaOptions::default())?;
    lua.set_memory_limit(MEMORY_LIMIT)?;
    let start = Instant::now();
    let deadline = start + time;
    lua.set_hook(HookTriggers::new().every_nth_instruction(10_000), move |_, _| {
        if start.elapsed() > time {
            Err(mlua::Error::runtime(format!("took longer than {} s", time.as_secs())))
        } else {
            Ok(VmState::Continue)
        }
    })?;

    let g = lua.globals();
    // os: only the clock and dates.
    let os: Table = g.get("os")?;
    for name in ["execute", "exit", "remove", "rename", "tmpname", "getenv", "setlocale"] {
        os.set(name, Value::Nil)?;
    }

    let app = label.to_string();
    let sink = opts.log.clone();
    let log = lua.create_function(move |_, args: mlua::Variadic<Value>| {
        let parts: Vec<String> = args.iter().map(|v| v.to_string().unwrap_or_else(|_| format!("{v:?}"))).collect();
        let line = parts.join(" ");
        tracing::info!(app = app.as_str(), "{line}");
        if let Some(sink) = &sink
            && let Ok(mut lines) = sink.lock()
            && lines.len() < 500
        {
            lines.push(line.chars().take(2000).collect());
        }
        Ok(())
    })?;
    g.set("print", log.clone())?;
    g.set("log", log)?;

    let json = lua.create_table()?;
    json.set(
        "decode",
        lua.create_function(|lua, s: mlua::LuaString| {
            let v: serde_json::Value = serde_json::from_slice(&s.as_bytes()).map_err(mlua::Error::external)?;
            to_lua(lua, &v)
        })?,
    )?;
    json.set(
        "encode",
        lua.create_function(|lua, v: Value| {
            let v: serde_json::Value = lua.from_value(v)?;
            Ok(v.to_string())
        })?,
    )?;
    g.set("json", json)?;

    let blobs = data_dir.join("blobs");
    g.set(
        "blob",
        lua.create_function(move |_, bytes: mlua::LuaString| {
            let bytes = bytes.as_bytes();
            let hash = hex(&Sha256::digest(&bytes));
            let path = blobs.join(&hash);
            if !path.exists() {
                std::fs::create_dir_all(&blobs).map_err(mlua::Error::external)?;
                std::fs::write(&path, &bytes).map_err(mlua::Error::external)?;
            }
            Ok(format!("blob:{hash}"))
        })?,
    )?;

    if network {
        g.set("http", http_table(&lua, deadline)?)?;
        // Waiting, for rate limits: within what is left of the time limit.
        g.set(
            "sleep",
            lua.create_function(move |_, secs: f64| {
                let left = deadline.saturating_duration_since(Instant::now()).as_secs_f64();
                if !(0.0..=left - 0.5).contains(&secs) {
                    return Err(mlua::Error::runtime(format!("sleep({secs}): only {left:.1} s of the time limit are left")));
                }
                std::thread::sleep(Duration::from_secs_f64(secs));
                Ok(())
            })?,
        )?;
    }
    lua.load(PRELUDE).set_name("=prelude").exec()?;
    crate::luatime::install(&lua)?;
    if let Some(now) = opts.now {
        // A fixed clock: every way an app can read the time gives `now`.
        lua.load(
            r#"local now = ...
            local os_time, os_date, relative = os.time, os.date, time.relative
            time.now = function() return now end
            time.relative = function(t, from) return relative(t, from or now) end
            os.time = function(t) if t == nil then return now end return os_time(t) end
            os.date = function(fmt, t) return os_date(fmt, t or now) end"#,
        )
        .set_name("=clock")
        .call::<()>(now)?;
    }
    Ok(lua)
}

fn http_table(lua: &Lua, deadline: Instant) -> mlua::Result<Table> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .http_status_as_error(false)
        .user_agent(concat!("tephra/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let t = lua.create_table()?;

    fn respond(lua: &Lua, mut r: ureq::http::Response<ureq::Body>) -> mlua::Result<Table> {
        let status = r.status().as_u16();
        let out = lua.create_table()?;
        let hs = lua.create_table()?;
        for (k, v) in r.headers() {
            if let Ok(v) = v.to_str() {
                hs.set(k.as_str().to_ascii_lowercase(), v)?;
            }
        }
        let body = r.body_mut().with_config().limit(HTTP_MAX_BODY).read_to_vec().map_err(mlua::Error::external)?;
        out.set("status", status)?;
        out.set("ok", (200..300).contains(&status))?;
        out.set("headers", hs)?;
        out.set("body", lua.create_string(&body)?)?;
        Ok(out)
    }
    // Rate limits: a 429 or 503 is tried again after its Retry-After (or a second), up to three
    // times, as long as the wait fits in what is left of the time limit.
    fn with_retries(deadline: Instant, send: impl Fn() -> Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> mlua::Result<ureq::http::Response<ureq::Body>> {
        let mut tries = 0;
        loop {
            let r = send().map_err(mlua::Error::external)?;
            let status = r.status().as_u16();
            if !(status == 429 || status == 503) || tries == 3 {
                return Ok(r);
            }
            let after = r.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| {
                v.trim().parse::<f64>().ok().or_else(|| {
                    chrono::DateTime::parse_from_rfc2822(v.trim()).ok().map(|t| (t.timestamp() - chrono::Utc::now().timestamp()) as f64)
                })
            });
            let wait = after.unwrap_or(1.0).max(0.1);
            let left = deadline.saturating_duration_since(Instant::now()).as_secs_f64();
            if wait > 30.0 || wait > left - 2.0 {
                return Ok(r);
            }
            std::thread::sleep(Duration::from_secs_f64(wait));
            tries += 1;
        }
    }
    fn headers(opts: &Option<Table>) -> mlua::Result<Vec<(String, String)>> {
        let Some(h) = opts.as_ref().map(|o| o.get::<Option<Table>>("headers")).transpose()?.flatten() else {
            return Ok(Vec::new());
        };
        h.pairs::<String, String>().collect()
    }

    let a = agent.clone();
    t.set(
        "get",
        lua.create_function(move |lua, (url, opts): (String, Option<Table>)| {
            let hs = headers(&opts)?;
            let r = with_retries(deadline, || {
                let mut req = a.get(&url);
                for (k, v) in &hs {
                    req = req.header(k, v);
                }
                req.call()
            })?;
            respond(lua, r)
        })?,
    )?;
    let a = agent;
    t.set(
        "post",
        lua.create_function(move |lua, (url, body, opts): (String, mlua::LuaString, Option<Table>)| {
            let hs = headers(&opts)?;
            let body = body.as_bytes().to_vec();
            let r = with_retries(deadline, || {
                let mut req = a.post(&url);
                for (k, v) in &hs {
                    req = req.header(k, v);
                }
                req.send(&body[..])
            })?;
            respond(lua, r)
        })?,
    )?;
    t.set(
        "urlencode",
        lua.create_function(|_, s: String| {
            Ok(s.bytes()
                .map(|b| match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
                    _ => format!("%{b:02X}"),
                })
                .collect::<String>())
        })?,
    )?;
    Ok(t)
}

fn to_lua(lua: &Lua, v: &serde_json::Value) -> mlua::Result<Value> {
    // JSON null becomes nil, so `if x.field then` works as apps expect.
    lua.to_value_with(v, mlua::serde::SerializeOptions::new().serialize_none_to_null(false).serialize_unit_to_null(false))
}


/// Lua for a table key: bare when it can be, `["..."]` otherwise.
pub fn lua_key(k: &str) -> String {
    const KEYWORDS: &[&str] = &[
        "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in", "local", "nil", "not",
        "or", "repeat", "return", "then", "true", "until", "while",
    ];
    let ident = k.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !KEYWORDS.contains(&k);
    if ident { k.to_string() } else { format!("[{}]", lua_string(k)) }
}

fn lua_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out += "\\\\",
            '"' => out += "\\\"",
            '\n' => out += "\\n",
            '\r' => out += "\\r",
            '\t' => out += "\\t",
            c if (c as u32) < 0x20 => out += &format!("\\{}", c as u32),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A JSON value written as a Lua expression.
pub fn lua_literal(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "nil".into(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => lua_string(s),
        serde_json::Value::Array(a) => format!("{{ {} }}", a.iter().map(lua_literal).collect::<Vec<_>>().join(", ")),
        serde_json::Value::Object(o) => {
            format!("{{ {} }}", o.iter().map(|(k, v)| format!("{} = {}", lua_key(k), lua_literal(v))).collect::<Vec<_>>().join(", "))
        }
    }
}

fn read_meta(meta: &mut AppMeta) {
    let result = (|| -> mlua::Result<()> {
        let lua = new_lua(&meta.name, Path::new("."), false, RENDER_TIME, &RunOpts::default())?;
        lua.globals().set("config", lua.create_table()?)?;
        let t = load_app(&lua, meta)?;
        if meta.builtin {
            if let Some(n) = t.get::<Option<String>>("name")? {
                meta.name = n;
            }
            meta.description = t.get::<Option<String>>("description")?.unwrap_or_default();
        }
        meta.refresh_s = match t.get::<Value>("refresh")? {
            Value::Nil => 900,
            Value::Integer(n) => n.max(60) as u64,
            Value::Number(n) => (n as u64).max(60),
            v => parse_duration(&v.to_string()?).ok_or_else(|| mlua::Error::runtime("refresh: use seconds or a duration like \"15m\""))?,
        };
        if let Some(s) = t.get::<Option<Value>>("settings")? {
            meta.settings = lua.from_value(s)?;
        }
        meta.has_fetch = matches!(t.get::<Value>("fetch")?, Value::Function(_));
        if !matches!(t.get::<Value>("render")?, Value::Function(_)) {
            return Err(mlua::Error::runtime("the code must return a table with a render function"));
        }
        Ok(())
    })();
    if let Err(e) = result {
        meta.error = Some(short_error(&e));
    }
}

fn builtin_meta(dir: &Path) -> AppMeta {
    let id = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let read = |f: &str| std::fs::read_to_string(dir.join(f));
    let validate = read("validate.lua").ok();
    let mut meta = AppMeta {
        id: id.clone(),
        name: id,
        description: String::new(),
        builtin: true,
        code: String::new(),
        validate_enabled: validate.is_some(),
        prototype: false,
        validate_code: validate.unwrap_or_default(),
        refresh_s: 900,
        settings: Vec::new(),
        has_fetch: false,
        error: None,
        dir: Some(dir.to_path_buf()),
    };
    match read("app.lua") {
        Ok(code) => {
            meta.code = code;
            read_meta(&mut meta);
        }
        Err(e) => meta.error = Some(format!("reading app.lua: {e}")),
    }
    meta
}

pub fn stored_meta(s: &StoredApp) -> AppMeta {
    let mut meta = AppMeta {
        id: s.id.clone(),
        name: s.name.clone(),
        description: s.description.clone(),
        builtin: false,
        code: s.code.clone(),
        validate_code: s.validate_code.clone(),
        validate_enabled: s.validate_enabled,
        prototype: s.prototype,
        refresh_s: 900,
        settings: Vec::new(),
        has_fetch: false,
        error: None,
        dir: None,
    };
    read_meta(&mut meta);
    meta
}

fn load_app(lua: &Lua, app: &AppMeta) -> mlua::Result<Table> {
    match lua.load(&app.code).set_name("=app").eval::<Value>()? {
        Value::Table(t) => Ok(t),
        v => Err(mlua::Error::runtime(format!("the code must return a table, not {}", v.type_name()))),
    }
}

/// "90s", "15m", "1h", "1d" -> seconds (at least 60).
fn parse_duration(s: &str) -> Option<u64> {
    let s = s.trim();
    let (n, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit())?);
    let n: u64 = n.parse().ok()?;
    let mult = match unit.trim() {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => return None,
    };
    Some((n * mult).max(60))
}

/// A Lua failure, with where it happened when Lua said.
#[derive(Debug, Clone, Serialize)]
pub struct LuaFailure {
    /// The message without its position, e.g. "attempt to index a nil value (field 'x')".
    pub message: String,
    /// Which code: `app`, `config` or `config check`.
    pub chunk: Option<String>,
    pub line: Option<u32>,
    /// Lua's stack traceback, when there is one.
    pub traceback: Option<String>,
}

impl LuaFailure {
    fn from_error(e: &mlua::Error) -> Self {
        let full = e.to_string();
        let traceback = full.split_once("\nstack traceback:").map(|(_, t)| format!("stack traceback:{t}"));
        Self { traceback, ..Self::parse(&short_error(e)) }
    }

    /// Splits a message like "app:12: attempt to call a nil value" into its parts.
    pub fn parse(short: &str) -> Self {
        let short = short.to_string();
        let traceback = None;
        // "app:12: message" (chunk names are ours: app, config, config check).
        for chunk in ["config check", "config", "app"] {
            if let Some(rest) = short.strip_prefix(chunk).and_then(|r| r.strip_prefix(':'))
                && let Some((line, msg)) = rest.split_once(": ")
                && let Ok(line) = line.parse()
            {
                return Self { message: msg.to_string(), chunk: Some(chunk.into()), line: Some(line), traceback };
            }
        }
        Self { message: short, chunk: None, line: None, traceback }
    }

    /// One line, with where it happened: "config:1: unexpected symbol".
    pub fn short(&self) -> String {
        match (&self.chunk, self.line) {
            (Some(c), Some(l)) => format!("{c}:{l}: {}", self.message),
            _ => self.message.clone(),
        }
    }

    fn plain(message: &str) -> Self {
        Self { message: message.to_string(), chunk: None, line: None, traceback: None }
    }
}

pub fn short_error(e: &mlua::Error) -> String {
    // The message without the Rust-side traceback; Lua's own position is in the first line.
    let s = e.to_string();
    let mut s = s.split("\nstack traceback:").next().unwrap_or(&s).trim();
    // mlua prefixes each layer it passes through; once is noise enough.
    while let Some(rest) = s.strip_prefix("runtime error: ").or_else(|| s.strip_prefix("syntax error: ")) {
        s = rest;
    }
    s.chars().take(600).collect()
}


/// Runs an instance's config in `lua`, fills in the app's defaults and runs the app's config
/// check. The result is also the global `config`, so the app's code can read it anywhere.
fn prepare_config(lua: &Lua, app: &AppMeta, config_src: &str) -> mlua::Result<Table> {
    let config = lua.create_table()?;
    // Plain assignments (`units = "metric"`) land in the table; reads fall back to the globals,
    // so the config can use string, math, os.date and so on.
    let env = lua.create_table()?;
    env.set("__index", lua.globals())?;
    config.set_metatable(Some(env))?;
    let returned: Value = lua.load(config_src).set_name("=config").set_environment(config.clone()).eval()?;
    config.set_metatable(None)?;
    // A config may also `return { ... }`.
    if let Value::Table(t) = returned {
        for pair in t.pairs::<Value, Value>() {
            let (k, v) = pair?;
            config.raw_set(k, v)?;
        }
    }
    for f in &app.settings {
        if config.raw_get::<Value>(f.key.as_str())?.is_nil() && !f.default.is_null() {
            config.raw_set(f.key.as_str(), to_lua(lua, &f.default)?)?;
        }
    }
    // Typed settings: a value that can't be read is the config's mistake, named as such.
    let time: Table = lua.globals().get("time")?;
    for f in &app.settings {
        let v: Value = config.raw_get(f.key.as_str())?;
        if v.is_nil() {
            continue;
        }
        let key = f.key.as_str();
        let problem = match f.kind.as_str() {
            "duration" => time.get::<mlua::Function>("duration")?.call::<i64>(v).err().map(|e| short_error(&e)),
            "zone" => time.get::<mlua::Function>("format")?.call::<String>((0, "%H", v)).err().map(|e| short_error(&e)),
            "time" => {
                let s = v.to_string().unwrap_or_default();
                let ok = s.split_once(':').and_then(|(h, m)| Some((h.parse::<u32>().ok()?, m.parse::<u32>().ok()?))).is_some_and(|(h, m)| h < 24 && m < 60 && s.len() <= 5);
                (!ok).then(|| format!("{s:?} is not a time of day like \"16:04\""))
            }
            _ => None,
        };
        if let Some(p) = problem {
            return Err(mlua::Error::runtime(format!("config: {key}: {p}")));
        }
    }
    lua.globals().set("config", config.clone())?;
    if app.validate_enabled && !app.validate_code.trim().is_empty() {
        lua.load(&app.validate_code).set_name("=config check").exec()?;
    }
    Ok(config)
}

/// Whether an instance's config runs and passes the app's check.
pub fn check_config(app: &AppMeta, config_src: &str) -> Result<(), String> {
    (|| -> mlua::Result<()> {
        let lua = new_lua(&app.name, Path::new("."), false, RENDER_TIME, &RunOpts::default())?;
        prepare_config(&lua, app, config_src).map(|_| ())
    })()
    .map_err(|e| short_error(&e))
}

/// What `fetch` returned: the data, and when it asked to run next (seconds since 1970).
#[derive(Debug, Clone)]
pub struct Fetched {
    pub data: serde_json::Value,
    pub next: Option<i64>,
}

/// Runs the app's `fetch(config, previous)`: what it returns is cached and handed to `render`.
pub fn fetch(app: &AppMeta, config_src: &str, previous: &serde_json::Value, data_dir: &Path) -> Result<Fetched, String> {
    fetch_with(app, config_src, previous, data_dir, &RunOpts::default()).map_err(|f| f.short())
}

/// `fetch`, with run options, and the failure's position and traceback.
pub fn fetch_with(app: &AppMeta, config_src: &str, previous: &serde_json::Value, data_dir: &Path, opts: &RunOpts) -> Result<Fetched, LuaFailure> {
    if let Some(e) = &app.error {
        return Err(LuaFailure::plain(e));
    }
    (|| -> mlua::Result<Fetched> {
        let lua = new_lua(&app.name, data_dir, true, FETCH_TIME, opts)?;
        let config = prepare_config(&lua, app, config_src)?;
        let t = load_app(&lua, app)?;
        let f: mlua::Function = t.get("fetch")?;
        let out: mlua::MultiValue = f.call((config, to_lua(&lua, previous)?))?;
        let mut out = out.into_iter();
        let data = lua.from_value(out.next().unwrap_or(Value::Nil))?;
        let next = match out.next() {
            Some(Value::Table(o)) => {
                let after = match o.get::<Value>("refresh")? {
                    Value::Nil => None,
                    Value::Integer(n) => Some(n.max(60)),
                    Value::Number(n) => Some((n as i64).max(60)),
                    v => Some(parse_duration(&v.to_string()?).ok_or_else(|| mlua::Error::runtime("fetch's refresh: seconds or a duration like \"5m\""))? as i64),
                };
                match (o.get::<Option<i64>>("refresh_at")?, after) {
                    (Some(at), _) => Some(at.max(opts.now() + 60)),
                    (None, Some(after)) => Some(opts.now() + after),
                    (None, None) => None,
                }
            }
            Some(Value::Nil) | None => None,
            Some(v) => return Err(mlua::Error::runtime(format!("fetch's second result: a table like {{ refresh = \"5m\" }}, not {}", v.type_name()))),
        };
        Ok(Fetched { data, next })
    })()
    .map_err(|e| LuaFailure::from_error(&e))
}

/// What `render` drew, and when it asked to be drawn again (seconds since 1970).
#[derive(Debug, Clone)]
pub struct Rendered {
    pub node: Node,
    pub redraw_at: Option<i64>,
}

/// Runs the app's `render(data, slot, config)` and turns the tree it returns into nodes.
pub fn render(app: &AppMeta, config_src: &str, data: &serde_json::Value, slot: Slot, data_dir: &Path) -> Result<Node, String> {
    render_with(app, config_src, data, slot, data_dir, &RunOpts::default()).map(|r| r.node).map_err(|f| f.short())
}

/// `render`, with run options, when to draw again, and the failure's position and traceback.
pub fn render_with(app: &AppMeta, config_src: &str, data: &serde_json::Value, slot: Slot, data_dir: &Path, opts: &RunOpts) -> Result<Rendered, LuaFailure> {
    if let Some(e) = &app.error {
        return Err(LuaFailure::plain(e));
    }
    (|| -> mlua::Result<Rendered> {
        let lua = new_lua(&app.name, data_dir, false, RENDER_TIME, opts)?;
        install_measure(&lua, app.dir.clone(), data_dir.to_path_buf())?;
        let config = prepare_config(&lua, app, config_src)?;
        let t = load_app(&lua, app)?;
        let f: mlua::Function = t.get("render")?;
        let out: mlua::MultiValue = f.call((to_lua(&lua, data)?, lua.to_value(&slot)?, config))?;
        let mut out = out.into_iter();
        let tree = out.next().unwrap_or(Value::Nil);
        let redraw_at = match out.next() {
            Some(Value::Table(o)) => match (o.get::<Option<i64>>("redraw_at")?, o.get::<Value>("redraw")?) {
                (Some(at), _) => Some(at),
                (None, Value::Nil) => None,
                (None, Value::Integer(n)) => Some(opts.now() + n),
                (None, Value::Number(n)) => Some(opts.now() + n as i64),
                (None, v) => Some(opts.now() + parse_duration(&v.to_string()?).ok_or_else(|| mlua::Error::runtime("render's redraw: seconds or a duration like \"5m\""))? as i64),
            },
            Some(Value::Nil) | None => None,
            Some(v) => return Err(mlua::Error::runtime(format!("render's second result: a table like {{ redraw_at = t }}, not {}", v.type_name()))),
        };
        let ctx = NodeCtx { app_dir: app.dir.as_deref(), data_dir };
        let node = node_from_lua(&tree, &ctx).map_err(mlua::Error::runtime)?;
        Ok(Rendered { node, redraw_at })
    })()
    .map_err(|e| LuaFailure::from_error(&e))
}

/// `ui.measure(node [, max_w [, max_h]])`: the size a node would take, before drawing it.
fn install_measure(lua: &Lua, app_dir: Option<PathBuf>, data_dir: PathBuf) -> mlua::Result<()> {
    let ui: Table = lua.globals().get("ui")?;
    let measure = lua.create_function(move |lua, (node, max_w, max_h): (Value, Option<f32>, Option<f32>)| {
        let ctx = NodeCtx { app_dir: app_dir.as_deref(), data_dir: &data_dir };
        // A bare string measures as body text.
        let node = match node {
            Value::String(s) => {
                let t = lua.create_table()?;
                t.set(1, s)?;
                t.set("kind", "text")?;
                Value::Table(t)
            }
            v => v,
        };
        let n = node_from_lua(&node, &ctx).map_err(mlua::Error::runtime)?;
        let (lw, lh) = (max_w.unwrap_or(f32::INFINITY), max_h.unwrap_or(f32::INFINITY));
        let (w, h, lines) = n.measured(lw, lh);
        // What it needs with no limit: its natural width, and its height at the width given.
        let needed_w = n.measured(f32::INFINITY, f32::INFINITY).0;
        let needed_h = n.measured(lw, f32::INFINITY).1;
        // Whether it fits: lay it out in the space given, exactly as a render would.
        let (cut, missing) = n.trial(lw.min(needed_w), lh.min(needed_h));

        let out = lua.create_table()?;
        out.set("w", w.ceil() as i64)?;
        out.set("h", h.ceil() as i64)?;
        out.set("needed_w", needed_w.ceil() as i64)?;
        out.set("needed_h", needed_h.ceil() as i64)?;
        out.set("fits", cut.iter().all(|c| *c == "missing_glyph"))?;
        out.set("cut", lua.create_sequence_from(cut)?)?;
        if !missing.is_empty() {
            out.set("missing", missing)?;
        }
        if let Node::Text { style, .. } = &n {
            out.set("lines", lines.unwrap_or(1))?;
            out.set("line_height", style.line_height() as i64)?;
        }
        Ok(out)
    })?;
    ui.set("measure", measure)
}

struct NodeCtx<'a> {
    /// Built-in apps can use image files from their directory; written apps only blobs.
    app_dir: Option<&'a Path>,
    data_dir: &'a Path,
}

fn node_from_lua(v: &Value, ctx: &NodeCtx) -> Result<Node, String> {
    let Value::Table(t) = v else {
        return Err(format!("render must return a ui node (a table), not {}", v.type_name()));
    };
    let get_s = |k: &str| t.get::<Option<String>>(k).ok().flatten();
    let get_f = |k: &str| t.get::<Option<f32>>(k).ok().flatten();
    let get_b = |k: &str| t.get::<Option<bool>>(k).ok().flatten();
    let shade = |k: &str| -> Result<Option<u8>, String> {
        match t.get::<Value>(k).map_err(|e| e.to_string())? {
            Value::Nil => Ok(None),
            Value::String(s) => match &*s.to_str().map_err(|e| e.to_string())? {
                "black" => Ok(Some(0)),
                "dark" => Ok(Some(85)),
                "light" => Ok(Some(170)),
                "white" => Ok(Some(255)),
                other => Err(format!("{k}: \"black\", \"dark\", \"light\" or \"white\", not {other:?}")),
            },
            v => Err(format!("{k}: \"black\", \"dark\", \"light\" or \"white\", not a {}", v.type_name())),
        }
    };
    let fill = match t.get::<Option<String>>("fill").ok().flatten().as_deref() {
        Some("hatch") => Some(Fill::Hatch),
        Some("crosshatch") => Some(Fill::Cross),
        Some("dots") => Some(Fill::Dots),
        _ => shade("fill").map_err(|e| e.replace("\"light\" or \"white\"", "\"light\", \"white\", \"hatch\", \"crosshatch\" or \"dots\""))?.map(Fill::Gray),
    };
    let common = Common { grow: get_f("grow").unwrap_or(0.0), width: get_f("width"), height: get_f("height"), fill, color: shade("color")? };
    let kind = get_s("kind").ok_or("a table without a kind: build nodes with ui.column, ui.text, ...")?;

    let children = || -> Result<Vec<Node>, String> {
        let mut items: Vec<(i64, Value)> = t
            .pairs::<Value, Value>()
            .flatten()
            .filter_map(|(k, v)| match k {
                Value::Integer(i) => Some((i, v)),
                _ => None,
            })
            .collect();
        items.sort_by_key(|(i, _)| *i);
        items.iter().filter(|(_, v)| !matches!(v, Value::Nil | Value::Boolean(false))).map(|(_, v)| node_from_lua(v, ctx)).collect()
    };

    Ok(match kind.as_str() {
        "column" | "row" | "box" => Node::Stack {
            dir: if kind == "row" || get_s("dir").as_deref() == Some("row") { Dir::Row } else { Dir::Column },
            children: children()?,
            gap: get_f("gap").unwrap_or(if kind == "row" { 12.0 } else { 6.0 }),
            align: match get_s("align").as_deref() {
                Some("start") => Align::Start,
                Some("center") => Align::Center,
                Some("end") => Align::End,
                Some("baseline") => Align::Baseline,
                _ => Align::Stretch,
            },
            justify: match get_s("justify").as_deref() {
                Some("center") => Justify::Center,
                Some("end") => Justify::End,
                Some("between") => Justify::Between,
                _ => Justify::Start,
            },
            padding: get_f("padding").unwrap_or(if kind == "box" { 10.0 } else { 0.0 }),
            border: match t.get::<Value>("border").map_err(|e| e.to_string())? {
                Value::Nil => Edges::uniform(if kind == "box" { 2.0 } else { 0.0 }),
                Value::Integer(n) => Edges::uniform(n as f32),
                Value::Number(n) => Edges::uniform(n as f32),
                Value::Table(b) => {
                    let side = |k: &str| b.get::<Option<f32>>(k).ok().flatten().unwrap_or(0.0);
                    Edges { top: side("top"), right: side("right"), bottom: side("bottom"), left: side("left") }
                }
                _ => return Err("border: a width, or { top = …, right = …, bottom = …, left = … }".into()),
            },
            invert: get_b("invert").unwrap_or(false),
            common,
        },
        "text" => {
            let text = match t.get::<Value>(1).map_err(|e| e.to_string())? {
                Value::Nil => String::new(),
                v => v.to_string().map_err(|e| e.to_string())?,
            };
            let mut style = match get_s("style") {
                Some(s) => TextStyle::named(&s).ok_or_else(|| format!("unknown text style {s:?}: use small, body, label, title, large or huge"))?,
                None => TextStyle::named("body").unwrap(),
            };
            if let Some(size) = get_f("size") {
                style.size = size.clamp(6.0, 400.0);
            }
            if let Some(b) = get_b("bold") {
                style.bold = b;
            }
            if let Some(f) = get_s("font") {
                style.face = Face::parse(&f).ok_or_else(|| format!("unknown font {f:?}: use {}", Face::NAMES))?;
            }
            style.italic = get_b("italic").unwrap_or(false);
            style.smooth = get_b("smooth");
            style.tracking = get_f("tracking").unwrap_or(0.0).clamp(-20.0, 100.0);
            style.tabular = match get_s("digits").as_deref() {
                None | Some("proportional") => false,
                Some("tabular") => true,
                Some(other) => return Err(format!("digits: \"tabular\" or \"proportional\", not {other:?}")),
            };
            let shrink_to = match get_s("fit").as_deref() {
                None => None,
                Some("shrink") => Some(get_f("min_size").unwrap_or(10.0).clamp(6.0, style.size)),
                Some(other) => return Err(format!("text fit {other:?}: the only choice is \"shrink\"")),
            };
            Node::Text {
                text,
                style,
                wrap: get_b("wrap").unwrap_or(false),
                align: match get_s("align").as_deref() {
                    Some("center") => TextAlign::Center,
                    Some("right") => TextAlign::Right,
                    _ => TextAlign::Left,
                },
                max_lines: t.get::<Option<usize>>("max_lines").ok().flatten(),
                shrink_to,
                common,
            }
        }
        "image" => {
            let src: String = t.get::<Option<String>>(1).ok().flatten().ok_or("ui.image needs a source: ui.image { \"file.png\" } or a blob")?;
            let bytes = load_image_bytes(&src, ctx)?;
            let crop = match t.get::<Value>("crop").map_err(|e| e.to_string())? {
                Value::Nil => None,
                Value::Table(c) => {
                    let n = |k: &str, i: i64| -> Result<u32, String> {
                        c.get::<Option<f64>>(k)
                            .ok()
                            .flatten()
                            .or_else(|| c.get::<Option<f64>>(i).ok().flatten())
                            .map(|v| v.max(0.0) as u32)
                            .ok_or_else(|| "crop: { x = …, y = …, w = …, h = … } in the image's pixels".to_string())
                    };
                    Some((n("x", 1)?, n("y", 2)?, n("w", 3)?, n("h", 4)?))
                }
                _ => return Err("crop: { x = …, y = …, w = …, h = … } in the image's pixels".into()),
            };
            let mut colors = Vec::new();
            if let Some(list) = t.get::<Option<Table>>("colors").map_err(|e| e.to_string())? {
                let bad = || "colors: a list of { \"#rrggbb\", gray }, the gray a number (0 black to 255 white) or \"black\", \"dark\", \"light\" or \"white\"".to_string();
                for pair in list.sequence_values::<Table>() {
                    let pair = pair.map_err(|_| bad())?;
                    let hexs: String = pair.get(1).map_err(|_| bad())?;
                    let h = hexs.trim_start_matches('#');
                    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Err(format!("colors: {hexs:?} is not a color like \"#04e9e7\""));
                    }
                    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
                    let gray = match pair.get::<Value>(2).map_err(|_| bad())? {
                        Value::Integer(v) => v.clamp(0, 255) as u8,
                        Value::Number(v) => v.clamp(0.0, 255.0).round() as u8,
                        Value::String(s) => match &*s.to_str().map_err(|_| bad())? {
                            "black" => 0,
                            "dark" => 85,
                            "light" => 170,
                            "white" => 255,
                            _ => return Err(bad()),
                        },
                        _ => return Err(bad()),
                    };
                    colors.push(([byte(0), byte(2), byte(4)], gray));
                }
            }
            let transparent = get_b("transparent").unwrap_or(false);
            let (img, mask) = decode_with_alpha(&bytes, crop, &colors, transparent).map_err(|e| format!("{src}: {e:#}"))?;
            Node::Image {
                img: Arc::new(img),
                mask: mask.map(Arc::new),
                fit: if get_s("fit").as_deref() == Some("cover") { Fit::Cover } else { Fit::Contain },
                dither: get_b("dither").unwrap_or(true),
                blur: get_f("blur").unwrap_or(0.0).clamp(0.0, 20.0),
                threshold: get_f("threshold").map(|v| v.clamp(0.0, 255.0) as u8),
                invert: get_b("invert").unwrap_or(false),
                common,
            }
        }
        "layers" => Node::Layers { children: children()?, common },
        "first" => Node::First { children: children()?, common },
        "rule" => Node::Rule {
            thickness: get_f("thickness").unwrap_or(2.0),
            dash: match t.get::<Value>("dash").map_err(|e| e.to_string())? {
                Value::Nil | Value::Boolean(false) => None,
                Value::Boolean(true) => Some((6.0, 4.0)),
                Value::Table(d) => Some((d.get::<f32>(1).unwrap_or(6.0), d.get::<f32>(2).unwrap_or(4.0))),
                _ => return Err("dash: true, or { on, off } in pixels".into()),
            },
            common,
        },
        "spacer" => Node::Spacer { common: Common { grow: if common.grow > 0.0 { common.grow } else { 1.0 }, ..common } },
        other => return Err(format!("unknown node kind {other:?}")),
    })
}

/// `blob:<hash>` from `blob()`, or a file inside the app's own directory.
fn load_image_bytes(src: &str, ctx: &NodeCtx) -> Result<Vec<u8>, String> {
    let path = if let Some(hash) = src.strip_prefix("blob:") {
        if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("not a blob: {src}"));
        }
        ctx.data_dir.join("blobs").join(hash)
    } else {
        let Some(app_dir) = ctx.app_dir else {
            return Err(format!("{src}: apps written in tephra can only show images stored with blob()"));
        };
        let rel = Path::new(src);
        if rel.is_absolute() || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return Err(format!("{src}: images must be files inside the app's directory"));
        }
        app_dir.join(rel)
    };
    std::fs::read(&path).map_err(|e| format!("{src}: {e}"))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(code: &str) -> AppMeta {
        stored_meta(&StoredApp {
            id: "t1".into(),
            name: "Demo".into(),
            description: String::new(),
            code: code.into(),
            validate_code: String::new(),
            validate_enabled: false,
            prototype: false,
        })
    }

    const DEMO: &str = r#"
        return {
          refresh = "15m",
          settings = { { key = "who", label = "Who", default = "world" } },
          render = function(data, slot, config)
            return ui.column { ui.text { "hello " .. config.who }, nil, ui.text { slot.size } }
          end,
        }"#;

    fn first_text(node: &Node) -> String {
        match node {
            Node::Text { text, .. } => text.clone(),
            Node::Stack { children, .. } => first_text(&children[0]),
            _ => String::new(),
        }
    }

    fn slot() -> Slot {
        Slot { w: 400, h: 300, size: "half", grays: 2 }
    }

    #[test]
    fn reads_meta_and_renders_a_tree() {
        let meta = stored(DEMO);
        assert_eq!(meta.error, None);
        assert_eq!((meta.name.as_str(), meta.refresh_s, meta.has_fetch), ("Demo", 900, false));
        let node = render(&meta, "", &serde_json::Value::Null, slot(), Path::new(".")).unwrap();
        let Node::Stack { children, .. } = &node else { panic!() };
        assert_eq!(children.len(), 2, "the nil hole is skipped");
        assert_eq!(first_text(&node), "hello world", "the default fills in");
    }

    #[test]
    fn fetch_gets_its_previous_result_and_may_choose_when_it_runs_next() {
        let meta = stored(r#"return {
            fetch = function(config, previous)
              return { n = (previous and previous.n or 0) + 1 }, { refresh = "5m" }
            end,
            render = function() return ui.text "x" end }"#);
        let opts = RunOpts { now: Some(1_000_000), log: None };
        let f = fetch_with(&meta, "", &serde_json::json!({ "n": 41 }), Path::new("."), &opts).unwrap();
        assert_eq!(f.data["n"], 42);
        assert_eq!(f.next, Some(1_000_300));
        let f = fetch_with(&meta, "", &serde_json::Value::Null, Path::new("."), &opts).unwrap();
        assert_eq!(f.data["n"], 1, "no previous result the first time");
    }

    #[test]
    fn render_sees_the_given_clock_logs_and_may_ask_to_be_drawn_again() {
        let meta = stored(r#"return { render = function()
            log("drawn at", time.now())
            return ui.text { time.now() .. " " .. os.time() .. " " .. os.date("!%H:%M") .. " " .. time.relative(time.now() + 7200) },
              { redraw_at = time.now() + 120 }
        end }"#);
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        let opts = RunOpts { now: Some(3600 * 13), log: Some(log.clone()) };
        let r = render_with(&meta, "", &serde_json::Value::Null, slot(), Path::new("."), &opts).unwrap();
        assert_eq!(first_text(&r.node), "46800 46800 13:00 in 2 h");
        assert_eq!(r.redraw_at, Some(46_920));
        assert_eq!(log.lock().unwrap().as_slice(), ["drawn at 46800"]);
    }

    #[test]
    fn http_waits_out_rate_limits_and_sleep_waits() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for (i, conn) in listener.incoming().take(2).enumerate() {
                let mut conn = conn.unwrap();
                let mut buf = [0u8; 2048];
                let _ = conn.read(&mut buf);
                let reply = if i == 0 {
                    "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                } else {
                    "HTTP/1.1 200 OK\r\nX-Answer: 42\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                };
                conn.write_all(reply.as_bytes()).unwrap();
            }
        });
        let meta = stored(&format!(r#"return {{
            fetch = function()
              local r = http.get("http://127.0.0.1:{port}/")
              sleep(0.1)
              return {{ status = r.status, body = r.body, answer = r.headers["x-answer"] }}
            end,
            render = function() return ui.text "x" end }}"#));
        let t = Instant::now();
        let f = fetch(&meta, "", &serde_json::Value::Null, Path::new(".")).unwrap();
        assert_eq!(f.data, serde_json::json!({ "status": 200, "body": "ok", "answer": "42" }));
        assert!(t.elapsed() >= Duration::from_millis(1100), "waited for Retry-After and the sleep");
    }

    #[test]
    fn typed_settings_are_checked() {
        let meta = stored(r#"return {
            settings = {
              { key = "every", type = "duration", default = "6h" },
              { key = "zone", type = "zone", default = "local" },
              { key = "at", type = "time", default = "16:04" },
            },
            render = function(_, _, config) return ui.text { tostring(time.duration(config.every)) } end }"#);
        let ok = render(&meta, "", &serde_json::Value::Null, slot(), Path::new(".")).unwrap();
        assert_eq!(first_text(&ok), "21600");
        for (config, key) in [(r#"every = "6x""#, "every"), (r#"zone = "Mars/Olympus""#, "zone"), (r#"at = "25:00""#, "at")] {
            let err = render(&meta, config, &serde_json::Value::Null, slot(), Path::new(".")).unwrap_err();
            assert!(err.contains(&format!("config: {key}:")), "{config}: {err}");
        }
        assert_eq!(crate::luatime::parse_duration("1h30m"), Ok(5400));
    }

    #[test]
    fn letter_spacing_and_tabular_digits() {
        let meta = stored(r#"return { render = function()
            local w = function(t) return ui.measure(ui.text(t)).w end
            return ui.text { table.concat({
              tostring(w { "1111", digits = "tabular", font = "overpass" } == w { "0000", digits = "tabular", font = "overpass" }),
              tostring(w { "1111", font = "overpass" } < w { "0000", font = "overpass" }),
              w { "RACE", tracking = 4 } - w { "RACE" },
            }, " ") }
        end }"#);
        let node = render(&meta, "", &serde_json::Value::Null, slot(), Path::new(".")).unwrap();
        assert_eq!(first_text(&node), "true true 12");
    }

    #[test]
    fn the_config_is_lua_run_first() {
        let meta = stored(DEMO);
        let node = render(&meta, r#"who = ("lua"):upper()"#, &serde_json::Value::Null, slot(), Path::new(".")).unwrap();
        assert_eq!(first_text(&node), "hello LUA");
        let node = render(&meta, r#"return { who = "table" }"#, &serde_json::Value::Null, slot(), Path::new(".")).unwrap();
        assert_eq!(first_text(&node), "hello table");
        let err = render(&meta, "who = ", &serde_json::Value::Null, slot(), Path::new(".")).unwrap_err();
        assert!(err.contains("config"), "{err}");
    }

    #[test]
    fn the_config_check_runs_before_the_app() {
        let mut meta = stored(DEMO);
        meta.validate_enabled = true;
        meta.validate_code = r#"assert(#config.who <= 5, "who: at most 5 letters")"#.into();
        assert_eq!(check_config(&meta, r#"who = "Ann""#), Ok(()));
        let err = check_config(&meta, r#"who = "Bartholomew""#).unwrap_err();
        assert!(err.contains("at most 5 letters"), "{err}");
        // Switched off, the check doesn't run.
        meta.validate_enabled = false;
        assert_eq!(check_config(&meta, r#"who = "Bartholomew""#), Ok(()));
    }

    #[test]
    fn the_template_round_trips() {
        let meta = stored(DEMO);
        let t = meta.config_template();
        assert!(t.contains("who = \"world\""), "{t}");
        assert_eq!(check_config(&meta, &t), Ok(()));
    }

    #[test]
    fn the_sandbox_has_no_files_or_processes() {
        let meta = stored(r#"return { render = function() return ui.text { tostring(io) .. tostring(os.execute) .. tostring(require) } end }"#);
        let node = render(&meta, "", &serde_json::Value::Null, slot(), Path::new(".")).unwrap();
        assert_eq!(first_text(&node), "nilnilnil");
    }

    #[test]
    fn a_runaway_render_is_stopped() {
        let meta = stored(r#"return { render = function() while true do end end }"#);
        let err = render(&meta, "", &serde_json::Value::Null, slot(), Path::new(".")).unwrap_err();
        assert!(err.contains("took longer"), "{err}");
    }

    #[test]
    fn broken_code_reports_why() {
        assert!(stored("return {").error.is_some());
        assert!(stored("return { name = 'x' }").error.unwrap().contains("render"));
    }

    #[test]
    fn written_apps_cannot_read_files() {
        let meta = stored(r#"return { render = function() return ui.image { "../../etc/passwd" } end }"#);
        let err = render(&meta, "", &serde_json::Value::Null, slot(), Path::new(".")).unwrap_err();
        assert!(err.contains("blob()"), "{err}");
    }

    #[test]
    fn copy_names_count_up() {
        let apps = Apps::load(Path::new("/nonexistent"), &[]);
        assert_eq!(apps.copy_name("Clock"), "Clock Copy 1");
        apps.put(&StoredApp {
            id: "a".into(),
            name: "Clock Copy 1".into(),
            description: String::new(),
            code: DEMO.into(),
            validate_code: String::new(),
            validate_enabled: false,
            prototype: false,
        });
        assert_eq!(apps.copy_name("Clock"), "Clock Copy 2");
        assert_eq!(apps.copy_name("Clock Copy 1"), "Clock Copy 2");
        assert!(apps.name_taken("clock copy 1", None));
        assert!(!apps.name_taken("Clock Copy 1", Some("a")));
    }

    #[test]
    fn lua_literals() {
        assert_eq!(lua_key("units"), "units");
        assert_eq!(lua_key("end"), "[\"end\"]");
        assert_eq!(lua_key("a-b"), "[\"a-b\"]");
        assert_eq!(lua_literal(&serde_json::json!({"a": [1, "x\n"]})), "{ a = { 1, \"x\\n\" } }");
    }

    #[test]
    fn json_round_trips_lists_and_nulls() {
        let meta = stored(r#"return { render = function()
            local v = json.decode('{"a":null,"b":[1,2]}')
            return ui.text { json.encode({ 1, 2 }) .. tostring(v.a) .. #v.b }
        end }"#);
        let node = render(&meta, "", &serde_json::Value::Null, slot(), Path::new(".")).unwrap();
        assert_eq!(first_text(&node), "[1,2]nil2");
    }

    #[test]
    fn failures_say_where() {
        let meta = stored("return { render = function(d, s, c)\n  return ui.text { c.missing.field }\nend }");
        let e = render_with(&meta, "", &serde_json::Value::Null, slot(), Path::new("."), &RunOpts::default()).unwrap_err();
        assert_eq!((e.chunk.as_deref(), e.line), (Some("app"), Some(2)), "{e:?}");
        assert!(e.message.contains("missing"), "{e:?}");
        assert!(e.traceback.is_some());
        let e = render_with(&stored(DEMO), "who = = 1", &serde_json::Value::Null, slot(), Path::new("."), &RunOpts::default()).unwrap_err();
        assert_eq!((e.chunk.as_deref(), e.line), (Some("config"), Some(1)), "{e:?}");
    }

    #[test]
    fn measure_matches_what_is_drawn() {
        let meta = stored(r#"return { render = function(d, slot)
            local one = ui.measure(ui.text { "Abu Dhabi Grand Prix", style = "large" })
            local wrapped = ui.measure(ui.text { "Abu Dhabi Grand Prix", style = "large", wrap = true }, 200)
            local mono = ui.measure(ui.text { "0000", font = "mono" })
            local mono1 = ui.measure(ui.text { "1111", font = "mono" })
            return ui.text { one.lines .. " " .. wrapped.lines .. " " .. tostring(wrapped.w <= 200)
              .. " " .. tostring(wrapped.h == wrapped.lines * wrapped.line_height) .. " " .. tostring(mono.w == mono1.w)
              .. " " .. ui.measure("x").lines }
        end }"#);
        let node = render(&meta, "", &serde_json::Value::Null, slot(), Path::new(".")).unwrap();
        assert_eq!(first_text(&node), "1 2 true true true 1");
    }

    #[test]
    fn measure_says_what_does_not_fit() {
        let meta = stored(r#"return { render = function()
            local name = ui.text { "Autódromo José Carlos Pace", style = "title" }
            local m = ui.measure(name, 150)
            local ok = ui.measure(name, 1000)
            local wrapped = ui.measure(ui.text { "Autódromo José Carlos Pace", wrap = true, max_lines = 1 }, 150)
            local star = ui.measure(ui.text { "★ \u{E000}" })
            return ui.text { table.concat({ m.w, tostring(m.needed_w > 150), tostring(m.fits), table.concat(m.cut, "+"),
              tostring(ok.fits), #ok.cut, tostring(wrapped.fits), table.concat(wrapped.cut, "+"),
              tostring(star.fits), star.missing }, " ") }
        end }"#);
        let node = render(&meta, "", &serde_json::Value::Null, slot(), Path::new(".")).unwrap();
        assert_eq!(first_text(&node), "149 true false too_wide true 0 false max_lines true \u{E000}");
    }

    #[test]
    fn layers_draw_transparent_images_over_each_other() {
        use crate::render::{Canvas, Rect};
        let dir = std::env::temp_dir().join(format!("tephra-layers-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A see-through image with a black square in its middle.
        let img = image::RgbaImage::from_fn(20, 20, |x, y| image::Rgba(if (5..15).contains(&x) && (5..15).contains(&y) { [0, 0, 0, 255] } else { [0, 0, 0, 0] }));
        img.save(dir.join("mark.png")).unwrap();
        let mut meta = stored(r#"return { render = function()
            return ui.layers {
              ui.box { fill = "dark", border = 0 },
              ui.image { "mark.png", transparent = true, fit = "cover", dither = false, blur = 1 },
            }
        end }"#);
        meta.dir = Some(dir.clone());
        let node = render(&meta, "", &serde_json::Value::Null, slot(), Path::new(".")).unwrap();
        let mut cv = Canvas::new(100, 100).with_grays(4);
        node.draw(&mut cv, Rect::new(0, 0, 100, 100), crate::render::BLACK, crate::render::layout::Dir::Column);
        assert_eq!(cv.get(50, 50), 0, "the mark, over the fill");
        assert_eq!(cv.get(5, 5), 85, "the fill shows through the clear parts");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fills_shade_with_four_grays_and_turn_text_white_on_dark() {
        use crate::render::{Canvas, Rect};
        let meta = stored(r#"return { render = function()
            return ui.column { gap = 0,
              ui.box { fill = "dark", border = 0, height = 40, ui.text { "on dark", smooth = false } },
              ui.box { fill = "light", border = 0, height = 40, ui.text { "light", color = "dark", smooth = false } },
            }
        end }"#);
        let node = render(&meta, "", &serde_json::Value::Null, slot(), Path::new(".")).unwrap();
        let draw = |grays: u8| {
            let mut cv = Canvas::new(400, 80).with_grays(grays);
            node.draw(&mut cv, Rect::new(0, 0, 400, 80), crate::render::BLACK, crate::render::layout::Dir::Column);
            let mut seen = std::collections::BTreeSet::new();
            for y in 0..80 {
                for x in 0..400 {
                    seen.insert((y / 40, cv.get(x, y)));
                }
            }
            seen
        };
        // 4 grays: the dark box has dark gray and white text; the light one light gray and dark text.
        let four = draw(4);
        assert!(four.contains(&(0, 85)) && four.contains(&(0, 255)) && !four.contains(&(0, 0)), "{four:?}");
        assert!(four.contains(&(1, 170)) && four.contains(&(1, 85)), "{four:?}");
        // 2 grays: dark becomes black (text still white), light becomes white.
        let two = draw(2);
        assert!(two.iter().all(|&(_, v)| v == 0 || v == 255));
        assert!(two.contains(&(0, 0)) && two.contains(&(0, 255)));

        let bad = stored(r#"return { render = function() return ui.box { fill = "blue" } end }"#);
        let err = render(&bad, "", &serde_json::Value::Null, slot(), Path::new(".")).unwrap_err();
        assert!(err.contains("fill: \"black\", \"dark\", \"light\", \"white\", \"hatch\""), "{err}");
    }

    #[test]
    fn text_options_are_checked() {
        let bad = stored(r#"return { render = function() return ui.text { "x", font = "comic" } end }"#);
        let err = render(&bad, "", &serde_json::Value::Null, slot(), Path::new(".")).unwrap_err();
        assert!(err.contains("sans, serif, mono or overpass"), "{err}");
        let ok = stored(r#"return { render = function() return ui.text { "x", font = "serif", italic = true, smooth = true, fit = "shrink", min_size = 12 } end }"#);
        assert!(render(&ok, "", &serde_json::Value::Null, slot(), Path::new(".")).is_ok());
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("15m"), Some(900));
        assert_eq!(parse_duration("1h"), Some(3600));
        assert_eq!(parse_duration("5s"), Some(60));
        assert_eq!(parse_duration("abc"), None);
    }
}
