//! Integration tests for the `Driver` port against an in-process SQLite
//! database (sqlx, file-backed tempfile opened read-only — no external
//! service, CI-friendly). Exercises connect → query → `QueryResult`, the
//! SQLite-storage-class → `Value` mapping (the type seam from #4), and the
//! read-only boundary the read path enforces.

use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::Executor as _;
use tempfile::NamedTempFile;
use vellum::driver::{Capabilities, Driver, SqliteDriver};
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
  assert_eq!(driver.backend(), Backend::Sqlite);
  assert_eq!(result.columns.len(), 1);
  assert_eq!(result.rows.len(), 1);
  assert_eq!(result.rows[0][0], Value::Int(1));
  assert_eq!(result.affected, None);
}

#[tokio::test]
async fn sqlite_capabilities() {
  // The frozen port's per-backend feature gate: SQLite has EXPLAIN and foreign
  // keys, but a single schema (no `schemas` level in the sidebar).
  let (driver, _db) = seeded_driver().await;
  assert_eq!(
    driver.capabilities(),
    Capabilities {
      explain: true,
      schemas: false,
      foreign_keys: true,
    }
  );
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
  assert_eq!(driver.backend(), Backend::Sqlite);
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

#[tokio::test]
async fn sqlite_introspection_keeps_user_tables_prefixed_like_sqlite() {
  // `NOT LIKE 'sqlite_%'` would wrongly drop `sqlitexdata` — `_` is a LIKE
  // wildcard, so it matches the `x`. Only the *literal* `sqlite_` internal
  // tables must be excluded.
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
    .execute("create table sqlitexdata (id integer)")
    .await
    .expect("seed a table whose name starts with `sqlite`");
  setup.close().await;
  let driver = SqliteDriver::connect(&dsn).await.expect("connect read-only");

  let catalog = driver.introspect().await.expect("introspect the schema");
  let schema = catalog.database("main").unwrap().schema("main").unwrap();
  assert!(
    schema.relation("sqlitexdata").is_some(),
    "a user table named like `sqlite...` (no literal `sqlite_`) must not be dropped"
  );
}

#[tokio::test]
async fn sqlite_introspection_includes_generated_columns() {
  // `pragma_table_info` omits generated columns, but a FK can reference one —
  // so the catalog must list them (`pragma_table_xinfo`). Internal hidden
  // columns stay excluded.
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
    .execute("create table widget (w integer, h integer, area integer generated always as (w * h) stored)")
    .await
    .expect("seed a table with a generated column");
  setup.close().await;
  let driver = SqliteDriver::connect(&dsn).await.expect("connect read-only");

  let catalog = driver.introspect().await.expect("introspect the schema");
  let widget = catalog
    .database("main")
    .unwrap()
    .schema("main")
    .unwrap()
    .relation("widget")
    .expect("relation `widget`");
  let names: Vec<&str> = widget.columns.iter().map(|c| c.name.as_str()).collect();
  assert!(
    names.contains(&"area"),
    "the generated column `area` must be listed, got {names:?}"
  );
}

#[tokio::test]
async fn sqlite_introspection_excludes_hidden_virtual_table_columns() {
  // `pragma_table_xinfo` lists a virtual table's internal columns with
  // `hidden = 1` — an fts5 table exposes a column named after the table and a
  // `rank` column, both hidden. Those must be excluded; only the declared
  // columns (`hidden = 0`) and generated ones (`hidden = 2/3`) belong in the
  // catalog. This pins the `WHERE hidden != 1` filter against regression.
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
    .execute("create virtual table docs using fts5(title, body)")
    .await
    .expect("seed an fts5 virtual table");
  setup.close().await;
  let driver = SqliteDriver::connect(&dsn).await.expect("connect read-only");

  let catalog = driver.introspect().await.expect("introspect the schema");
  let docs = catalog
    .database("main")
    .unwrap()
    .schema("main")
    .unwrap()
    .relation("docs")
    .expect("relation `docs`");
  let names: Vec<&str> = docs.columns.iter().map(|c| c.name.as_str()).collect();
  // The two declared columns survive; the hidden `docs` / `rank` internals must
  // not leak into the catalog.
  assert_eq!(
    names,
    ["title", "body"],
    "only the declared fts5 columns are listed (hidden internals excluded), got {names:?}"
  );
}

#[tokio::test]
async fn sqlite_introspection_keeps_an_explicit_empty_named_target_column() {
  // A foreign key can name a target column that is the empty string
  // (`references parent("")`). `pragma_foreign_key_list` reports `to = ''`
  // (text) — distinct from the NULL of an *implicit* target. The empty name
  // must stay an explicit reference: never folded to the parent's primary key
  // the way an implicit (NULL) target is. This pins the `to: Option<String>`
  // read (only NULL is implicit) against regression.
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
    // `parent` has NO primary key — only a unique column named "". If the empty
    // target were mistaken for NULL (implicit), it would resolve to the parent
    // PK and come back empty; staying explicit keeps the `""` column name.
    "create table parent(\"\" integer unique)",
    "create table child(x integer references parent(\"\"))",
  ] {
    setup.execute(stmt).await.expect("seed schema");
  }
  setup.close().await;
  let driver = SqliteDriver::connect(&dsn).await.expect("connect read-only");

  let catalog = driver.introspect().await.expect("introspect the schema");
  let child = catalog
    .database("main")
    .unwrap()
    .schema("main")
    .unwrap()
    .relation("child")
    .expect("relation `child`");
  assert_eq!(child.foreign_keys.len(), 1);
  let fk = &child.foreign_keys[0];
  assert_eq!(fk.columns, ["x"]);
  assert_eq!(fk.references.relation, "parent");
  // Explicit empty-string target — NOT the implicit-target fallback (which
  // would resolve to the parent PK and, with no PK here, come back empty).
  assert_eq!(fk.references.columns, [""]);
}

