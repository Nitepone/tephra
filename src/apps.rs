//! Lua apps: loading `apps/<id>/app.lua`, and running its `fetch` and `render` in a sandbox.
//!
//! Every call gets a fresh Lua state, so apps can't keep state between calls or interfere with
//! each other, and a crashed or runaway script costs nothing beyond its own call. Calls block;
//! run them with `spawn_blocking`.

use crate::render::font::TextStyle;
use crate::render::layout::{Align, Common, Dir, Fit, Justify, Node, TextAlign};
use crate::render::decode_gray;
use mlua::{HookTriggers, Lua, LuaOptions, LuaSerdeExt, StdLib, Table, Value, VmState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// One field of an app's settings form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingField {
    pub key: String,
    #[serde(default)]
    pub label: String,
    /// `string`, `text` (several lines), `number`, `bool` or `choice`.
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
    /// How often `fetch` runs; also the shortest refresh it asks of a device showing it.
    pub refresh_s: u64,
    pub settings: Vec<SettingField>,
    pub has_fetch: bool,
    /// Why the app could not be loaded. Such an app is listed but cannot run.
    pub error: Option<String>,
    #[serde(skip)]
    pub dir: PathBuf,
}

impl AppMeta {
    /// The stored settings with the app's defaults filled in.
    pub fn settings_with_defaults(&self, stored: &serde_json::Value) -> serde_json::Value {
        let mut out = serde_json::Map::new();
        for f in &self.settings {
            let v = stored.get(&f.key).filter(|v| !v.is_null()).unwrap_or(&f.default);
            out.insert(f.key.clone(), v.clone());
        }
        serde_json::Value::Object(out)
    }
}

/// The installed apps, as last loaded from disk.
pub struct Apps {
    dir: PathBuf,
    map: RwLock<BTreeMap<String, Arc<AppMeta>>>,
}

impl Apps {
    pub fn load(dir: &Path) -> Self {
        let apps = Self { dir: dir.to_path_buf(), map: RwLock::default() };
        apps.reload();
        apps
    }

    pub fn reload(&self) {
        let mut map = BTreeMap::new();
        let entries = std::fs::read_dir(&self.dir).map(|rd| rd.flatten().collect::<Vec<_>>()).unwrap_or_else(|e| {
            tracing::warn!("cannot read the apps directory {}: {e}", self.dir.display());
            Vec::new()
        });
        for e in entries {
            let dir = e.path();
            if dir.join("app.lua").is_file() {
                let meta = load_meta(&dir);
                if let Some(err) = &meta.error {
                    tracing::warn!(app = meta.id, "{err}");
                }
                map.insert(meta.id.clone(), Arc::new(meta));
            }
        }
        tracing::info!("{} apps loaded from {}", map.len(), self.dir.display());
        *self.map.write().unwrap() = map;
    }

    pub fn get(&self, id: &str) -> Option<Arc<AppMeta>> {
        self.map.read().unwrap().get(id).cloned()
    }

    pub fn list(&self) -> Vec<Arc<AppMeta>> {
        self.map.read().unwrap().values().cloned().collect()
    }
}

/// What `render` is told about the space it draws into.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Slot {
    pub w: i32,
    pub h: i32,
    pub size: &'static str,
}

const MEMORY_LIMIT: usize = 64 << 20;
const FETCH_TIME: Duration = Duration::from_secs(60);
const RENDER_TIME: Duration = Duration::from_secs(2);
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const HTTP_MAX_BODY: u64 = 16 << 20;

/// The `ui` constructors: each returns a plain table tagged with its kind.
const PRELUDE: &str = r#"
ui = {}
for _, kind in ipairs { "column", "row", "box", "text", "image", "rule", "spacer" } do
  ui[kind] = function(t)
    if type(t) ~= "table" then t = { t } end
    t.kind = kind
    return t
  end
end
"#;

