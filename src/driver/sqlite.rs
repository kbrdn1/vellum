//! The SQLite `Driver` — the first and only impl for Phase 0 (sqlx, bundled
//! libsqlite3, in-process). Maps SQLite's five storage classes (NULL /
//! INTEGER / REAL / TEXT / BLOB) onto the normalised `Value`.

use std::path::Path;
use std::str::FromStr;

use async_trait::async_trait;
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection, SqlitePool, SqliteRow};
// Trait methods are imported anonymously to avoid colliding with the domain
// `Column` / `Row` types.
use sqlx::{
  AssertSqlSafe, Column as _, Executor as _, Row as _, SqlSafeStr as _, Statement as _, TypeInfo as _, ValueRef as _,
};

use sqlparser::dialect::SQLiteDialect;

use crate::driver::{ensure_single_read_query, Capabilities, Driver};
use crate::error::{Result, VellumError};
// `catalog` is module-qualified so its `Column` doesn't clash with the result
// `Column` imported flat below.
use crate::model::catalog;
use crate::model::{Backend, Column, QueryResult, Row, TypeKind, Value};

/// A connection to a SQLite database, backed by a sqlx pool.
pub struct SqliteDriver {
  pool: SqlitePool,
}

impl SqliteDriver {
  /// Open a **read-only** connection to a SQLite database file by path. Unlike
  /// [`Driver::connect`] — which parses a `sqlite:` DSN as a URI — the path is
  /// handed to sqlx verbatim via `.filename`, so a name with URL
  /// metacharacters (`?`, `%`, `#`) or one that looks like a DSN (`:memory:`,
  /// `file:…`) opens the literal file named instead of being reinterpreted as a
  /// connection URI. Read-only is enforced exactly as in `connect`
  /// (`SQLITE_OPEN_READONLY`, unbypassable from SQL).
  pub async fn open_readonly(path: &Path) -> Result<Self> {
    let options = SqliteConnectOptions::new().filename(path).read_only(true);
    let pool = SqlitePool::connect_with(options).await.map_err(driver_err)?;
    Ok(Self { pool })
  }

