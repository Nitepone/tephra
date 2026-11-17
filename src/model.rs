//! The rows the server works with, and the queries that read and write them.

use crate::db::now_ms;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct Device {
    pub id: i64,
    /// None for a placeholder.
    pub mac: Option<String>,
    #[serde(skip)]
    pub api_key: Option<String>,
    pub friendly_id: String,
    pub name: String,
    pub model: String,
    pub width: u32,
    pub height: u32,
    pub firmware: String,
    pub battery_v: Option<f64>,
    pub rssi: Option<i32>,
    pub last_seen_ms: Option<i64>,
    pub created_ms: i64,
    pub refresh_min_s: u32,
    pub refresh_max_s: u32,
    pub button: String,
    /// 2: black and white, fast refresh. 4: also two grays, a full refresh whenever gray is shown.
    pub grays: u8,
    pub placeholder: bool,
}

const DEVICE_COLS: &str = "id, mac, api_key, friendly_id, name, model, width, height, firmware, battery_v, rssi, \
     last_seen_ms, created_ms, refresh_min_s, refresh_max_s, button, grays";

fn device_row(r: &Row) -> rusqlite::Result<Device> {
    let mac: Option<String> = r.get(1)?;
    Ok(Device {
        id: r.get(0)?,
        placeholder: mac.is_none(),
        mac,
        api_key: r.get(2)?,
        friendly_id: r.get(3)?,
        name: r.get(4)?,
        model: r.get(5)?,
        width: r.get(6)?,
        height: r.get(7)?,
        firmware: r.get(8)?,
        battery_v: r.get(9)?,
        rssi: r.get(10)?,
        last_seen_ms: r.get(11)?,
        created_ms: r.get(12)?,
        refresh_min_s: r.get(13)?,
        refresh_max_s: r.get(14)?,
        button: r.get(15)?,
        grays: r.get(16)?,
    })
}

impl Device {
    pub fn label(&self) -> String {
        if !self.name.is_empty() {
            self.name.clone()
        } else if !self.friendly_id.is_empty() {
            format!("TRMNL {}", self.friendly_id)
        } else {
            format!("Device {}", self.id)
        }
    }
}

pub fn devices(c: &Connection) -> rusqlite::Result<Vec<Device>> {
    let mut st = c.prepare(&format!("SELECT {DEVICE_COLS} FROM devices ORDER BY name = '', name, id"))?;
    st.query_map([], device_row)?.collect()
}

pub fn device(c: &Connection, id: i64) -> rusqlite::Result<Option<Device>> {
    c.query_row(&format!("SELECT {DEVICE_COLS} FROM devices WHERE id = ?"), [id], device_row).optional()
}

pub fn device_by_mac(c: &Connection, mac: &str) -> rusqlite::Result<Option<Device>> {
    c.query_row(&format!("SELECT {DEVICE_COLS} FROM devices WHERE mac = ?"), [mac], device_row).optional()
}

pub fn device_by_key(c: &Connection, key: &str) -> rusqlite::Result<Option<Device>> {
    c.query_row(&format!("SELECT {DEVICE_COLS} FROM devices WHERE api_key = ?"), [key], device_row).optional()
}

/// A random lowercase hex string of `bytes` bytes.
pub fn random_hex(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    getrandom::fill(&mut b).expect("system randomness");
    crate::apps::hex(&b)
}

/// Registers a device that called `/api/setup`; returns it with its new key.
pub fn register_device(c: &Connection, mac: &str, model: &str, width: u32, height: u32, firmware: &str) -> rusqlite::Result<Device> {
    let key = random_hex(16);
    let friendly = random_hex(3).to_uppercase();
    c.execute(
        "INSERT INTO devices (mac, api_key, friendly_id, model, width, height, firmware, created_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        params![mac, key, friendly, model, width, height, firmware, now_ms()],
    )?;
    let id = c.last_insert_rowid();
    Ok(device(c, id)?.expect("just inserted"))
}

/// Takes in a device that already has an API key from another server (LaraPaper, Terminus, ...).
/// The firmware only calls `/api/setup` when it has no key, so a moved device never would: it is
/// registered under the key it presents. A known MAC with a new key gets the new key.
pub fn adopt_device(c: &Connection, mac: &str, key: &str, model: &str, width: u32, height: u32, firmware: &str) -> rusqlite::Result<Device> {
    if let Some(d) = device_by_mac(c, mac)? {
        c.execute("UPDATE devices SET api_key = ? WHERE id = ?", params![key, d.id])?;
        return Ok(device(c, d.id)?.expect("exists"));
    }
    let friendly = random_hex(3).to_uppercase();
    c.execute(
        "INSERT INTO devices (mac, api_key, friendly_id, model, width, height, firmware, created_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        params![mac, key, friendly, model, width, height, firmware, now_ms()],
    )?;
    Ok(device(c, c.last_insert_rowid())?.expect("just inserted"))
}

