//! The server's configuration file (TOML). Everything has a default, so the file is optional.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Where the web UI and the device API listen. One port serves both.
    pub listen: SocketAddr,
    /// The address devices use to reach this server, e.g. `http://192.168.1.10:8374`.
    /// Image URLs handed to devices are built from it. When empty, it is taken from the
    /// request's Host header.
    pub public_url: String,
    /// Holds the database and rendered images.
    pub data_dir: PathBuf,
    /// One directory per app, each with an `app.lua`.
    pub apps_dir: PathBuf,
    /// IANA timezone that apps see, e.g. `America/Los_Angeles`. Empty means the system's.
    pub timezone: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // 8374 spells "TEPH" on a phone keypad.
            listen: "0.0.0.0:8374".parse().unwrap(),
            public_url: String::new(),
            data_dir: "data".into(),
            apps_dir: "apps".into(),
            timezone: String::new(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("tephra.db")
    }
}
