//! Build a backend DSN from a parsed `.vellum.toml` [`Connection`] plus a
//! resolved password (#95).
//!
//! This is a **pure** transform — `Connection` + `Option<&str>` password in, a
//! DSN string out — so it is exhaustively unit-tested (`tests/dsn_tests.rs`)
//! without touching a database. The built DSN is then routed through each
//! driver's existing `Driver::connect(dsn)` so the per-engine read-only
//! backstop is never duplicated (the sacred write path stays single-sourced).
//!
//! Every value that lands inside the URL (user, password, database, sslmode)
//! is percent-encoded first: a password like `p@ss:w/rd` carries bytes that are
//! *structural* in a DSN (`@` ends the userinfo, `:` splits user/pass, `/`
//! starts the path), so without encoding it would mis-parse and silently
//! re-route the connection to the wrong host. SQLite is **not** built here — it
//! opens by path (`SqliteDriver::open_readonly`), which sidesteps the `?%#`
//! reinterpretation a `sqlite:` URI would apply — so a SQLite connection is a
//! caller error at this seam.

// Placeholder body — real implementation lands in the green step (#95).
use crate::config::Connection;
use crate::error::Result;

/// Build the connection DSN for `conn`, injecting `password` (already resolved
/// from the keyring / env) into the userinfo when present.
pub fn build(conn: &Connection, password: Option<&str>) -> Result<String> {
  let _ = (conn, password);
  Ok(String::new())
}
