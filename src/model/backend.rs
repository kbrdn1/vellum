//! The database engine tag. Pure data — the `Driver` port reports it via
//! `kind()`, the `.vellum.toml` `backend = "…"` field deserialises into it,
//! and (later) the TUI keys backend-specific behaviour off it. SQLite shipped
//! in Phase 0; Postgres / MySQL are the Phase 1 sqlx drivers. The variant set
//! is the closed list of *valid* backend names — an unknown name is a config
//! error (ARCHITECTURE §4). A variant existing here does not imply its driver
//! is wired yet.

use serde::Deserialize;

/// Which database engine a connection talks to.
///
/// `Deserialize` maps the lowercase `.vellum.toml` token (`"postgres"`,
/// `"mysql"`, `"sqlite"`) to the variant; serde rejects any other token with
/// an "unknown variant" error, which the config parser surfaces as a
/// `VellumError::Config`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
  Postgres,
  MySql,
  Sqlite,
}

impl Backend {
  /// The lowercase engine tag, matching the `.vellum.toml` token — the browse
  /// header badge (`sqlite` / `postgres` / `mysql`).
  pub fn as_str(&self) -> &'static str {
    match self {
      Backend::Postgres => "postgres",
      Backend::MySql => "mysql",
      Backend::Sqlite => "sqlite",
    }
  }

  /// Quote an identifier (schema / table / column) for this engine's SQL
  /// dialect, doubling the quote char so a name containing it can't break out of
  /// the quoting. Postgres and SQLite use ANSI double quotes; MySQL uses
  /// backticks — its default mode reads `"` as a string literal, so a
  /// double-quoted table name would parse as a string and the query would fail.
  pub fn quote_ident(&self, ident: &str) -> String {
    match self {
      Backend::MySql => format!("`{}`", ident.replace('`', "``")),
      Backend::Postgres | Backend::Sqlite => format!("\"{}\"", ident.replace('"', "\"\"")),
    }
  }
}