pub fn add_placeholder(c: &Connection, name: &str, model: &str, width: u32, height: u32) -> rusqlite::Result<Device> {
    c.execute(
        "INSERT INTO devices (name, model, width, height, created_ms) VALUES (?, ?, ?, ?, ?)",
        params![name, model, width, height, now_ms()],
    )?;
    Ok(device(c, c.last_insert_rowid())?.expect("just inserted"))
}

/// What a device reports about itself with each request.
#[derive(Debug, Default)]
pub struct Telemetry {
    pub model: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub firmware: Option<String>,
    pub battery_v: Option<f64>,
    pub rssi: Option<i32>,
}

pub fn record_telemetry(c: &Connection, id: i64, t: &Telemetry) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE devices SET model = coalesce(?, model), width = coalesce(?, width), height = coalesce(?, height),
             firmware = coalesce(?, firmware), battery_v = coalesce(?, battery_v), rssi = coalesce(?, rssi),
             last_seen_ms = ? WHERE id = ?",
        params![t.model, t.width, t.height, t.firmware, t.battery_v, t.rssi, now_ms(), id],
    )?;
    Ok(())
}

// ---- screens ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotConfig {
    pub instance_id: Option<i64>,
    #[serde(default = "yes")]
    pub title_bar: bool,
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenConfig {
    pub preset: String,
    pub slots: Vec<SlotConfig>,
}

pub fn screen(c: &Connection, device_id: i64) -> rusqlite::Result<ScreenConfig> {
    let preset: String = c
        .query_row("SELECT preset FROM screens WHERE device_id = ?", [device_id], |r| r.get(0))
        .optional()?
        .unwrap_or_else(|| "full".into());
    let n = crate::screen::preset(&preset).slots.len();
    let mut slots: Vec<SlotConfig> = (0..n).map(|_| SlotConfig { instance_id: None, title_bar: true }).collect();
    let mut st = c.prepare("SELECT slot, instance_id, title_bar FROM slots WHERE device_id = ?")?;
    let rows = st.query_map([device_id], |r| Ok((r.get::<_, i64>(0)? as usize, r.get::<_, Option<i64>>(1)?, r.get::<_, bool>(2)?)))?;
    for row in rows {
        let (i, instance_id, title_bar) = row?;
        if let Some(s) = slots.get_mut(i) {
            *s = SlotConfig { instance_id, title_bar };
        }
    }
    Ok(ScreenConfig { preset: crate::screen::preset(&preset).id.into(), slots })
}

pub fn save_screen(c: &mut Connection, device_id: i64, s: &ScreenConfig) -> rusqlite::Result<()> {
    let tx = c.transaction()?;
    tx.execute(
        "INSERT INTO screens (device_id, preset, updated_ms) VALUES (?1, ?2, ?3)
         ON CONFLICT (device_id) DO UPDATE SET preset = ?2, updated_ms = ?3",
        params![device_id, s.preset, now_ms()],
    )?;
    tx.execute("DELETE FROM slots WHERE device_id = ?", [device_id])?;
    for (i, slot) in s.slots.iter().enumerate() {
        tx.execute(
            "INSERT INTO slots (device_id, slot, instance_id, title_bar) VALUES (?, ?, ?, ?)",
            params![device_id, i as i64, slot.instance_id, slot.title_bar],
        )?;
    }
    tx.commit()
}

// ---- app instances ----

#[derive(Debug, Clone, Serialize)]
pub struct Instance {
    pub id: i64,
    pub app: String,
    pub name: String,
    /// Lua, run before the app; what it assigns becomes the app's `config`.
    pub config: String,
    pub fetched_ms: Option<i64>,
    pub error: Option<String>,
    #[serde(skip)]
    pub data: serde_json::Value,
}

const INSTANCE_SELECT: &str = "SELECT i.id, i.app, i.name, i.config, d.fetched_ms, d.error, d.data
     FROM instances i LEFT JOIN app_data d ON d.instance_id = i.id";

fn instance_row(r: &Row) -> rusqlite::Result<Instance> {
    let data: Option<String> = r.get(6)?;
    Ok(Instance {
        id: r.get(0)?,
        app: r.get(1)?,
        name: r.get(2)?,
        config: r.get(3)?,
        fetched_ms: r.get(4)?,
        error: r.get(5)?,
        data: data.and_then(|d| serde_json::from_str(&d).ok()).unwrap_or_default(),
    })
}

pub fn instances(c: &Connection) -> rusqlite::Result<Vec<Instance>> {
    let mut st = c.prepare(&format!("{INSTANCE_SELECT} ORDER BY i.app, i.name, i.id"))?;
    st.query_map([], instance_row)?.collect()
}

