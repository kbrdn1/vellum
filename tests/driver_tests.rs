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
use vellum::model::catalog::RelationKind;
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
async fn column_kind_skips_leading_nulls() {
  // A nullable column whose first row is NULL must still report the kind of a
  // later non-null cell, not `Null`.
  let (driver, _db) = seeded_driver().await;
  let result = driver
    .query("select null as v union all select 'text' as v")
    .await
    .expect("query");
  assert_eq!(result.rows[0][0], Value::Null);
  assert_eq!(result.columns[0].kind, TypeKind::Text);
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
async fn refuses_unparseable_input() {
  // Fail closed: input sqlparser can't parse is refused, not handed to the
  // database. This covers both a chained write behind unsupported syntax
  // (`SELECT … NOT INDEXED; CREATE TEMP TABLE …`) and a single unparsed
  // statement that still writes on a read-only handle (`VACUUM INTO 'file'`,
  // which copies the db to disk — sqlparser 0.62 rejects its INTO clause).
  let (driver, _db) = seeded_driver().await;
  assert!(
    driver
      .query("select * from items not indexed; create temp table t (x integer)")
      .await
      .is_err(),
    "an unparseable multi-statement payload must be refused"
  );
  assert!(
    driver.query("vacuum into 'snapshot.db'").await.is_err(),
    "VACUUM INTO must be refused on the read path"
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

#[tokio::test]
async fn open_readonly_opens_a_file_by_literal_path() {
  // `open_readonly` takes a path (not a `sqlite:` DSN), so a filename with URL
  // metacharacters (`%`, `#`) opens the literal file rather than being parsed
  // as a connection URI. Seed such a file, then read it back by path.
  let dir = tempfile::tempdir().expect("create temp dir");
  let path = dir.path().join("data%name#1.sqlite");
  {
    let setup = SqlitePool::connect_with(SqliteConnectOptions::new().filename(&path).create_if_missing(true))
      .await
      .expect("open writable connection for seeding");
    setup
      .execute("create table items (id integer, label text)")
      .await
      .expect("create table");
    setup
      .execute("insert into items (id, label) values (1, 'alpha')")
      .await
      .expect("seed rows");
    setup.close().await;
  }

  let driver = SqliteDriver::open_readonly(&path)
    .await
    .expect("open read-only by literal path");
  let result = driver
    .query("select label from items")
    .await
    .expect("query the path-opened db");
  assert_eq!(driver.kind(), Backend::Sqlite);
  assert_eq!(result.rows.len(), 1);
  assert_eq!(result.rows[0][0], Value::Text("alpha".to_string()));
}

/// A read-only `SqliteDriver` over a tempfile seeded with a small relational
/// schema: `users` (PK, a NOT NULL and a nullable column), `orders` (a FK to
/// `users`), and a view. The `NamedTempFile` guard must outlive the driver.
async fn introspectable_driver() -> (SqliteDriver, NamedTempFile) {
  let file = NamedTempFile::new().expect("create temp db file");
  let dsn = format!("sqlite:{}", file.path().display());

  let setup = SqlitePool::connect_with(
    SqliteConnectOptions::from_str(&dsn)
      .expect("parse dsn")
      .create_if_missing(true),
  )
  .await
  .expect("open writable connection for seeding");
  for stmt in [
    "create table users (id integer primary key, email text not null, bio text)",
    "create table orders (id integer primary key, \
       user_id integer not null references users(id), total real)",
    "create view recent_orders as select id, user_id from orders",
  ] {
    setup.execute(stmt).await.expect("seed schema");
  }
  setup.close().await;

  let driver = SqliteDriver::connect(&dsn).await.expect("connect read-only");
  (driver, file)
}

#[tokio::test]
async fn sqlite_introspection_returns_tables_columns_pk_fk() {
  let (driver, _db) = introspectable_driver().await;
  let catalog = driver.introspect().await.expect("introspect the schema");

  let db = catalog.database("main").expect("database `main`");
  let schema = db.schema("main").expect("schema `main`");

  // Tables and the view, in `sqlite_master` name order.
  let names: Vec<&str> = schema.relations.iter().map(|r| r.name.as_str()).collect();
  assert_eq!(names, ["orders", "recent_orders", "users"]);

  let users = schema.relation("users").expect("relation `users`");
  assert_eq!(users.kind, RelationKind::Table);

  let id = users.column("id").expect("column `id`");
  assert!(id.primary_key, "id is the primary key");
  assert_eq!(id.data_type.to_uppercase(), "INTEGER");

  // `email` is NOT NULL; `bio` is nullable. (The PK's own nullability is a
  // SQLite quirk — not asserted here.)
  let email = users.column("email").expect("column `email`");
  assert!(!email.nullable, "email is NOT NULL");
  assert!(!email.primary_key);
  let bio = users.column("bio").expect("column `bio`");
  assert!(bio.nullable, "bio admits NULL");

  let recent = schema.relation("recent_orders").expect("relation `recent_orders`");
  assert_eq!(recent.kind, RelationKind::View);

  // `orders.user_id` → `users.id`.
  let orders = schema.relation("orders").expect("relation `orders`");
  assert_eq!(orders.foreign_keys.len(), 1);
  let fk = &orders.foreign_keys[0];
  assert_eq!(fk.columns, ["user_id"]);
  assert_eq!(fk.references.relation, "users");
  assert_eq!(fk.references.columns, ["id"]);
  let target = db.resolve(fk, "main").expect("the FK resolves");
  assert_eq!(target.name, "users");
}

#[tokio::test]
async fn sqlite_introspection_folds_composite_and_implicit_foreign_keys() {
  // A composite FK with an *implicit* target (`references account`, no columns)
  // — `pragma_foreign_key_list` reports `to = NULL` for each row, and the
  // target is the parent's primary key.
  let file = NamedTempFile::new().expect("create temp db file");
  let dsn = format!("sqlite:{}", file.path().display());
  let setup = SqlitePool::connect_with(
    SqliteConnectOptions::from_str(&dsn)
      .expect("parse dsn")
      .create_if_missing(true),
  )
  .await
  .expect("open writable connection for seeding");
  for stmt in [
    "create table account (org_id integer, user_id integer, primary key (org_id, user_id))",
    "create table membership (org_id integer, user_id integer, role text, \
       foreign key (org_id, user_id) references account)",
  ] {
    setup.execute(stmt).await.expect("seed schema");
  }
  setup.close().await;
  let driver = SqliteDriver::connect(&dsn).await.expect("connect read-only");

  let catalog = driver.introspect().await.expect("introspect the schema");
  let db = catalog.database("main").expect("database `main`");
  let membership = db
    .schema("main")
    .unwrap()
    .relation("membership")
    .expect("relation `membership`");

  assert_eq!(membership.foreign_keys.len(), 1);
  let fk = &membership.foreign_keys[0];
  // Composite local columns folded into one key (ordered by seq).
  assert_eq!(fk.columns, ["org_id", "user_id"]);
  assert_eq!(fk.references.relation, "account");
  // Implicit target resolves to the parent's composite primary key.
  assert_eq!(fk.references.columns, ["org_id", "user_id"]);
  assert_eq!(db.resolve(fk, "main").expect("FK resolves").name, "account");
}