  /// Introspect the connected database into the pure [`catalog::Catalog`].
  ///
  /// Reads `sqlite_master` (tables + views, `sqlite_*` internal objects
  /// excluded) and the `pragma_*` table-valued functions (columns, PKs, FKs) on
  /// a single read transaction for a consistent snapshot. The queries go
  /// straight to the connection (not the `query` read guard) — `PRAGMA` is
  /// read-only by nature and wouldn't pass the SELECT-only check anyway. SQLite's
  /// single default schema maps to one `Database` / `Schema`, both named `main`.
  /// Backs [`Driver::introspect`] (the frozen port, #11).
  async fn introspect_catalog(&self) -> Result<catalog::Catalog> {
    // Run the whole introspection on one read transaction so the snapshot is
    // consistent — a concurrent DDL can't have a relation listed here and then
    // dropped before its columns / FKs are read. Read-only: dropping the
    // transaction rolls back (nothing is written).
    let mut tx = self.pool.begin().await.map_err(driver_err)?;

    let relation_rows = sqlx::query(
      // `GLOB 'sqlite_*'` matches the *literal* `sqlite_` prefix — unlike
      // `LIKE 'sqlite_%'`, whose `_` is a wildcard that would also drop a user
      // table like `sqlitexdata`.
      "SELECT name, type FROM sqlite_master \
       WHERE type IN ('table', 'view') AND name NOT GLOB 'sqlite_*' \
       ORDER BY name",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(driver_err)?;

    let mut relations = Vec::with_capacity(relation_rows.len());
    for row in &relation_rows {
      let name: String = row.try_get("name").map_err(driver_err)?;
      let type_name: String = row.try_get("type").map_err(driver_err)?;
      let kind = if type_name == "view" {
        catalog::RelationKind::View
      } else {
        catalog::RelationKind::Table
      };
      let columns = introspect_columns(&mut tx, &name).await?;
      let foreign_keys = introspect_foreign_keys(&mut tx, &name).await?;
      relations.push(catalog::Relation {
        name,
        kind,
        columns,
        foreign_keys,
      });
    }

    Ok(catalog::Catalog {
      databases: vec![catalog::Database {
        name: "main".to_string(),
        schemas: vec![catalog::Schema {
          name: "main".to_string(),
          relations,
        }],
      }],
    })
  }
}

/// Columns of `relation` in ordinal (`cid`) order. Uses `pragma_table_xinfo`
/// (not `table_info`) so generated columns are listed — a FK may reference one.
/// `hidden = 1` columns (virtual-table internals) are excluded; normal (0) and
/// generated (2 = virtual, 3 = stored) columns are kept.
async fn introspect_columns(conn: &mut SqliteConnection, relation: &str) -> Result<Vec<catalog::Column>> {
  let rows =
    sqlx::query("SELECT name, type, \"notnull\", pk FROM pragma_table_xinfo(?1) WHERE hidden != 1 ORDER BY cid")
      .bind(relation)
      .fetch_all(&mut *conn)
      .await
      .map_err(driver_err)?;

  let mut columns = Vec::with_capacity(rows.len());
  for row in &rows {
    let name: String = row.try_get("name").map_err(driver_err)?;
    let data_type: String = row.try_get("type").map_err(driver_err)?;
    let notnull: i64 = row.try_get("notnull").map_err(driver_err)?;
    let pk: i64 = row.try_get("pk").map_err(driver_err)?;
    columns.push(catalog::Column {
      name,
      data_type,
      // Faithful to SQLite: `nullable = (notnull == 0)` — *not* "pk implies
      // not-null". `INTEGER PRIMARY KEY` reports `notnull = 0`, and a
      // non-INTEGER SQLite `PRIMARY KEY` genuinely admits NULL.
      nullable: notnull == 0,
      primary_key: pk > 0,
    });
  }
  Ok(columns)
}

/// Foreign keys of `relation` from `pragma_foreign_key_list`. A multi-column
/// key spans several rows sharing an `id` (ordered by `seq`); they are folded
/// into one [`catalog::ForeignKey`]. An implicit target (every `to` is NULL)
/// references the parent's primary key.
async fn introspect_foreign_keys(conn: &mut SqliteConnection, relation: &str) -> Result<Vec<catalog::ForeignKey>> {
  let rows = sqlx::query("SELECT id, \"table\", \"from\", \"to\" FROM pragma_foreign_key_list(?1) ORDER BY id, seq")
    .bind(relation)
    .fetch_all(&mut *conn)
    .await
    .map_err(driver_err)?;

  let mut foreign_keys: Vec<catalog::ForeignKey> = Vec::new();
  let mut current_id: Option<i64> = None;
  for row in &rows {
    let id: i64 = row.try_get("id").map_err(driver_err)?;
    let referenced: String = row.try_get("table").map_err(driver_err)?;
    let from: String = row.try_get("from").map_err(driver_err)?;
    // `to` is NULL when the FK omits its target columns — an implicit reference
    // to the parent's primary key, filled in below. Only `None` is implicit; a
    // column explicitly named `""` stays explicit.
    let to: Option<String> = row.try_get::<Option<String>, _>("to").map_err(driver_err)?;

    if current_id == Some(id) {
      if let Some(fk) = foreign_keys.last_mut() {
        fk.columns.push(from);
        fk.references.columns.extend(to);
      }
    } else {
      current_id = Some(id);
      foreign_keys.push(catalog::ForeignKey {
        name: None,
        columns: vec![from],
        references: catalog::Reference {
          schema: None,
          relation: referenced,
          columns: to.into_iter().collect(),
        },
      });
    }
  }

  for fk in &mut foreign_keys {
    if fk.references.columns.is_empty() {
      let parent = fk.references.relation.clone();
      fk.references.columns = primary_key_columns(conn, &parent).await?;
    }
  }
  Ok(foreign_keys)
}

/// The primary-key column names of `relation`, ordered by their position in the
/// key (`pragma_table_info.pk`).
async fn primary_key_columns(conn: &mut SqliteConnection, relation: &str) -> Result<Vec<String>> {
  let rows = sqlx::query("SELECT name FROM pragma_table_info(?1) WHERE pk > 0 ORDER BY pk")
    .bind(relation)
    .fetch_all(&mut *conn)
    .await
    .map_err(driver_err)?;
  let mut names = Vec::with_capacity(rows.len());
  for row in &rows {
    names.push(row.try_get::<String, _>("name").map_err(driver_err)?);
  }
  Ok(names)
}

#[async_trait]
impl Driver for SqliteDriver {
  async fn connect(dsn: &str) -> Result<Self> {
    // The read path opens its connections read-only (SQLITE_OPEN_READONLY), so
    // a mutating statement is refused by SQLite itself. Unlike `PRAGMA
    // query_only`, this can't be undone from SQL (`PRAGMA query_only=OFF` or a
    // multi-statement payload) — the underlying file handle is read-only.
    // Intentional writes go through the gated write/diff path (a later, sacred
    // phase — tracked by #64).
    let options = SqliteConnectOptions::from_str(dsn).map_err(driver_err)?.read_only(true);
    let pool = SqlitePool::connect_with(options).await.map_err(driver_err)?;
    Ok(Self { pool })
  }

  async fn query(&self, sql: &str) -> Result<QueryResult> {
    // SQLite has no data-modifying CTEs, so the single-`Query` parser guard is
    // exact here; the read-only file handle (`connect`) is the backstop.
    ensure_single_read_query(&SQLiteDialect {}, sql)?;
    let raw_rows = sqlx::query(AssertSqlSafe(sql))
      .fetch_all(&self.pool)
      .await
      .map_err(driver_err)?;

    // Map every cell first — the runtime value type is the single reliable
    // source of type info (SQLite reports no decltype for literal columns).
    let mut rows: Vec<Row> = Vec::with_capacity(raw_rows.len());
    for raw in &raw_rows {
      let mut cells = Vec::with_capacity(raw.len());
      for i in 0..raw.len() {
        cells.push(value_at(raw, i)?);
      }
      rows.push(cells);
    }

    let columns = self.columns_for(sql, &raw_rows, &rows).await?;

    // `affected` is owned by the write path (a later, sacred phase); a read
    // query leaves it `None`.
    Ok(QueryResult {
      columns,
      rows,
      affected: None,
    })
  }

