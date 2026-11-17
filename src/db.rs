//! SQLite storage. One connection behind a mutex: the load is a handful of devices.
//!
//! The schema is versioned with `PRAGMA user_version`. Databases are in use now, so a change
//! is a new migration at the end of the list; published ones are never edited.

use anyhow::Context;
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

pub struct Db(Mutex<Connection>);

enum Migration {
    Sql(&'static str),
    Rust(fn(&rusqlite::Transaction) -> anyhow::Result<()>),
}

const MIGRATIONS: &[Migration] = &[
    // 1: devices, their one screen each, app instances and the log.
    Migration::Sql(r#"
    -- A device that registered over Wi-Fi has a MAC and an API key; a placeholder has neither,
    -- and exists so screens can be designed for it.
    CREATE TABLE devices (
        id            INTEGER PRIMARY KEY,
        mac           TEXT UNIQUE,
        api_key       TEXT UNIQUE,
        friendly_id   TEXT NOT NULL DEFAULT '',
        name          TEXT NOT NULL DEFAULT '',
        model         TEXT NOT NULL DEFAULT '',
        width         INTEGER NOT NULL DEFAULT 800,
        height        INTEGER NOT NULL DEFAULT 480,
        firmware      TEXT NOT NULL DEFAULT '',
        battery_v     REAL,
        rssi          INTEGER,
        last_seen_ms  INTEGER,
        created_ms    INTEGER NOT NULL,
        -- Bounds on the refresh interval handed to the device, in seconds.
        refresh_min_s INTEGER NOT NULL DEFAULT 300,
        refresh_max_s INTEGER NOT NULL DEFAULT 3600,
        -- What a double-click of the button does (the firmware's "special function").
        button        TEXT NOT NULL DEFAULT 'identify'
    );

    -- An app added with its own settings. One app may have several instances.
    CREATE TABLE instances (
        id         INTEGER PRIMARY KEY,
        app        TEXT NOT NULL,
        name       TEXT NOT NULL,
        settings   TEXT NOT NULL DEFAULT '{}',
        created_ms INTEGER NOT NULL
    );

    -- The last fetch() result per instance: the data, or the error that replaced it.
    CREATE TABLE app_data (
        instance_id INTEGER PRIMARY KEY REFERENCES instances(id) ON DELETE CASCADE,
        fetched_ms  INTEGER NOT NULL,
        data        TEXT,
        error       TEXT
    );

    -- Each device's one screen: a preset layout...
    CREATE TABLE screens (
        device_id  INTEGER PRIMARY KEY REFERENCES devices(id) ON DELETE CASCADE,
        preset     TEXT NOT NULL DEFAULT 'full',
        updated_ms INTEGER NOT NULL
    );
    -- ...and what each of its slots shows.
    CREATE TABLE slots (
        device_id   INTEGER NOT NULL REFERENCES screens(device_id) ON DELETE CASCADE,
        slot        INTEGER NOT NULL,
        instance_id INTEGER REFERENCES instances(id) ON DELETE SET NULL,
        title_bar   INTEGER NOT NULL DEFAULT 1,
        PRIMARY KEY (device_id, slot)
    );

    -- Device log entries and server events.
    CREATE TABLE log (
        id        INTEGER PRIMARY KEY,
        at_ms     INTEGER NOT NULL,
        device_id INTEGER REFERENCES devices(id) ON DELETE SET NULL,
        level     TEXT NOT NULL,
        message   TEXT NOT NULL,
        raw       TEXT
    );
    CREATE INDEX log_at ON log(at_ms);

    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    "#),
    // 2: apps written in the UI, and instance configs as Lua instead of JSON settings.
    Migration::Sql(r#"
    CREATE TABLE apps (
        id               TEXT PRIMARY KEY,
        name             TEXT NOT NULL UNIQUE COLLATE NOCASE,
        description      TEXT NOT NULL DEFAULT '',
        code             TEXT NOT NULL,
        validate_code    TEXT NOT NULL DEFAULT '',
        validate_enabled INTEGER NOT NULL DEFAULT 0,
        created_ms       INTEGER NOT NULL,
        updated_ms       INTEGER NOT NULL
    );
    ALTER TABLE instances ADD COLUMN config TEXT NOT NULL DEFAULT '';
    "#),
    Migration::Rust(settings_to_config),
    Migration::Sql("ALTER TABLE instances DROP COLUMN settings;"),
    // 3: apps pushed by an agent through the developer API.
    Migration::Sql("ALTER TABLE apps ADD COLUMN prototype INTEGER NOT NULL DEFAULT 0;"),
    // 4: how many grays a device's screen is drawn with (2 or 4).
    Migration::Sql("ALTER TABLE devices ADD COLUMN grays INTEGER NOT NULL DEFAULT 2;"),
];

/// Rewrites each instance's JSON settings as the equivalent Lua config.
fn settings_to_config(tx: &rusqlite::Transaction) -> anyhow::Result<()> {
    let rows: Vec<(i64, String)> = tx
        .prepare("SELECT id, settings FROM instances")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, settings) in rows {
        let v: serde_json::Value = serde_json::from_str(&settings).unwrap_or_default();
        let lua = match v.as_object() {
            Some(o) => o
                .iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| format!("{} = {}\n", crate::apps::lua_key(k), crate::apps::lua_literal(v)))
                .collect(),
            None => String::new(),
        };
        tx.execute("UPDATE instances SET config = ? WHERE id = ?", rusqlite::params![lua, id])?;
    }
    Ok(())
}

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("creating {}", dir.display()))?;
        }
        let mut conn =
            Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        migrate(&mut conn)?;
        Ok(Self(Mutex::new(conn)))
    }

    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves nothing half-written that SQLite didn't roll back.
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn migrate(conn: &mut Connection) -> anyhow::Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    anyhow::ensure!(
        current as usize <= MIGRATIONS.len(),
        "the database is from a newer tephra (schema {current}, this build knows {})",
        MIGRATIONS.len()
    );
    for (i, m) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        match m {
            Migration::Sql(sql) => tx.execute_batch(sql).map_err(anyhow::Error::from),
            Migration::Rust(f) => f(&tx),
        }
        .with_context(|| format!("migrating the database to schema {}", i + 1))?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_to_a_fresh_database() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v as usize, MIGRATIONS.len());
        // Running again is a no-op.
        migrate(&mut conn).unwrap();
    }

    #[test]
    fn json_settings_become_lua_config() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(match &MIGRATIONS[0] {
            Migration::Sql(s) => s,
            _ => unreachable!(),
        })
        .unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute(
            "INSERT INTO instances (app, name, settings, created_ms) VALUES ('weather', 'W', ?, 0)",
            [r#"{"location":"Seattle \"WA\"","units":"metric","n":3,"on":true,"gone":null}"#],
        )
        .unwrap();
        migrate(&mut conn).unwrap();
        let config: String = conn.query_row("SELECT config FROM instances", [], |r| r.get(0)).unwrap();
        assert_eq!(config, "location = \"Seattle \\\"WA\\\"\"\nn = 3\non = true\nunits = \"metric\"\n");
    }
}
