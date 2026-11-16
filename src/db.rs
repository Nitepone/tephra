//! SQLite storage. One connection behind a mutex: the load is a handful of devices.
//!
//! The schema is versioned with `PRAGMA user_version`. Until there is a release, version 1
//! may still be edited in place (delete `data/tephra.db` after changing it).

use anyhow::Context;
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

pub struct Db(Mutex<Connection>);

const MIGRATIONS: &[&str] = &[
    // 1: devices, their one screen each, app instances and the log.
    r#"
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
    "#,
];

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
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)
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
}