/// A sandboxed Lua state for one app. `network`: whether `http` is available (fetch only).
fn new_lua(app_dir: &Path, data_dir: &Path, network: bool, time: Duration) -> mlua::Result<Lua> {
    let lua = Lua::new_with(StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8 | StdLib::OS, LuaOptions::default())?;
    lua.set_memory_limit(MEMORY_LIMIT)?;
    let start = Instant::now();
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

    let app = app_dir.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let log = lua.create_function(move |_, args: mlua::Variadic<Value>| {
        let parts: Vec<String> = args.iter().map(|v| v.to_string().unwrap_or_else(|_| format!("{v:?}"))).collect();
        tracing::info!(app = app.as_str(), "{}", parts.join(" "));
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
        g.set("http", http_table(&lua)?)?;
    }
    lua.load(PRELUDE).set_name("=prelude").exec()?;
    Ok(lua)
}

fn http_table(lua: &Lua) -> mlua::Result<Table> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .http_status_as_error(false)
        .user_agent(concat!("tephra/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let t = lua.create_table()?;

    fn respond(lua: &Lua, r: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> mlua::Result<Table> {
        let mut r = r.map_err(mlua::Error::external)?;
        let status = r.status().as_u16();
        let body = r.body_mut().with_config().limit(HTTP_MAX_BODY).read_to_vec().map_err(mlua::Error::external)?;
        let out = lua.create_table()?;
        out.set("status", status)?;
        out.set("ok", (200..300).contains(&status))?;
        out.set("body", lua.create_string(&body)?)?;
        Ok(out)
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
            let mut req = a.get(&url);
            for (k, v) in headers(&opts)? {
                req = req.header(k, v);
            }
            respond(lua, req.call())
        })?,
    )?;
    let a = agent;
    t.set(
        "post",
        lua.create_function(move |lua, (url, body, opts): (String, mlua::LuaString, Option<Table>)| {
            let mut req = a.post(&url);
            for (k, v) in headers(&opts)? {
                req = req.header(k, v);
            }
            respond(lua, req.send(&body.as_bytes()[..]))
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

fn load_app(lua: &Lua, dir: &Path) -> mlua::Result<Table> {
    let path = dir.join("app.lua");
    let src = std::fs::read(&path).map_err(mlua::Error::external)?;
    lua.load(src).set_name(format!("@{}/app.lua", dir.file_name().unwrap_or_default().to_string_lossy())).eval()
}

fn load_meta(dir: &Path) -> AppMeta {
    let id = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let mut meta = AppMeta {
        id: id.clone(),
        name: id.clone(),
        description: String::new(),
        refresh_s: 900,
        settings: Vec::new(),
        has_fetch: false,
        error: None,
        dir: dir.to_path_buf(),
    };
    let result = (|| -> mlua::Result<()> {
        let lua = new_lua(dir, Path::new("."), false, RENDER_TIME)?;
        let t = load_app(&lua, dir)?;
        if let Some(n) = t.get::<Option<String>>("name")? {
            meta.name = n;
        }
        meta.description = t.get::<Option<String>>("description")?.unwrap_or_default();
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
            return Err(mlua::Error::runtime("app.lua must return a table with a render function"));
        }
        Ok(())
    })();
    if let Err(e) = result {
        meta.error = Some(short_error(&e));
    }
    meta
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

fn short_error(e: &mlua::Error) -> String {
    // The message without the Rust-side traceback; Lua's own position is in the first line.
    let s = e.to_string();
    let s = s.split("\nstack traceback:").next().unwrap_or(&s).trim();
    s.chars().take(600).collect()
}

/// Runs the app's `fetch(settings)`: what it returns is cached and handed to `render`.
pub fn fetch(app: &AppMeta, settings: &serde_json::Value, data_dir: &Path) -> Result<serde_json::Value, String> {
    if let Some(e) = &app.error {
        return Err(e.clone());
    }
    (|| -> mlua::Result<serde_json::Value> {
        let lua = new_lua(&app.dir, data_dir, true, FETCH_TIME)?;
        let t = load_app(&lua, &app.dir)?;
        let f: mlua::Function = t.get("fetch")?;
        let out: Value = f.call(to_lua(&lua, settings)?)?;
        lua.from_value(out)
    })()
    .map_err(|e| short_error(&e))
}

/// Runs the app's `render(data, slot, settings)` and turns the tree it returns into nodes.
pub fn render(app: &AppMeta, settings: &serde_json::Value, data: &serde_json::Value, slot: Slot, data_dir: &Path) -> Result<Node, String> {
    if let Some(e) = &app.error {
        return Err(e.clone());
    }
    (|| -> mlua::Result<Node> {
        let lua = new_lua(&app.dir, data_dir, false, RENDER_TIME)?;
        let t = load_app(&lua, &app.dir)?;
        let f: mlua::Function = t.get("render")?;
        let tree: Value = f.call((to_lua(&lua, data)?, lua.to_value(&slot)?, to_lua(&lua, settings)?))?;
        let ctx = NodeCtx { app_dir: &app.dir, data_dir };
        node_from_lua(&tree, &ctx).map_err(mlua::Error::runtime)
    })()
    .map_err(|e| short_error(&e))
}

struct NodeCtx<'a> {
    app_dir: &'a Path,
    data_dir: &'a Path,
}

fn node_from_lua(v: &Value, ctx: &NodeCtx) -> Result<Node, String> {
    let Value::Table(t) = v else {
        return Err(format!("render must return a ui node (a table), not {}", v.type_name()));
    };
    let get_s = |k: &str| t.get::<Option<String>>(k).ok().flatten();
    let get_f = |k: &str| t.get::<Option<f32>>(k).ok().flatten();
    let get_b = |k: &str| t.get::<Option<bool>>(k).ok().flatten();
    let common = Common { grow: get_f("grow").unwrap_or(0.0), width: get_f("width"), height: get_f("height") };
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
                _ => Align::Stretch,
            },
            justify: match get_s("justify").as_deref() {
                Some("center") => Justify::Center,
                Some("end") => Justify::End,
                Some("between") => Justify::Between,
                _ => Justify::Start,
            },
            padding: get_f("padding").unwrap_or(if kind == "box" { 10.0 } else { 0.0 }),
            border: get_f("border").unwrap_or(if kind == "box" { 2.0 } else { 0.0 }),
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
                common,
            }
        }
        "image" => {
            let src: String = t.get::<Option<String>>(1).ok().flatten().ok_or("ui.image needs a source: ui.image { \"file.png\" } or a blob")?;
            let bytes = load_image_bytes(&src, ctx)?;
            let img = decode_gray(&bytes).map_err(|e| format!("{src}: {e:#}"))?;
            Node::Image {
                img: Arc::new(img),
                fit: if get_s("fit").as_deref() == Some("cover") { Fit::Cover } else { Fit::Contain },
                dither: get_b("dither").unwrap_or(true),
                common,
            }
        }
        "rule" => Node::Rule { thickness: get_f("thickness").unwrap_or(2.0), common },
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
        let rel = Path::new(src);
        if rel.is_absolute() || rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return Err(format!("{src}: images must be files inside the app's directory"));
        }
        ctx.app_dir.join(rel)
    };
    std::fs::read(&path).map_err(|e| format!("{src}: {e}"))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(src: &str) -> (tempdir::TempDir, AppMeta) {
        let d = tempdir::TempDir::new();
        let dir = d.0.join("demo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("app.lua"), src).unwrap();
        let meta = load_meta(&dir);
        (d, meta)
    }

    /// A throwaway directory, removed when dropped.
    mod tempdir {
        pub struct TempDir(pub std::path::PathBuf);
        impl TempDir {
            pub fn new() -> Self {
                let mut b = [0u8; 8];
                getrandom::fill(&mut b).unwrap();
                let p = std::env::temp_dir().join(format!("tephra-test-{}", super::hex(&b)));
                std::fs::create_dir_all(&p).unwrap();
                Self(p)
            }
        }
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn loads_meta_and_renders_a_tree() {
        let (d, meta) = app(r#"
            return {
              name = "Demo", refresh = "15m",
              settings = { { key = "who", label = "Who", default = "world" } },
              render = function(data, slot, settings)
                return ui.column { ui.text { "hello " .. settings.who, style = "title" }, nil, ui.text { slot.size } }
              end,
            }"#);
        assert_eq!(meta.error, None);
        assert_eq!((meta.name.as_str(), meta.refresh_s, meta.has_fetch), ("Demo", 900, false));
        let settings = meta.settings_with_defaults(&serde_json::json!({}));
        let node = render(&meta, &settings, &serde_json::Value::Null, Slot { w: 400, h: 300, size: "half" }, &d.0).unwrap();
        let Node::Stack { children, .. } = node else { panic!() };
        assert_eq!(children.len(), 2, "the nil hole is skipped");
        assert!(matches!(&children[0], Node::Text { text, .. } if text == "hello world"));
    }

    #[test]
    fn the_sandbox_has_no_files_or_processes() {
        let (d, meta) = app(r#"return { render = function() return ui.text { tostring(io) .. tostring(os.execute) .. tostring(require) } end }"#);
        let node = render(&meta, &serde_json::json!({}), &serde_json::Value::Null, Slot { w: 1, h: 1, size: "full" }, &d.0).unwrap();
        assert!(matches!(node, Node::Text { text, .. } if text == "nilnilnil"));
    }

    #[test]
    fn a_runaway_render_is_stopped() {
        let (d, meta) = app(r#"return { render = function() while true do end end }"#);
        let err = render(&meta, &serde_json::json!({}), &serde_json::Value::Null, Slot { w: 1, h: 1, size: "full" }, &d.0).unwrap_err();
        assert!(err.contains("took longer"), "{err}");
    }

    #[test]
    fn a_broken_app_reports_why() {
        let (_d, meta) = app("return {");
        assert!(meta.error.is_some());
        let (_d, meta) = app("return { name = 'x' }");
        assert!(meta.error.unwrap().contains("render"));
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("15m"), Some(900));
        assert_eq!(parse_duration("1h"), Some(3600));
        assert_eq!(parse_duration("5s"), Some(60));
        assert_eq!(parse_duration("abc"), None);
    }
}
