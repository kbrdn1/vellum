//! Integration tests for the `Driver` port against an in-process SQLite
//! database (sqlx, in-memory — no external service, CI-friendly). Exercises
//! the connect → query → `QueryResult` loop and the SQLite-storage-class →
//! `Value` mapping (the type seam from #4).

use vellum::driver::{Driver, SqliteDriver};
use vellum::model::{Backend, TypeKind, Value};

async fn memory_driver() -> SqliteDriver {
  SqliteDriver::connect("sqlite::memory:")
    .await
    .expect("connect to in-memory sqlite")
}

#[tokio::test]
async fn connect_then_select_literal_one() {
  let driver = memory_driver().await;
  let result = driver.query("select 1").await.expect("query select 1");
  assert_eq!(driver.kind(), Backend::Sqlite);
  assert_eq!(result.columns.len(), 1);
  assert_eq!(result.rows.len(), 1);
  assert_eq!(result.rows[0][0], Value::Int(1));
  assert_eq!(result.affected, None);
}

#[tokio::test]
async fn maps_each_sqlite_storage_class_to_value() {
  // The five SQLite storage classes — NULL / INTEGER / REAL / TEXT / BLOB —
  // each map to their normalised `Value`.
  let driver = memory_driver().await;
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
  // SQLite reports no decltype for literal/expression columns, so column
  // kinds are derived from the first row's values, not the column metadata.
  let driver = memory_driver().await;
  let result = driver.query("select 7, 'x'").await.expect("query literals");
  assert_eq!(result.columns[0].kind, TypeKind::Int);
  assert_eq!(result.columns[1].kind, TypeKind::Text);
}

#[tokio::test]
async fn invalid_sql_returns_driver_error() {
  let driver = memory_driver().await;
  let outcome = driver.query("select * from does_not_exist").await;
  assert!(outcome.is_err(), "querying a missing table must error");
}

#[tokio::test]
async fn query_refuses_writes_on_the_read_path() {
  // The read path is read-only by construction (`PRAGMA query_only`): SQLite
  // itself rejects a write statement, so `query()` can never silently mutate
  // a database. Intentional writes go through the gated write/diff path
  // (a later, sacred phase — tracked by #64).
  let driver = memory_driver().await;
  let err = driver
    .query("create table t (x integer)")
    .await
    .expect_err("a write through the read path must be refused");
  assert!(
    err.to_string().to_lowercase().contains("readonly"),
    "expected a read-only refusal from SQLite, got: {err}"
  );
}

#[tokio::test]
async fn empty_select_still_reports_columns() {
  // A valid SELECT that returns zero rows still has a column schema
  // (`SELECT ... WHERE 0`). The headers must survive an empty result so the
  // table render can tell "empty with columns" from "no columns".
  let driver = memory_driver().await;
  let result = driver
    .query("select 1 as n, 'x' as label where 0")
    .await
    .expect("query empty select");
  assert!(result.rows.is_empty());
  assert_eq!(result.columns.len(), 2);
  assert_eq!(result.columns[0].name, "n");
  assert_eq!(result.columns[1].name, "label");
}
