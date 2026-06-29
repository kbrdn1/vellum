//! `.vellum.toml` — the connection manager contract.
//!
//! Pure parse + validation: a TOML string in, a typed [`Config`] out. No I/O,
//! no secrets, no driver wiring — those are separate seams (keyring + the
//! `VELLUM_DSN_<NAME>` env override land in #9; the drivers consume the parsed
//! [`Connection`] later). The shape here is the schema we **freeze for 1.0**,
//! so it is deliberately strict: unknown keys are rejected and a plaintext
//! `password` is refused outright (see [`Config::from_toml_str`]).

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::{Result, VellumError};
use crate::model::Backend;

/// A parsed `.vellum.toml`: the named connections plus the `[ui]` block.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
  /// `[connections.<name>]` tables, keyed by name. A `BTreeMap` keeps the
  /// order deterministic (stable sidebar / listing, reproducible tests).
  #[serde(default)]
  pub connections: BTreeMap<String, Connection>,
  /// `[ui]` block — defaulted in full when the section is omitted.
  #[serde(default)]
  pub ui: Ui,
}

/// One `[connections.<name>]` entry. Every field bar `backend` is optional —
/// a server connection fills host/port/user/database/sslmode, a SQLite one
/// fills `path`. The password is intentionally absent: it never lives here.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
  /// The engine to talk to. An unknown token is a config error.
  pub backend: Backend,
  #[serde(default)]
  pub host: Option<String>,
  #[serde(default)]
  pub port: Option<u16>,
  #[serde(default)]
  pub user: Option<String>,
  #[serde(default)]
  pub database: Option<String>,
  /// SQLite (and other file-backed engines): path to the database file.
  #[serde(default)]
  pub path: Option<String>,
  #[serde(default)]
  pub sslmode: Option<String>,
  /// Refused on presence. A secret never lives in the file — but it is
  /// captured here (rather than left to `deny_unknown_fields`) so the parser
  /// can reject it with a message that points at the real channels instead of
  /// a generic "unknown field". Non-`pub`: never exposed to callers.
  #[serde(default)]
  password: Option<String>,
}

/// The `[ui]` block. Both fields default so a file may omit them (or the whole
/// section).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ui {
  /// Rows fetched per browse page (`LIMIT`).
  #[serde(default = "default_page_size")]
  pub page_size: usize,
  /// Active theme name.
  #[serde(default = "default_theme")]
  pub theme: String,
}

fn default_page_size() -> usize {
  200
}

fn default_theme() -> String {
  "vellum".to_string()
}

impl Default for Ui {
  fn default() -> Self {
    Self {
      page_size: default_page_size(),
      theme: default_theme(),
    }
  }
}

impl Config {
  /// Parse a `.vellum.toml` string into a typed [`Config`].
  ///
  /// Fails with [`VellumError::Config`] on malformed TOML, an unknown
  /// `backend`, an unknown key, or a plaintext secret.
  pub fn from_toml_str(input: &str) -> Result<Config> {
    let config: Config = toml::from_str(input).map_err(|e| VellumError::Config(e.message().to_string()))?;

    // A secret never lives in the file. Reject on presence, naming the
    // connection and pointing at the real channels (#9 wires keyring +
    // `VELLUM_DSN_<NAME>`).
    for (name, conn) in &config.connections {
      if conn.password.is_some() {
        return Err(VellumError::Config(format!(
          "connection `{name}`: `password` must not be stored in `.vellum.toml` — \
           put the secret in the system keyring or a `VELLUM_DSN_*` environment variable"
        )));
      }
    }

    Ok(config)
  }
}
