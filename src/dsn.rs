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
//! re-route the connection to the wrong host. The host is emitted verbatim —
//! encoding it would mangle the `.` in a name like `db.internal` or the `[]`
//! of an IPv6 literal. SQLite is **not** built here — it opens by path
//! (`SqliteDriver::open_readonly`), which sidesteps the `?%#` reinterpretation
//! a `sqlite:` URI would apply — so a SQLite connection is a caller error at
//! this seam.

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

use crate::config::Connection;
use crate::error::{Result, VellumError};
use crate::model::Backend;

/// Percent-encode set: every non-alphanumeric byte *except* the RFC 3986
/// unreserved marks (`-` `_` `.` `~`), which are safe unencoded. Encoding them
/// would needlessly mangle a value like `VERIFY_CA` (MySQL's `ssl-mode`) or a
/// password containing `.` — harmless once decoded, but it makes the DSN
/// unreadable and trips a strict parser that compares the raw token.
const ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

/// Percent-encode a value for a URL: everything reserved / unsafe is escaped,
/// the unreserved marks left as-is (see [`ENCODE_SET`]).
fn enc(value: &str) -> String {
  utf8_percent_encode(value, ENCODE_SET).to_string()
}

/// Build the connection DSN for `conn`, injecting `password` (already resolved
/// from the keyring / env) into the userinfo when present.
///
/// Fails with [`VellumError::Config`] when the connection can't yield a valid
/// DSN: a SQLite backend (opens by path, not a DSN), or a resolved password
/// with no `user` to attach it to.
pub fn build(conn: &Connection, password: Option<&str>) -> Result<String> {
  let (scheme, ssl_param) = match conn.backend {
    Backend::Postgres => ("postgres", "sslmode"),
    Backend::MySql => ("mysql", "ssl-mode"),
    Backend::Sqlite => {
      return Err(VellumError::Config(
        "a SQLite connection opens by its `path`, not a DSN — set `path` in the `.vellum.toml` entry".to_string(),
      ))
    }
  };

  let mut url = format!("{scheme}://");

  // Userinfo: `user`, plus `:password` when one was resolved. A password with
  // no user can't be placed in `user:pass@`, so refuse it rather than silently
  // drop the secret and connect unauthenticated.
  match (&conn.user, password) {
    (Some(user), Some(pw)) => url.push_str(&format!("{}:{}@", enc(user), enc(pw))),
    (Some(user), None) => url.push_str(&format!("{}@", enc(user))),
    (None, Some(_)) => {
      return Err(VellumError::Config(
        "a password was resolved but the connection has no `user` — add `user` to the `.vellum.toml` entry".to_string(),
      ))
    }
    (None, None) => {}
  }

  // Host (verbatim; default `localhost` so the authority is never empty) and an
  // optional port.
  url.push_str(conn.host.as_deref().unwrap_or("localhost"));
  if let Some(port) = conn.port {
    url.push_str(&format!(":{port}"));
  }

  // Optional database as a path segment.
  if let Some(database) = &conn.database {
    url.push('/');
    url.push_str(&enc(database));
  }

  // Optional TLS mode under the engine's own query parameter (`sslmode` for
  // Postgres, `ssl-mode` for MySQL) so a `sslmode = "require"` the user set to
  // secure the connection is never silently dropped. MySQL's parameter also
  // takes a different vocabulary, so the generic value is mapped for it.
  if let Some(mode) = &conn.sslmode {
    let value = if conn.backend == Backend::MySql {
      mysql_ssl_mode(mode)
    } else {
      mode.clone()
    };
    url.push_str(&format!("?{ssl_param}={}", enc(&value)));
  }

  Ok(url)
}

/// Map a generic (Postgres-vocabulary) `sslmode` value to MySQL's `ssl-mode`
/// vocabulary: the `.vellum.toml` field is the common `sslmode`
/// (`require` / `prefer` / `verify-full`), but `MySqlConnectOptions` expects
/// `REQUIRED` / `PREFERRED` / `VERIFY_IDENTITY` and rejects the Postgres spelling
/// outright. Already-MySQL spellings map to themselves; an unknown value passes
/// through unchanged, so it fails closed at connect rather than being silently
/// rewritten to something the user did not ask for.
fn mysql_ssl_mode(value: &str) -> String {
  match value.to_ascii_lowercase().as_str() {
    "disable" | "disabled" => "DISABLED",
    "prefer" | "preferred" => "PREFERRED",
    "require" | "required" => "REQUIRED",
    "verify-ca" | "verify_ca" => "VERIFY_CA",
    "verify-full" | "verify-identity" | "verify_identity" => "VERIFY_IDENTITY",
    _ => value,
  }
  .to_string()
}
