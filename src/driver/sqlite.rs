//! The SQLite `Driver` — the first and only impl for Phase 0 (sqlx, bundled
//! libsqlite3, in-process). Maps SQLite's five storage classes (NULL /
//! INTEGER / REAL / TEXT / BLOB) onto the normalised `Value`.

use std::str::FromStr;

use async_trait::async_trait;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqliteRow};
// Trait methods are imported anonymously to avoid colliding with the domain
// `Column` / `Row` types.
use sqlx::{Column as _, Executor as _, Row as _, TypeInfo as _, ValueRef as _};

use sqlparser::ast::Statement;
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::Parser;

use crate::driver::Driver;
use crate::error::{Result, VellumError};
use crate::model::{Backend, Column, QueryResult, Row, TypeKind, Value};

/// A connection to a SQLite database, backed by a sqlx pool.
pub struct SqliteDriver {
  pool: SqlitePool,
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
    ensure_read_only_query(sql)?;
    let raw_rows = sqlx::query(sql).fetch_all(&self.pool).await.map_err(driver_err)?;

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

    // Column headers: names from the row metadata, kinds from the first
    // row's mapped values (empty result → no columns).
    let columns = match (raw_rows.first(), rows.first()) {
      (Some(meta), Some(first)) => meta
        .columns()
        .iter()
        .enumerate()
        .map(|(i, c)| Column {
          name: c.name().to_string(),
          kind: first[i].kind(),
        })
        .collect(),
      // No rows to infer kinds from, but a valid SELECT still has a column
      // schema (e.g. `SELECT a, b WHERE 0`) — describe the statement so the
      // headers survive. Kinds come from the declared affinity (best-effort;
      // unreliable for literal columns, hence the `Null` fallback).
      _ => {
        let described = (&self.pool).describe(sql).await.map_err(driver_err)?;
        described
          .columns()
          .iter()
          .map(|c| Column {
            name: c.name().to_string(),
            kind: typekind_from_class(c.type_info().name()),
          })
          .collect()
      }
    };

    // `affected` is owned by the write path (a later, sacred phase); a read
    // query leaves it `None`.
    Ok(QueryResult {
      columns,
      rows,
      affected: None,
    })
  }

  fn kind(&self) -> Backend {
    Backend::Sqlite
  }
}

/// Guard the read path: reject anything that isn't a single read-only query
/// before it reaches the database. The primary write-safety boundary (the
/// read-only connection is a backstop): `CREATE TEMP TABLE`, DML/DDL, and
/// multi-statement payloads are refused here, so they never run outside the
/// gated write/diff path (#64).
///
/// If sqlparser can't parse the input (its SQLite coverage isn't total), we
/// do *not* false-reject a possibly-valid read — it falls through to the
/// read-only connection, which still refuses any write to the main database.
fn ensure_read_only_query(sql: &str) -> Result<()> {
  // Fail closed: the read path runs only what it can verify is a single
  // read-only query. Anything sqlparser can't parse — or that parses as a
  // write or as multiple statements — is refused rather than handed to the
  // database. Allowing unparsed SQL through is unsafe: some statements write
  // even on a read-only handle (e.g. `VACUUM INTO 'file'` copies the db to
  // disk), and an unparsed chain could smuggle a write past a one-statement
  // check. Intentional writes go through the gated write/diff path (#64).
  let statements = Parser::parse_sql(&SQLiteDialect {}, sql)
    .map_err(|e| VellumError::Driver(format!("read-only path: could not parse SQL ({e})")))?;
  match statements.as_slice() {
    // A single SELECT-style query (covers `WITH … SELECT`, `VALUES`, unions),
    // or empty / comment-only input (harmless — let SQLite handle it).
    [Statement::Query(_)] | [] => Ok(()),
    [_] => Err(VellumError::Driver(
      "read-only path: only SELECT-style queries run here; writes go through \
       the write/diff gate"
        .into(),
    )),
    stmts => Err(VellumError::Driver(format!(
      "read-only path: exactly one statement is allowed, got {}",
      stmts.len()
    ))),
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
