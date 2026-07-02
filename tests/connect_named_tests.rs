//! Integration tests for the named-connection factory (`driver::connect_named`)
//! against an in-process SQLite tempfile — no external service, CI-friendly.
//! The Postgres / MySQL arms compose `dsn::build` (unit-tested) with each
//! driver's `connect` (exercised under the `it-db` feature against a real
//! server), so the CI-safe surface here is the SQLite path plus the DSN-override
//! dispatch that bypasses the config fields.

use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::Executor as _;
use tempfile::NamedTempFile;
use vellum::config::Connection;
use vellum::driver::connect_named;
use vellum::model::Backend;
use vellum::secrets::{Credential, SecretString};

/// A seeded, closed SQLite tempfile. The returned guard must outlive the driver
/// — dropping it deletes the backing file.
async fn seed_sqlite() -> NamedTempFile {
  let file = NamedTempFile::new().expect("create temp db file");
  let dsn = format!("sqlite:{}", file.path().display());
  let setup = SqlitePool::connect_with(
    SqliteConnectOptions::from_str(&dsn)
      .expect("parse dsn")
      .create_if_missing(true),
  )
  .await
  .expect("open writable connection for seeding");
  setup
    .execute("create table widgets (id integer, name text)")
    .await
    .expect("create table");
  setup.close().await;
  file
}

/// A bare `Connection` of `backend` with every optional field unset.
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

#[tokio::test]
async fn sqlite_by_path_with_no_credential_connects_read_only() {
  let db = seed_sqlite().await;
  let mut c = conn(Backend::Sqlite);
  c.path = Some(db.path().display().to_string());

  let driver = connect_named(&c, None).await.expect("a sqlite path connects");
  assert_eq!(driver.backend(), Backend::Sqlite);
  // The catalog reflects the seeded schema — proving it opened the named file.
  let catalog = driver.introspect().await.expect("introspect");
  let names: Vec<_> = catalog
    .databases
    .iter()
    .flat_map(|d| &d.schemas)
    .flat_map(|s| &s.relations)
    .map(|r| r.name.as_str())
    .collect();
  assert!(names.contains(&"widgets"), "expected the seeded table, got {names:?}");
}

#[tokio::test]
async fn sqlite_without_a_path_is_a_config_error() {
  let c = conn(Backend::Sqlite);

  // `Box<dyn Driver>` isn't `Debug`, so destructure rather than `expect_err`.
  let Err(err) = connect_named(&c, None).await else {
    panic!("a SQLite connection with no path must error");
  };
  assert!(
    matches!(err, vellum::error::VellumError::Config(_)),
    "expected a config error, got {err:?}"
  );
}

#[tokio::test]
async fn a_dsn_credential_override_is_used_verbatim() {
  // `VELLUM_DSN_<NAME>` resolves to `Credential::Dsn` — the full DSN is used
  // as-is (config host/path fields ignored), dispatched by backend. Here the
  // connection carries NO path, yet the override opens the seeded file.
  let db = seed_sqlite().await;
  let c = conn(Backend::Sqlite);
  let dsn = SecretString::from(format!("sqlite:{}", db.path().display()));

  let driver = connect_named(&c, Some(Credential::Dsn(dsn)))
    .await
    .expect("the DSN override connects");
  assert_eq!(driver.backend(), Backend::Sqlite);
  let result = driver
    .query("select count(*) from widgets")
    .await
    .expect("query the override db");
  assert_eq!(result.rows.len(), 1);
}
