//! Integration tests for the `Driver` port against an in-process SQLite
//! database (sqlx, file-backed tempfile opened read-only — no external
//! service, CI-friendly). Exercises connect → query → `QueryResult`, the
//! SQLite-storage-class → `Value` mapping (the type seam from #4), and the
//! read-only boundary the read path enforces.

use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::Executor as _;
use tempfile::NamedTempFile;
use vellum::driver::{Driver, SqliteDriver};
use vellum::model::{Backend, TypeKind, Value};

/// A read-only `SqliteDriver` over a freshly-seeded tempfile database. The
/// returned `NamedTempFile` guard must stay alive for the driver's lifetime —
/// dropping it deletes the backing file.
async fn seeded_driver() -> (SqliteDriver, NamedTempFile) {
  let file = NamedTempFile::new().expect("create temp db file");
  let dsn = format!("sqlite:{}", file.path().display());

  // Initialise + seed with a throwaway writable connection, then close it so
  // the read-only driver below holds the only handle.
  let setup = SqlitePool::connect_with(
    SqliteConnectOptions::from_str(&dsn)
      .expect("parse dsn")
      .create_if_missing(true),
  )
  .await
  .expect("open writable connection for seeding");
  setup
    .execute("create table items (id integer, label text)")
    .await
    .expect("create table");
  setup
    .execute("insert into items (id, label) values (1, 'alpha'), (2, 'beta')")
    .await
    .expect("seed rows");
  setup.close().await;

  let driver = SqliteDriver::connect(&dsn).await.expect("connect read-only");
  (driver, file)
}

#[tokio::test]
async fn connect_then_select_literal_one() {
  let (driver, _db) = seeded_driver().await;
  let result = driver.query("select 1").await.expect("query select 1");
  assert_eq!(driver.kind(), Backend::Sqlite);
  assert_eq!(result.columns.len(), 1);
  assert_eq!(result.rows.len(), 1);
  assert_eq!(result.rows[0][0], Value::Int(1));
  assert_eq!(result.affected, None);
}

#[tokio::test]
async fn reads_seeded_table_rows_and_types() {
  let (driver, _db) = seeded_driver().await;
  let result = driver
    .query("select id, label from items order by id")
    .await
    .expect("query items");
  assert_eq!(result.columns.len(), 2);
  assert_eq!(result.columns[0].name, "id");
  assert_eq!(result.columns[0].kind, TypeKind::Int);
  assert_eq!(result.columns[1].name, "label");
  assert_eq!(result.columns[1].kind, TypeKind::Text);
  assert_eq!(result.rows.len(), 2);
  assert_eq!(result.rows[0], vec![Value::Int(1), Value::Text("alpha".into())]);
  assert_eq!(result.rows[1], vec![Value::Int(2), Value::Text("beta".into())]);
}

#[tokio::test]
async fn maps_each_sqlite_storage_class_to_value() {
  // The five SQLite storage classes — NULL / INTEGER / REAL / TEXT / BLOB —
  // each map to their normalised `Value`.
  let (driver, _db) = seeded_driver().await;
  let result = driver
    .query("select 42, 3.5, 'hello', null, x'deadbeef'")
    .await
    .expect("query mixed literals");
  let row = &result.rows[0];
  assert_eq!(row[0], Value::Int(42));
  assert_eq!(row[1], Value::Float(3.5));
  assert_eq!(row[2], Value::Text("hello".into()));
  assert_eq!(row[3], Value::Null);
  assert_eq!(row[4], Value::Bytes(vec![0xde, 0xad, 0xbe, 0xef]));
}

#[tokio::test]
async fn column_kind_follows_the_cell_value() {
  // SQLite reports no decltype for literal/expression columns, so column kinds
  // are derived from the first row's values, not the column metadata.
  let (driver, _db) = seeded_driver().await;
  let result = driver.query("select 7, 'x'").await.expect("query literals");
  assert_eq!(result.columns[0].kind, TypeKind::Int);
  assert_eq!(result.columns[1].kind, TypeKind::Text);
}

#[tokio::test]
async fn invalid_sql_returns_driver_error() {
  let (driver, _db) = seeded_driver().await;
  let outcome = driver.query("select * from does_not_exist").await;
  assert!(outcome.is_err(), "querying a missing table must error");
}

#[tokio::test]
async fn query_refuses_writes_on_the_read_path() {
  // A write is rejected by the parser guard before it reaches the database
  // (the read-only connection is a backstop). Intentional writes go through
  // the gated write/diff path (#64).
  let (driver, _db) = seeded_driver().await;
  let err = driver
    .query("create table t (x integer)")
    .await
    .expect_err("a write through the read path must be refused");
  assert!(
    err.to_string().contains("read-only path"),
    "expected a read-only-path refusal, got: {err}"
  );
}

#[tokio::test]
async fn refuses_create_temp_table() {
  // `SQLITE_OPEN_READONLY` alone wouldn't catch a TEMP-schema write (it's not
  // the main file); the parser guard rejects it before execution.
  let (driver, _db) = seeded_driver().await;
  let outcome = driver.query("create temp table t (x integer)").await;
  assert!(outcome.is_err(), "CREATE TEMP TABLE must be refused");
}

#[tokio::test]
async fn refuses_multi_statement_input() {
  // sqlx-sqlite would run every statement and merge their rows under the first
  // header; the guard rejects multi-statement input before execution.
  let (driver, _db) = seeded_driver().await;
  let outcome = driver.query("select 1 as a; select 2 as b, 3 as c").await;
  assert!(outcome.is_err(), "multi-statement input must be refused");
}

#[tokio::test]
async fn refuses_multi_statement_even_when_unparseable() {
  // The bypass: a statement sqlparser can't fully parse (`NOT INDEXED`),
  // chained with a write. The token-level statement count catches the `;`
  // before execution, so the parse-failure fallthrough can't be abused.
  let (driver, _db) = seeded_driver().await;
  let outcome = driver
    .query("select * from items not indexed; create temp table t (x integer)")
    .await;
  assert!(
    outcome.is_err(),
    "an unparseable multi-statement payload must be refused"
  );
}

#[tokio::test]
async fn pragma_query_only_off_cannot_unlock_writes() {
  // The documented bypass: flip query_only off, then write, in one payload.
  // It is multi-statement, so the parser guard refuses it before execution.
  let (driver, _db) = seeded_driver().await;
  let outcome = driver
    .query("pragma query_only = off; create table t (x integer)")
    .await;
  assert!(
    outcome.is_err(),
    "the query_only=off bypass must be refused on the read path"
  );
}

#[tokio::test]
async fn empty_select_still_reports_columns() {
  // A valid SELECT that returns zero rows still has a column schema
  // (`SELECT ... WHERE 0`). The headers must survive an empty result so the
  // table render can tell "empty with columns" from "no columns".
  let (driver, _db) = seeded_driver().await;
  let result = driver
    .query("select 1 as n, 'x' as label where 0")
    .await
    .expect("query empty select");
  assert!(result.rows.is_empty());
  assert_eq!(result.columns.len(), 2);
  assert_eq!(result.columns[0].name, "n");
  assert_eq!(result.columns[1].name, "label");
}