/// PostgreSQL integration tests — behind the `it-db` feature so the default
/// `cargo test` stays on in-memory SQLite (no Docker). Run with:
///
/// ```bash
/// VELLUM_IT_PG_DSN=postgres://postgres@localhost:5432/postgres \
///   cargo test --features it-db --test driver_tests postgres_it
/// ```
///
/// Each test seeds through a **separate writable** `sqlx` pool (the read-only
/// `PostgresDriver` would refuse the DDL/INSERT) and uses a uniquely-named
/// table, so the suite is safe to run concurrently against one server.
#[cfg(feature = "it-db")]
mod postgres_it {
  use sqlx::postgres::PgPoolOptions;
  use sqlx::{Executor as _, PgPool};
  use vellum::driver::{Capabilities, Driver, PostgresDriver};
  use vellum::model::catalog::RelationKind;
  use vellum::model::{Backend, Value};

  /// The PG DSN — from `VELLUM_IT_PG_DSN`, or a standard local default so the
  /// CI `it-db` job (service on `localhost:5432`) needs no extra env.
  fn dsn() -> String {
    std::env::var("VELLUM_IT_PG_DSN").unwrap_or_else(|_| "postgres://postgres@localhost:5432/postgres".to_string())
  }

  /// A writable pool for seeding — deliberately NOT the read-only driver.
  async fn seed_pool() -> PgPool {
    PgPoolOptions::new()
      .max_connections(1)
      .connect(&dsn())
      .await
      .expect("connect a writable seed pool")
  }

