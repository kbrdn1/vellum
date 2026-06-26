//! The SQLite `Driver` — the first and only impl for Phase 0 (sqlx, bundled
//! libsqlite3, in-process). Maps SQLite's five storage classes (NULL /
//! INTEGER / REAL / TEXT / BLOB) onto the normalised `Value`.

use async_trait::async_trait;
use sqlx::sqlite::{SqlitePool, SqliteRow};
// Trait methods are imported anonymously to avoid colliding with the domain
// `Column` / `Row` types.
use sqlx::{Column as _, Executor as _, Row as _, TypeInfo as _, ValueRef as _};

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
    let pool = SqlitePool::connect(dsn).await.map_err(driver_err)?;
    Ok(Self { pool })
  }

  async fn query(&self, sql: &str) -> Result<QueryResult> {
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
