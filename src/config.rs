//! `.vellum.toml` — the connection manager contract.
//!
//! Pure parse + validation: a TOML string in, a typed [`Config`] out. No I/O,
//! no secrets, no driver wiring — those are separate seams (keyring + the
//! `VELLUM_DSN_<NAME>` env override land in #9; the drivers consume the parsed
//! [`Connection`] later). The shape here is the schema we **freeze for 1.0**,
//! so it is deliberately strict and *safe by construction*:
//!
//! - The public [`Config`] / [`Connection`] do **not** derive `Deserialize`
//!   and carry **no** secret field. The only deserialise surface is the
//!   private `Raw*` layer below, which every parse routes through — there is
//!   no path that builds a public value while skipping the password gate, and
//!   the derived `Debug` of a public value has no secret to leak.
//! - Unknown keys are rejected (`deny_unknown_fields`) and a `password` of any
//!   TOML type is refused outright (see [`Config::from_toml_str`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Result, VellumError};
use crate::model::Backend;

/// A parsed `.vellum.toml`: the named connections plus the `[ui]` block.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
  /// `[connections.<name>]` tables, keyed by name. A `BTreeMap` keeps the
  /// order deterministic (stable sidebar / listing, reproducible tests).
  pub connections: BTreeMap<String, Connection>,
  /// `[ui]` block — defaulted in full when the section is omitted.
  pub ui: Ui,
}

/// One `[connections.<name>]` entry. Every field bar `backend` is optional —
/// a server connection fills host/port/user/database/sslmode, a SQLite one
/// fills `path`. There is no password field: a secret never lives here, and
/// the public type carries no surface for one.
#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
  /// The engine to talk to.
  pub backend: Backend,
  pub host: Option<String>,
  pub port: Option<u16>,
  pub user: Option<String>,
  pub database: Option<String>,
  /// SQLite (and other file-backed engines): path to the database file.
  pub path: Option<String>,
  pub sslmode: Option<String>,
}

/// The `[ui]` block. Both fields default so a file may omit them (or the whole
/// section). Unlike the connection types it has no secret, so it deserialises
/// directly.
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

/// Private deserialise layer — the sole `Deserialize` surface. Mirrors the
/// public shape but adds the captured `password`, so the parser sees a secret
/// no matter how it was written and can reject it before any public value
/// exists.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
  #[serde(default)]
  connections: BTreeMap<String, RawConnection>,
  #[serde(default)]
  ui: Ui,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConnection {
  backend: Backend,
  #[serde(default)]
  host: Option<String>,
  #[serde(default)]
  port: Option<u16>,
  #[serde(default)]
  user: Option<String>,
  #[serde(default)]
  database: Option<String>,
  #[serde(default)]
  path: Option<String>,
  #[serde(default)]
  sslmode: Option<String>,
  /// Captured as a raw TOML value so a secret of *any* type (string, integer,
  /// table, dotted key) trips the presence gate — rather than failing serde's
  /// type check first with a generic error that may echo the value. Only its
  /// presence is ever inspected; the value is neither stored nor formatted.
  #[serde(default)]
  password: Option<toml::Value>,
}

impl Config {
  /// Parse a `.vellum.toml` string into a typed [`Config`].
  ///
  /// Fails with [`VellumError::Config`] on malformed TOML, an unknown
  /// `backend`, an unknown key, or a `password` of any type.
  pub fn from_toml_str(input: &str) -> Result<Config> {
    let raw: RawConfig = toml::from_str(input).map_err(|e| VellumError::Config(e.message().to_string()))?;

    let mut connections = BTreeMap::new();
    for (name, conn) in raw.connections {
      // A secret never lives in the file. Reject on presence (any type),
      // naming the connection and pointing at the real channels (#9 wires
      // keyring + `VELLUM_DSN_<NAME>`) — never echoing the value.
      if conn.password.is_some() {
        return Err(VellumError::Config(format!(
          "connection `{name}`: `password` must not be stored in `.vellum.toml` — \
           put the secret in the system keyring or a `VELLUM_DSN_*` environment variable"
        )));
      }
      connections.insert(
        name,
        Connection {
          backend: conn.backend,
          host: conn.host,
          port: conn.port,
          user: conn.user,
          database: conn.database,
          path: conn.path,
          sslmode: conn.sslmode,
        },
      );
    }

    // Two connection names must not collide under the `VELLUM_DSN_<NAME>`
    // override (#9): the normalisation (uppercase, non-alphanumeric → `_`) is
    // not injective, so distinct names can map to one env var. Reject the
    // ambiguity here — otherwise one connection's override could silently
    // mis-route a secret to another.
    let mut env_overrides: BTreeMap<String, String> = BTreeMap::new();
    for name in connections.keys() {
      let env = crate::secrets::env_var_name(name);
      if let Some(first) = env_overrides.insert(env.clone(), name.clone()) {
        return Err(VellumError::Config(format!(
          "connections `{first}` and `{name}` both map to the `{env}` environment \
           override — rename one so secret overrides stay unambiguous"
        )));
      }
    }

    Ok(Config {
      connections,
      ui: raw.ui,
    })
  }

  /// Discover and load `.vellum.toml` from the standard locations: the current
  /// directory (`./.vellum.toml`, a project-local override) first, then the
  /// global config dir (`<config>/vellum/.vellum.toml`). The first that exists
  /// is parsed; none found is a [`VellumError::Config`] pointing at how to
  /// create one.
  pub fn load() -> Result<Config> {
    Self::load_discovered(&config_candidates(), |path| path.exists())
  }

  /// Read and parse the `.vellum.toml` at `path`. A missing or unreadable file
  /// is a [`VellumError::Config`] naming the path — never the file contents, so
  /// a secret mistakenly placed in it can't surface in the error.
  pub fn load_path(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path)
      .map_err(|e| VellumError::Config(format!("could not read `{}`: {e}", path.display())))?;
    Config::from_toml_str(&text)
  }

  /// Load the first of `candidates` for which `exists` returns true, parsing it
  /// via [`Config::load_path`]. When none exists, a [`VellumError::Config`]
  /// names where it looked and points at `vellum connect`. `exists` is injected
  /// so the precedence is testable without depending on the real filesystem.
  pub fn load_discovered(candidates: &[PathBuf], exists: impl Fn(&Path) -> bool) -> Result<Config> {
    match candidates.iter().find(|path| exists(path)) {
      Some(path) => Self::load_path(path),
      None => Err(VellumError::Config(format!(
        "no `.vellum.toml` found (looked in {}) — create one or run `vellum connect <name>` to register a connection",
        render_candidates(candidates)
      ))),
    }
  }
}

/// The ordered `.vellum.toml` search path: the current directory first (a
/// project-local override), then the global config dir via `dirs`
/// (`~/.config/vellum/.vellum.toml` on Linux, the platform equivalent
/// elsewhere). A platform with no resolvable config dir simply drops the global
/// candidate — cwd still works.
fn config_candidates() -> Vec<PathBuf> {
  let mut candidates = vec![PathBuf::from(".vellum.toml")];
  if let Some(dir) = dirs::config_dir() {
    candidates.push(dir.join("vellum").join(".vellum.toml"));
  }
  candidates
}

/// Render the candidate paths for the "not found" message: a comma-separated
/// list of quoted paths (or a plain note when there are none).
fn render_candidates(candidates: &[PathBuf]) -> String {
  if candidates.is_empty() {
    return "no candidate paths".to_string();
  }
  candidates
    .iter()
    .map(|path| format!("`{}`", path.display()))
    .collect::<Vec<_>>()
    .join(", ")
}