  #[tokio::test]
  async fn pg_connect_then_select_maps_types() {
    let pool = seed_pool().await;
    pool.execute("drop table if exists it_types").await.expect("drop");
    pool
      .execute(
        "create table it_types (
           b bool, i2 int2, i4 int4, i8 int8, f4 float4, f8 float8,
           t text, by bytea, j jsonb, u uuid, ts timestamptz, arr int4[]
         )",
      )
      .await
      .expect("create table");
    pool
      .execute(
        "insert into it_types values (
           true, 1, 2, 3, 1.5, 2.5,
           'hello', '\\xdeadbeef', '{\"k\":1}',
           '00000000-0000-0000-0000-000000000001',
           '2024-01-02 03:04:05+00', '{10,20,30}'
         )",
      )
      .await
      .expect("seed row");

    let driver = PostgresDriver::connect(&dsn()).await.expect("connect read-only");
    assert_eq!(driver.backend(), Backend::Postgres);

    let result = driver.query("select * from it_types").await.expect("query it_types");
    assert_eq!(result.rows.len(), 1);
    let row = &result.rows[0];
    // Native scalars decode faithfully.
    assert_eq!(row[0], Value::Bool(true));
    assert_eq!(row[1], Value::Int(1));
    assert_eq!(row[2], Value::Int(2));
    assert_eq!(row[3], Value::Int(3));
    assert_eq!(row[4], Value::Float(1.5));
    assert_eq!(row[5], Value::Float(2.5));
    assert_eq!(row[6], Value::Text("hello".into()));
    assert_eq!(row[7], Value::Bytes(vec![0xde, 0xad, 0xbe, 0xef]));
    // json/jsonb → Json; uuid → Text; timestamptz → Timestamp (conservative,
    // into the existing variants — formats asserted loosely).
    assert!(
      matches!(&row[8], Value::Json(s) if s.contains("\"k\"")),
      "jsonb → Json, got {:?}",
      row[8]
    );
    assert_eq!(row[9], Value::Text("00000000-0000-0000-0000-000000000001".into()));
    assert!(
      matches!(&row[10], Value::Timestamp(s) if s.contains("2024")),
      "timestamptz → Timestamp, got {:?}",
      row[10]
    );
    // int4[] is the conservative long tail (#76): an honest non-data marker,
    // never a fake value.
    assert!(
      matches!(&row[11], Value::Text(s) if s.starts_with('<')),
      "array → marker, got {:?}",
      row[11]
    );
  }

  #[tokio::test]
  async fn pg_query_refuses_a_data_modifying_cte() {
    // THE write-path guard for PG: a data-modifying CTE parses as a single
    // top-level `SELECT` (the sqlparser guard waves it through) but it WRITES.
    // The read-only session backstop (`default_transaction_read_only = on`)
    // must refuse it, and nothing may be inserted.
    let pool = seed_pool().await;
    pool.execute("drop table if exists it_cte_guard").await.expect("drop");
    pool
      .execute("create table it_cte_guard (x int)")
      .await
      .expect("create table");

    let driver = PostgresDriver::connect(&dsn()).await.expect("connect read-only");
    let outcome = driver
      .query("with t as (insert into it_cte_guard values (1) returning *) select * from t")
      .await;
    assert!(
      outcome.is_err(),
      "a data-modifying CTE must be refused on the read path"
    );

    let count: i64 = sqlx::query_scalar("select count(*) from it_cte_guard")
      .fetch_one(&pool)
      .await
      .expect("count rows");
    assert_eq!(count, 0, "the refused CTE must not have inserted a row");
  }

  #[tokio::test]
  async fn pg_query_refuses_a_plain_write() {
    // The shared parser guard, wired with the PG dialect, refuses a plain
    // write before it reaches the server (the read-only session is a backstop).
    let driver = PostgresDriver::connect(&dsn()).await.expect("connect read-only");
    assert!(
      driver.query("create table it_plain_ddl (x int)").await.is_err(),
      "DDL must be refused on the read path"
    );
    assert!(
      driver.query("delete from it_types").await.is_err(),
      "a DELETE must be refused on the read path"
    );
  }

  #[tokio::test]
  async fn pg_read_only_survives_a_set_config_poison() {
    // The session `default_transaction_read_only` is bypassable on its own: a
    // SELECT can flip it — `select set_config('default_transaction_read_only',
    // 'off', false)` parses as a `Query`, passes the guard, and turns the
    // session default OFF — and pooled connections are reused, so a later query
    // inherits the poisoned session. The read path must therefore run each
    // query in its own transaction-level READ ONLY, which a single statement
    // can't undo. Without that, the data-modifying CTE below writes.
    let pool = seed_pool().await;
    pool
      .execute("drop table if exists it_poison_guard")
      .await
      .expect("drop");
    pool
      .execute("create table it_poison_guard (x int)")
      .await
      .expect("create table");

    let driver = PostgresDriver::connect(&dsn()).await.expect("connect read-only");

    // Poison the session: disable the read-only default.
    let _ = driver
      .query("select set_config('default_transaction_read_only', 'off', false)")
      .await;

    // The follow-up write (reusing the poisoned pooled connection) must STILL be
    // refused.
    let outcome = driver
      .query("with t as (insert into it_poison_guard values (1) returning *) select * from t")
      .await;
    assert!(
      outcome.is_err(),
      "read-only must survive a set_config poison — the write must be refused"
    );

    let count: i64 = sqlx::query_scalar("select count(*) from it_poison_guard")
      .fetch_one(&pool)
      .await
      .expect("count rows");
    assert_eq!(count, 0, "no row may be written after a poison attempt");
  }

  #[tokio::test]
  async fn pg_capabilities() {
    // Postgres is the backend with real schemas — the frozen capability gates
    // the sidebar's schema level on.
    let driver = PostgresDriver::connect(&dsn()).await.expect("connect read-only");
    assert_eq!(
      driver.capabilities(),
      Capabilities {
        explain: true,
        schemas: true,
        foreign_keys: true,
      }
    );
  }

  #[tokio::test]
  async fn pg_introspection_returns_schema_tables_columns_pk_fk() {
    let pool = seed_pool().await;
    // A dedicated schema isolates this test (Postgres has real schemas).
    pool
      .execute("drop schema if exists vellum_it_pgcat cascade")
      .await
      .expect("drop schema");
    pool
      .execute("create schema vellum_it_pgcat")
      .await
      .expect("create schema");
    pool
      .execute("create table vellum_it_pgcat.users (id int primary key, email text not null, bio text)")
      .await
      .expect("create users");
    pool
      .execute(
        "create table vellum_it_pgcat.orders (id int primary key, \
         user_id int not null references vellum_it_pgcat.users(id), total double precision)",
      )
      .await
      .expect("create orders");
    pool
      .execute("create view vellum_it_pgcat.recent as select id, user_id from vellum_it_pgcat.orders")
      .await
      .expect("create view");

    let driver = PostgresDriver::connect(&dsn()).await.expect("connect read-only");
    let catalog = driver.introspect().await.expect("introspect the schema");
    let db = catalog.databases.first().expect("one database");
    let schema = db.schema("vellum_it_pgcat").expect("schema vellum_it_pgcat");

    let users = schema.relation("users").expect("relation users");
    assert_eq!(users.kind, RelationKind::Table);
    assert!(users.column("id").expect("id").primary_key, "id is the PK");
    assert!(!users.column("email").expect("email").nullable, "email is NOT NULL");
    assert!(users.column("bio").expect("bio").nullable, "bio admits NULL");

    let recent = schema.relation("recent").expect("relation recent");
    assert_eq!(recent.kind, RelationKind::View);

    let orders = schema.relation("orders").expect("relation orders");
    assert_eq!(orders.foreign_keys.len(), 1);
    let fk = &orders.foreign_keys[0];
    assert_eq!(fk.columns, ["user_id"]);
    assert_eq!(fk.references.relation, "users");
    assert_eq!(fk.references.columns, ["id"]);
    // The FK resolves (same-schema reference carries its schema).
    let target = db.resolve(fk, "vellum_it_pgcat").expect("the FK resolves");
    assert_eq!(target.name, "users");
  }

  #[tokio::test]
  async fn pg_introspection_keeps_user_schemas_prefixed_like_pg() {
    // `pgvellum_it` matches a wildcard `pg_%` (the `_` is a LIKE wildcard) but is NOT a
    // reserved `pg_` schema — it must survive (the exclusion escapes the `_`).
    let pool = seed_pool().await;
    pool
      .execute("drop schema if exists pgvellum_it cascade")
      .await
      .expect("drop schema");
    pool.execute("create schema pgvellum_it").await.expect("create schema");
    pool
      .execute("create table pgvellum_it.t (id int primary key)")
      .await
      .expect("create table");

    let driver = PostgresDriver::connect(&dsn()).await.expect("connect read-only");
    let catalog = driver.introspect().await.expect("introspect the schema");
    let db = catalog.databases.first().expect("one database");
    assert!(
      db.schema("pgvellum_it").is_some(),
      "a user schema named like `pg...` (no literal `pg_`) must not be dropped"
    );
  }

  #[tokio::test]
  async fn pg_introspection_pk_does_not_leak_across_same_named_constraints() {
    // A PK on one table and a FK on another can share a constraint name in one
    // schema (the PK backs a schema-unique index; the FK does not). Both then
    // appear in `key_column_usage` under that name — so introspecting the PK
    // table must constrain the join to that table, or the FK table's column
    // leaks into the PK set and a homonym is wrongly flagged.
    let pool = seed_pool().await;
    pool
      .execute("drop schema if exists vellum_it_pkdup cascade")
      .await
      .expect("drop schema");
    pool
      .execute("create schema vellum_it_pkdup")
      .await
      .expect("create schema");
    // t_a: PK `pk_col` (constraint `shared_name`) plus a plain `other_col`.
    pool
      .execute("create table vellum_it_pkdup.t_a (pk_col int constraint shared_name primary key, other_col int)")
      .await
      .expect("create t_a");
    // t_b: a FK *also* named `shared_name`, on a column named `other_col`.
    pool
      .execute(
        "create table vellum_it_pkdup.t_b (other_col int, \
         constraint shared_name foreign key (other_col) references vellum_it_pkdup.t_a(pk_col))",
      )
      .await
      .expect("create t_b");

    let driver = PostgresDriver::connect(&dsn()).await.expect("connect read-only");
    let catalog = driver.introspect().await.expect("introspect the schema");
    let schema = catalog
      .databases
      .first()
      .unwrap()
      .schema("vellum_it_pkdup")
      .expect("schema vellum_it_pkdup");
    let t_a = schema.relation("t_a").expect("relation t_a");
    assert!(
      t_a.column("pk_col").expect("column pk_col").primary_key,
      "t_a.pk_col is the primary key"
    );
    assert!(
      !t_a.column("other_col").expect("column other_col").primary_key,
      "t_a.other_col must NOT be flagged PK — it is t_b's FK column under the same constraint name"
    );
  }

  #[tokio::test]
  async fn pg_a_write_hidden_in_a_subquery_lands_nothing() {
    // The parser guard is best-effort; the per-query READ ONLY transaction is
    // the guarantee. A data-modifying CTE buried in a derived table can't be a
    // top-level write — Postgres refuses it (`must be at the top level`) — so
    // even if the parser waves it through, nothing is written.
    let pool = seed_pool().await;
    pool
      .execute("drop table if exists vellum_it_hidden")
      .await
      .expect("drop");
    pool
      .execute("create table vellum_it_hidden (x int)")
      .await
      .expect("create table");

    let driver = PostgresDriver::connect(&dsn()).await.expect("connect read-only");
    let outcome = driver
      .query("select * from (with w as (insert into vellum_it_hidden values (1) returning *) select * from w) s")
      .await;
    assert!(outcome.is_err(), "a write hidden in a subquery must be refused");

    let count: i64 = sqlx::query_scalar("select count(*) from vellum_it_hidden")
      .fetch_one(&pool)
      .await
      .expect("count rows");
    assert_eq!(count, 0, "no row may be written by a hidden subquery write");
  }
}

