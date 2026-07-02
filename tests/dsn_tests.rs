//! Unit tests for the DSN builder (`vellum::dsn`). Pure transform — a parsed
//! [`Connection`] plus a resolved password in, a DSN string out. The security
//! surface is the percent-encoding of URL-structural bytes in a password /
//! user: an unencoded `@` or `:` would re-parse the userinfo boundary and
//! silently point the connection at the wrong host. Every case here pins the
//! exact string a driver's `connect(dsn)` will receive.

use vellum::config::Connection;
use vellum::dsn;
use vellum::model::Backend;

/// A bare connection of `backend` with every optional field unset — tests fill
/// only what they exercise.
fn conn(backend: Backend) -> Connection {
  Connection {
    backend,
    host: None,
    port: None,
    user: None,
    database: None,
    path: None,
    sslmode: None,
  }
}

#[test]
fn postgres_full_connection_builds_a_complete_url() {
  let mut c = conn(Backend::Postgres);
  c.host = Some("db.internal".to_string());
  c.port = Some(5432);
  c.user = Some("app".to_string());
  c.database = Some("appdb".to_string());
  c.sslmode = Some("require".to_string());

  let dsn = dsn::build(&c, Some("s3cret")).unwrap();
  assert_eq!(dsn, "postgres://app:s3cret@db.internal:5432/appdb?sslmode=require");
}

#[test]
fn postgres_password_special_chars_are_percent_encoded_in_userinfo() {
  let mut c = conn(Backend::Postgres);
  c.host = Some("db".to_string());
  c.user = Some("app".to_string());

  // `@ : / # %` are all structural in a DSN — each must be percent-encoded so
  // the userinfo boundary (`@`) and user/pass split (`:`) stay unambiguous.
  let dsn = dsn::build(&c, Some("p@ss:w/rd#%")).unwrap();
  assert_eq!(dsn, "postgres://app:p%40ss%3Aw%2Frd%23%25@db");
}

#[test]
fn postgres_user_special_chars_are_percent_encoded() {
  let mut c = conn(Backend::Postgres);
  c.host = Some("db".to_string());
  c.user = Some("a@d/min".to_string());

  let dsn = dsn::build(&c, Some("pw")).unwrap();
  assert_eq!(dsn, "postgres://a%40d%2Fmin:pw@db");
}

#[test]
fn postgres_without_a_password_omits_the_userinfo_colon() {
  let mut c = conn(Backend::Postgres);
  c.host = Some("db".to_string());
  c.user = Some("app".to_string());

  let dsn = dsn::build(&c, None).unwrap();
  assert_eq!(dsn, "postgres://app@db");
}

#[test]
fn postgres_missing_host_defaults_to_localhost() {
  // `host` is optional in the schema; an empty URL authority (`postgres:///db`)
  // is ambiguous, so the builder defaults to `localhost` rather than emit it.
  let mut c = conn(Backend::Postgres);
  c.user = Some("app".to_string());

  let dsn = dsn::build(&c, None).unwrap();
  assert_eq!(dsn, "postgres://app@localhost");
}

#[test]
fn postgres_omits_absent_port_database_and_sslmode() {
  let mut c = conn(Backend::Postgres);
  c.host = Some("db".to_string());
  c.user = Some("app".to_string());

  let dsn = dsn::build(&c, Some("pw")).unwrap();
  assert_eq!(dsn, "postgres://app:pw@db");
}

#[test]
fn postgres_without_a_user_has_no_userinfo() {
  let mut c = conn(Backend::Postgres);
  c.host = Some("db".to_string());
  c.database = Some("appdb".to_string());

  let dsn = dsn::build(&c, None).unwrap();
  assert_eq!(dsn, "postgres://db/appdb");
}

#[test]
fn postgres_password_without_a_user_is_a_config_error() {
  // A password with no user can't be placed in `user:pass@` — refuse loudly
  // rather than silently drop the secret and connect unauthenticated.
  let mut c = conn(Backend::Postgres);
  c.host = Some("db".to_string());

  let err = dsn::build(&c, Some("pw")).unwrap_err();
  assert!(
    matches!(err, vellum::error::VellumError::Config(_)),
    "expected a config error, got {err:?}"
  );
  // The error must not echo the password.
  assert!(!format!("{err}").contains("pw"), "the error must not leak the password");
}

#[test]
fn mysql_uses_the_ssl_mode_query_parameter() {
  // MySQL's TLS knob is `ssl-mode` (Postgres uses `sslmode`) — the builder must
  // emit the engine's own parameter name so `sslmode = "..."` isn't silently
  // dropped, which would leave a connection the user asked to secure in clear.
  let mut c = conn(Backend::MySql);
  c.host = Some("127.0.0.1".to_string());
  c.port = Some(3306);
  c.user = Some("root".to_string());
  c.database = Some("app".to_string());
  c.sslmode = Some("REQUIRED".to_string());

  let dsn = dsn::build(&c, Some("pw")).unwrap();
  assert_eq!(dsn, "mysql://root:pw@127.0.0.1:3306/app?ssl-mode=REQUIRED");
}

#[test]
fn sqlite_is_not_built_as_a_dsn() {
  // SQLite opens by path (`open_readonly`) to keep `?%#` literal; asking the DSN
  // builder for one is a caller error, surfaced as a config error.
  let mut c = conn(Backend::Sqlite);
  c.path = Some("./data/app.db".to_string());

  let err = dsn::build(&c, None).unwrap_err();
  assert!(
    matches!(err, vellum::error::VellumError::Config(_)),
    "expected a config error for a SQLite DSN, got {err:?}"
  );
}