pub fn instance(c: &Connection, id: i64) -> rusqlite::Result<Option<Instance>> {
    c.query_row(&format!("{INSTANCE_SELECT} WHERE i.id = ?"), [id], instance_row).optional()
}

pub fn add_instance(c: &Connection, app: &str, name: &str, config: &str) -> rusqlite::Result<i64> {
    c.execute("INSERT INTO instances (app, name, config, created_ms) VALUES (?, ?, ?, ?)", params![app, name, config, now_ms()])?;
    Ok(c.last_insert_rowid())
}

pub fn update_instance(c: &Connection, id: i64, name: &str, config: &str) -> rusqlite::Result<bool> {
    Ok(c.execute("UPDATE instances SET name = ?, config = ? WHERE id = ?", params![name, config, id])? > 0)
}

pub fn instance_count(c: &Connection, app: &str) -> rusqlite::Result<i64> {
    c.query_row("SELECT count(*) FROM instances WHERE app = ?", [app], |r| r.get(0))
}

// ---- apps written in the UI ----

/// An app written, forked or imported in the UI. Built-in apps are files instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredApp {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub code: String,
    #[serde(default)]
    pub validate_code: String,
    #[serde(default)]
    pub validate_enabled: bool,
    #[serde(default)]
    pub prototype: bool,
}

pub fn stored_apps(c: &Connection) -> rusqlite::Result<Vec<StoredApp>> {
    let mut st = c.prepare("SELECT id, name, description, code, validate_code, validate_enabled, prototype FROM apps ORDER BY name")?;
    st.query_map([], |r| {
        Ok(StoredApp {
            id: r.get(0)?,
            name: r.get(1)?,
            description: r.get(2)?,
            code: r.get(3)?,
            validate_code: r.get(4)?,
            validate_enabled: r.get(5)?,
            prototype: r.get(6)?,
        })
    })?
    .collect()
}

/// Inserts or replaces a written app.
pub fn save_app(c: &Connection, a: &StoredApp) -> rusqlite::Result<()> {
    let now = now_ms();
    c.execute(
        "INSERT INTO apps (id, name, description, code, validate_code, validate_enabled, prototype, created_ms, updated_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?8, ?7, ?7)
         ON CONFLICT (id) DO UPDATE SET name = ?2, description = ?3, code = ?4, validate_code = ?5,
             validate_enabled = ?6, prototype = ?8, updated_ms = ?7",
        params![a.id, a.name, a.description, a.code, a.validate_code, a.validate_enabled, now, a.prototype],
    )?;
    Ok(())
}

pub fn delete_app(c: &Connection, id: &str) -> rusqlite::Result<bool> {
    Ok(c.execute("DELETE FROM apps WHERE id = ?", [id])? > 0)
}

/// Stores a fetch result. A failed fetch keeps the previous data and records the error.
pub fn store_fetch(c: &Connection, id: i64, result: &Result<serde_json::Value, String>) -> rusqlite::Result<()> {
    match result {
        Ok(data) => c.execute(
            "INSERT INTO app_data (instance_id, fetched_ms, data, error) VALUES (?1, ?2, ?3, NULL)
             ON CONFLICT (instance_id) DO UPDATE SET fetched_ms = ?2, data = ?3, error = NULL",
            params![id, now_ms(), data.to_string()],
        )?,
        Err(e) => c.execute(
            "INSERT INTO app_data (instance_id, fetched_ms, data, error) VALUES (?1, ?2, NULL, ?3)
             ON CONFLICT (instance_id) DO UPDATE SET fetched_ms = ?2, error = ?3",
            params![id, now_ms(), e],
        )?,
    };
    Ok(())
}

// ---- log ----

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub id: i64,
    pub at_ms: i64,
    pub device_id: Option<i64>,
    pub level: String,
    pub message: String,
}

pub fn log(c: &Connection, device_id: Option<i64>, level: &str, message: &str, raw: Option<&str>) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO log (at_ms, device_id, level, message, raw) VALUES (?, ?, ?, ?, ?)",
        params![now_ms(), device_id, level, message, raw],
    )?;
    Ok(())
}

pub fn recent_log(c: &Connection, limit: u32) -> rusqlite::Result<Vec<LogEntry>> {
    let mut st = c.prepare("SELECT id, at_ms, device_id, level, message FROM log ORDER BY id DESC LIMIT ?")?;
    st.query_map([limit], |r| {
        Ok(LogEntry { id: r.get(0)?, at_ms: r.get(1)?, device_id: r.get(2)?, level: r.get(3)?, message: r.get(4)? })
    })?
    .collect()
}