  async fn introspect(&self) -> Result<catalog::Catalog> {
    self.introspect_catalog().await
  }

  fn backend(&self) -> Backend {
    Backend::Sqlite
  }

  fn capabilities(&self) -> Capabilities {
    // SQLite: `EXPLAIN QUERY PLAN`; a single schema (no `schemas`); foreign keys
    // are declarable and introspected (`pragma_foreign_key_list`).
    Capabilities {
      explain: true,
      schemas: false,
      foreign_keys: true,
    }
  }
}

impl SqliteDriver {
  /// Build the result columns. Names come from the row metadata (or the
  /// prepared statement for an empty result). A column's kind is the first
  /// **non-null** cell's runtime type — a nullable column's first row is often
  /// NULL, so reading row 0 alone is wrong. Columns with no non-null cell (and
  /// empty results) fall back to the declared affinity from the prepared
  /// statement's column metadata.
  async fn columns_for(&self, sql: &str, raw_rows: &[SqliteRow], rows: &[Row]) -> Result<Vec<Column>> {
    let runtime: Vec<Option<TypeKind>> = match raw_rows.first() {
      Some(meta) => (0..meta.len()).map(|i| first_non_null_kind(rows, i)).collect(),
      None => Vec::new(),
    };
    // Only prepare when a declared affinity is actually needed — an empty
    // result, or a column that is entirely NULL. `prepare` inspects the
    // statement's metadata without executing it (safe on the read path).
    let prepared = if raw_rows.is_empty() || runtime.iter().any(Option::is_none) {
      Some(
        (&self.pool)
          .prepare(AssertSqlSafe(sql).into_sql_str())
          .await
          .map_err(driver_err)?,
      )
    } else {
      None
    };
    let affinity = |i: usize| {
      prepared
        .as_ref()
        .and_then(|p| p.columns().get(i))
        .map_or(TypeKind::Null, |c| typekind_from_class(c.type_info().name()))
    };

    Ok(match raw_rows.first() {
      Some(meta) => meta
        .columns()
        .iter()
        .enumerate()
        .map(|(i, c)| Column {
          name: c.name().to_string(),
          kind: runtime[i].unwrap_or_else(|| affinity(i)),
        })
        .collect(),
      // Empty result: `prepared` is `Some` (we needed it above) — headers
      // survive (e.g. `SELECT a, b WHERE 0`) with their declared affinity.
      None => prepared.as_ref().map_or_else(Vec::new, |p| {
        p.columns()
          .iter()
          .map(|c| Column {
            name: c.name().to_string(),
            kind: typekind_from_class(c.type_info().name()),
          })
          .collect()
      }),
    })
  }
}

/// Decode the `i`th cell of a SQLite row into a normalised `Value`, by its
/// runtime storage class. `NULL` is checked before the type so a null in a
/// typed column still maps to `Value::Null`.
fn value_at(row: &SqliteRow, i: usize) -> Result<Value> {
  let raw = row.try_get_raw(i).map_err(driver_err)?;
  if raw.is_null() {
    return Ok(Value::Null);
  }
  let class = raw.type_info().name().to_string();
  let value = match class.as_str() {
    "INTEGER" => Value::Int(row.try_get::<i64, _>(i).map_err(driver_err)?),
    "REAL" => Value::Float(row.try_get::<f64, _>(i).map_err(driver_err)?),
    "TEXT" => Value::Text(row.try_get::<String, _>(i).map_err(driver_err)?),
    "BLOB" => Value::Bytes(row.try_get::<Vec<u8>, _>(i).map_err(driver_err)?),
    other => {
      return Err(VellumError::Driver(format!(
        "unsupported SQLite storage class: {other}"
      )))
    }
  };
  Ok(value)
}

/// The kind of the first non-null cell in column `i` across `rows`, or `None`
/// if every cell is null. A nullable column's first row is often NULL, so the
/// kind can't be read off row 0 alone.
fn first_non_null_kind(rows: &[Row], i: usize) -> Option<TypeKind> {
  rows
    .iter()
    .map(|row| &row[i])
    .find(|value| !matches!(value, Value::Null))
    .map(Value::kind)
}

/// Map a SQLite storage-class / declared-affinity name to a `TypeKind`. Used
/// for column headers when there are no rows to infer from; unknown or literal
/// affinities fall back to `Null`.
fn typekind_from_class(name: &str) -> TypeKind {
  match name {
    "INTEGER" => TypeKind::Int,
    "REAL" => TypeKind::Float,
    "TEXT" => TypeKind::Text,
    "BLOB" => TypeKind::Bytes,
    _ => TypeKind::Null,
  }
}

fn driver_err(e: sqlx::Error) -> VellumError {
  VellumError::Driver(e.to_string())
}
