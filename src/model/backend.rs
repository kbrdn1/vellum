//! The database engine tag. Pure data — the `Driver` port reports it via
//! `kind()`, and (later) the TUI keys backend-specific behaviour off it. One
//! variant for Phase 0 (SQLite-only); extended as backends land (Postgres,
//! MySQL, DuckDB, … — ARCHITECTURE §4).

/// Which database engine a connection talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
  Sqlite,
}