/// MySQL integration tests — behind the `it-db` feature (default `cargo test`
/// stays on in-memory SQLite, no Docker). Run with:
///
/// ```bash
/// VELLUM_IT_MYSQL_DSN=mysql://root@localhost:3306/testdb \
///   cargo test --features it-db --test driver_tests mysql_it
/// ```
///
/// Seeded through a separate writable pool; uniquely-named tables.
#[cfg(feature = "it-db")]
mod mysql_it {
  use sqlx::mysql::MySqlPoolOptions;
  use sqlx::{Executor as _, MySqlPool};
  use vellum::driver::{Capabilities, Driver, MySqlDriver};
  use vellum::model::catalog::RelationKind;
  use vellum::model::{Backend, Value};

  fn dsn() -> String {
    std::env::var("VELLUM_IT_MYSQL_DSN").unwrap_or_else(|_| "mysql://root@localhost:3306/testdb".to_string())
  }

  async fn seed_pool() -> MySqlPool {
    MySqlPoolOptions::new()
      .max_connections(1)
      .connect(&dsn())
      .await
      .expect("connect a writable seed pool")
  }

  #[tokio::test]
  async fn mysql_connect_then_select_maps_types() {
    let pool = seed_pool().await;
    pool.execute("drop table if exists it_types").await.expect("drop");
    pool
      .execute(
        "create table it_types (
           i_tiny tinyint, i_int int, i_big bigint,
           f_float float, f_double double,
           t_text text, t_varchar varchar(50), b_blob blob,
           j json, dt datetime
         )",
      )
      .await
      .expect("create table");
    pool
      .execute(
        "insert into it_types values (
           1, 2, 3, 1.5, 2.5, 'hello', 'world', x'deadbeef',
           '{\"k\":1}', '2024-01-02 03:04:05'
         )",
      )
      .await
      .expect("seed row");

    let driver = MySqlDriver::connect(&dsn()).await.expect("connect read-only");
    assert_eq!(driver.backend(), Backend::MySql);

    let result = driver.query("select * from it_types").await.expect("query it_types");
    assert_eq!(result.rows.len(), 1);
    let row = &result.rows[0];
    assert_eq!(row[0], Value::Int(1));
    assert_eq!(row[1], Value::Int(2));
    assert_eq!(row[2], Value::Int(3));
    assert_eq!(row[3], Value::Float(1.5));
    assert_eq!(row[4], Value::Float(2.5));
    assert_eq!(row[5], Value::Text("hello".into()));
    assert_eq!(row[6], Value::Text("world".into()));
    assert_eq!(row[7], Value::Bytes(vec![0xde, 0xad, 0xbe, 0xef]));
    assert!(
      matches!(&row[8], Value::Json(s) if s.contains("\"k\"")),
      "json → Json, got {:?}",
      row[8]
    );
    assert!(
      matches!(&row[9], Value::Timestamp(s) if s.contains("2024")),
      "datetime → Timestamp, got {:?}",
      row[9]
    );
  }

  #[tokio::test]
  async fn mysql_query_refuses_a_plain_write() {
    let driver = MySqlDriver::connect(&dsn()).await.expect("connect read-only");
    assert!(
      driver.query("create table it_plain_ddl (x int)").await.is_err(),
      "DDL must be refused on the read path"
    );
    assert!(
      driver
        .query("insert into it_types values (1,2,3,1.5,2.5,'a','b',x'00','{}','2024-01-01 00:00:00')")
        .await
        .is_err(),
      "an INSERT must be refused on the read path"
    );
  }

  #[tokio::test]
  async fn mysql_read_only_tx_refuses_a_writing_function() {
    // `SELECT writing_func()` parses as a `Query` (passes the parser guard) but
    // the function writes — the per-query READ ONLY transaction must refuse it
    // and insert nothing. This is MySQL's guard-passing write vector (it has no
    // data-modifying CTE; `INTO OUTFILE` is rejected at the parser).
    let pool = seed_pool().await;
    pool.execute("drop table if exists it_wf_guard").await.expect("drop");
    pool
      .execute("create table it_wf_guard (x int)")
      .await
      .expect("create table");
    // A data-modifying stored function needs the binlog trust flag — a *global*
    // server setting. Save it and restore it afterwards so the test leaves no
    // trace on a shared server.
    let original_trust: i64 = sqlx::query_scalar("select @@global.log_bin_trust_function_creators")
      .fetch_one(&pool)
      .await
      .expect("read the trust flag");
    pool
      .execute("set global log_bin_trust_function_creators = 1")
      .await
      .expect("set trust");
    // Everything below the SET GLOBAL may fail; capture results instead of
    // panicking so the global is restored no matter what.
    let result: Result<(bool, i64), String> = async {
      pool
        .execute("drop function if exists it_wf")
        .await
        .map_err(|e| e.to_string())?;
      pool
        .execute(
          "create function it_wf() returns int modifies sql data \
           begin insert into it_wf_guard values (1); return 1; end",
        )
        .await
        .map_err(|e| e.to_string())?;
      let driver = MySqlDriver::connect(&dsn()).await.map_err(|e| e.to_string())?;
      let refused = driver.query("select it_wf()").await.is_err();
      let count: i64 = sqlx::query_scalar("select count(*) from it_wf_guard")
        .fetch_one(&pool)
        .await
        .map_err(|e| e.to_string())?;
      Ok((refused, count))
    }
    .await;

    // Restore the global flag ALWAYS, before unwrapping the captured result.
    pool
      .execute(format!("set global log_bin_trust_function_creators = {original_trust}").as_str())
      .await
      .expect("restore the trust flag");

    let (refused, count) = result.expect("writing-function setup + check");
    assert!(
      refused,
      "a writing function via SELECT must be refused on the read path"
    );
    assert_eq!(count, 0, "the refused function must not have inserted a row");
  }

  #[tokio::test]
  async fn mysql_introspection_returns_tables_columns_pk_fk() {
    let pool = seed_pool().await;
    pool
      .execute("drop table if exists it_orders")
      .await
      .expect("drop orders");
    pool.execute("drop table if exists it_users").await.expect("drop users");
    pool.execute("drop view if exists it_recent").await.expect("drop view");
    pool
      .execute("create table it_users (id int primary key, email varchar(255) not null, bio text)")
      .await
      .expect("create users");
    pool
      .execute(
        "create table it_orders (id int primary key, \
         user_id int not null, total double, \
         constraint fk_user foreign key (user_id) references it_users(id))",
      )
      .await
      .expect("create orders");
    pool
      .execute("create view it_recent as select id, user_id from it_orders")
      .await
      .expect("create view");

    let driver = MySqlDriver::connect(&dsn()).await.expect("connect read-only");
    let catalog = driver.introspect().await.expect("introspect the schema");
    let db = catalog.databases.first().expect("one database");
    let schema = db.schemas.first().expect("one schema");

    let users = schema.relation("it_users").expect("relation it_users");
    assert_eq!(users.kind, RelationKind::Table);
    let id = users.column("id").expect("column id");
    assert!(id.primary_key, "id is the primary key");
    let email = users.column("email").expect("column email");
    assert!(!email.nullable, "email is NOT NULL");
    let bio = users.column("bio").expect("column bio");
    assert!(bio.nullable, "bio admits NULL");

    let recent = schema.relation("it_recent").expect("relation it_recent");
    assert_eq!(recent.kind, RelationKind::View);

    let orders = schema.relation("it_orders").expect("relation it_orders");
    assert_eq!(orders.foreign_keys.len(), 1);
    let fk = &orders.foreign_keys[0];
    assert_eq!(fk.columns, ["user_id"]);
    assert_eq!(fk.references.relation, "it_users");
    assert_eq!(fk.references.columns, ["id"]);
    // A same-database FK still carries its schema (the connected db) — not
    // `None` (guards the cross-db schema carry against regression). Derived from
    // the introspected db, so it holds whatever database the DSN points at.
    assert_eq!(fk.references.schema.as_deref(), Some(db.name.as_str()));
  }

  #[tokio::test]
  async fn mysql_capabilities() {
    // MySQL: EXPLAIN and foreign keys, but database = schema (no schema level).
    let driver = MySqlDriver::connect(&dsn()).await.expect("connect read-only");
    assert_eq!(
      driver.capabilities(),
      Capabilities {
        explain: true,
        schemas: false,
        foreign_keys: true,
      }
    );
  }

  #[tokio::test]
  async fn mysql_time_column_maps_to_a_marker_not_a_crash() {
    // MySQL `TIME` is a duration (here `838:59:59`, past wall-clock midnight),
    // which a `time::Time` decode can't hold — it must map to the conservative
    // marker, never fail the whole query.
    let pool = seed_pool().await;
    pool.execute("drop table if exists it_time").await.expect("drop");
    pool
      .execute("create table it_time (d time)")
      .await
      .expect("create table");
    pool
      .execute("insert into it_time values ('838:59:59')")
      .await
      .expect("seed an out-of-wall-clock TIME");

    let driver = MySqlDriver::connect(&dsn()).await.expect("connect read-only");
    let result = driver
      .query("select d from it_time")
      .await
      .expect("query must not fail on TIME");
    assert!(
      matches!(&result.rows[0][0], Value::Text(s) if s.starts_with('<')),
      "TIME → conservative marker, got {:?}",
      result.rows[0][0]
    );
  }

  #[tokio::test]
  async fn mysql_introspection_carries_a_cross_database_fk_schema() {
    // A MySQL foreign key can reference a table in another database (= schema).
    // The reference must carry that schema, not silently claim same-schema.
    let pool = seed_pool().await;
    pool.execute("drop table if exists it_child").await.expect("drop child");
    pool
      .execute("drop database if exists vellum_it_xdb")
      .await
      .expect("drop other db");
    pool
      .execute("create database vellum_it_xdb")
      .await
      .expect("create other db");
    pool
      .execute("create table vellum_it_xdb.parent (id int primary key)")
      .await
      .expect("create parent");
    pool
      .execute("create table it_child (pid int, foreign key (pid) references vellum_it_xdb.parent(id))")
      .await
      .expect("create child with a cross-db FK");

    // Introspect and extract the asserted values, capturing instead of
    // panicking so the cleanup below (which drops a database) always runs.
    let driver = MySqlDriver::connect(&dsn()).await.expect("connect read-only");
    let captured: Result<(usize, String, Option<String>), String> = async {
      let catalog = driver.introspect().await.map_err(|e| e.to_string())?;
      let schema = catalog
        .databases
        .first()
        .and_then(|d| d.schemas.first())
        .ok_or("no schema")?;
      let child = schema.relation("it_child").ok_or("relation it_child missing")?;
      let fk = child.foreign_keys.first().ok_or("no foreign key")?;
      Ok((
        child.foreign_keys.len(),
        fk.references.relation.clone(),
        fk.references.schema.clone(),
      ))
    }
    .await;

    // Clean up ALWAYS, before asserting.
    pool
      .execute("drop table if exists it_child")
      .await
      .expect("cleanup child");
    pool
      .execute("drop database if exists vellum_it_xdb")
      .await
      .expect("cleanup other db");

    let (fk_count, ref_relation, ref_schema) = captured.expect("introspect cross-db FK");
    assert_eq!(fk_count, 1);
    assert_eq!(ref_relation, "parent");
    assert_eq!(
      ref_schema.as_deref(),
      Some("vellum_it_xdb"),
      "a cross-database FK must carry the referenced schema"
    );
  }
}
