//! The PostgreSQL `Driver` — the second impl (Phase 1, #10), behind the same
//! port as SQLite. Read-only by construction: the shared parser guard plus a
//! `default_transaction_read_only` session. The parser guard alone is *not*
//! enough for PG — a data-modifying CTE
//! (`WITH t AS (INSERT … RETURNING *) SELECT * FROM t`) parses as a single
//! `Query` yet writes — so the session backstop is load-bearing.

use std::str::FromStr;

use async_trait::async_trait;
use sqlx::postgres::{PgConnectOptions, PgPool, PgRow};
// Trait methods imported anonymously to avoid colliding with the domain
// `Column` / `Row` types.
use sqlx::{Column as _, Row as _, TypeInfo as _, ValueRef as _};

use sqlparser::dialect::PostgreSqlDialect;

use crate::driver::{ensure_single_read_query, Driver};
use crate::error::{Result, VellumError};
use crate::model::{Backend, Column, QueryResult, Row, TypeKind, Value};

/// A connection to a PostgreSQL database, backed by a sqlx pool.
pub struct PostgresDriver {
  pool: PgPool,
}

#[async_trait]
impl Driver for PostgresDriver {
  async fn connect(dsn: &str) -> Result<Self> {
    // Read-only by construction: every implicit transaction on this session
    // defaults to READ ONLY, so a write that slips past the parser guard (a
    // data-modifying CTE) is still refused by the server. A lone
    // `SET … read_only = off` can't re-enable writes through `query()` — it's
    // not a `Query`, so the guard refuses it, and a multi-statement payload is
    // refused too. `sslmode` from the DSN is honoured by `PgConnectOptions`
    // (a rustls backend is compiled in). Intentional writes go through the
    // gated write/diff path (#64).
    let options = PgConnectOptions::from_str(dsn)
      .map_err(driver_err)?
      .options([("default_transaction_read_only", "on")]);
    let pool = PgPool::connect_with(options).await.map_err(driver_err)?;
    Ok(Self { pool })
  }

  async fn query(&self, sql: &str) -> Result<QueryResult> {
    // Primary guard — necessary but not sufficient for PG; the read-only
    // session (see `connect`) is the backstop for data-modifying CTEs.
    ensure_single_read_query(&PostgreSqlDialect {}, sql)?;
    let raw_rows = sqlx::query(sql).fetch_all(&self.pool).await.map_err(driver_err)?;

    let mut rows: Vec<Row> = Vec::with_capacity(raw_rows.len());
    for raw in &raw_rows {
      let mut cells = Vec::with_capacity(raw.len());
      for i in 0..raw.len() {
        cells.push(pg_value_at(raw, i)?);
      }
      rows.push(cells);
    }

    // PG reports column types reliably (unlike SQLite literals), so the header
    // kind comes straight from the column's type. Headers for an *empty*
    // result (no row metadata without a `describe` round-trip) land with the
    // PG browse consumer (#15) — no caller needs them in #10.
    let columns = match raw_rows.first() {
      Some(meta) => meta
        .columns()
        .iter()
        .map(|c| Column {
          name: c.name().to_string(),
          kind: typekind_from_pg(c.type_info().name()),
        })
        .collect(),
      None => Vec::new(),
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
    Backend::Postgres
  }
}

/// Decode the `i`th cell of a PG row into a normalised `Value`, dispatched on
/// the column's PG type. `NULL` is checked first. The long tail (numeric,
/// arrays, enums, …) maps to an honest non-data marker `<typename>` rather than
/// a faked value — faithful decode is #76.
fn pg_value_at(row: &PgRow, i: usize) -> Result<Value> {
  let raw = row.try_get_raw(i).map_err(driver_err)?;
  if raw.is_null() {
    return Ok(Value::Null);
  }
  let type_name = raw.type_info().name().to_string();
  let value = match type_name.as_str() {
    "BOOL" => Value::Bool(row.try_get::<bool, _>(i).map_err(driver_err)?),
    "INT2" => Value::Int(i64::from(row.try_get::<i16, _>(i).map_err(driver_err)?)),
    "INT4" => Value::Int(i64::from(row.try_get::<i32, _>(i).map_err(driver_err)?)),
    "INT8" => Value::Int(row.try_get::<i64, _>(i).map_err(driver_err)?),
    "FLOAT4" => Value::Float(f64::from(row.try_get::<f32, _>(i).map_err(driver_err)?)),
    "FLOAT8" => Value::Float(row.try_get::<f64, _>(i).map_err(driver_err)?),
    "TEXT" | "VARCHAR" | "BPCHAR" | "NAME" => Value::Text(row.try_get::<String, _>(i).map_err(driver_err)?),
    "BYTEA" => Value::Bytes(row.try_get::<Vec<u8>, _>(i).map_err(driver_err)?),
    "JSON" | "JSONB" => {
      let v: sqlx::types::JsonValue = row.try_get(i).map_err(driver_err)?;
      Value::Json(v.to_string())
    }
    "UUID" => {
      let u: sqlx::types::Uuid = row.try_get(i).map_err(driver_err)?;
      Value::Text(u.to_string())
    }
    "TIMESTAMPTZ" => {
      let t: sqlx::types::time::OffsetDateTime = row.try_get(i).map_err(driver_err)?;
      Value::Timestamp(t.to_string())
    }
    "TIMESTAMP" => {
      let t: sqlx::types::time::PrimitiveDateTime = row.try_get(i).map_err(driver_err)?;
      Value::Timestamp(t.to_string())
    }
    "DATE" => {
      let t: sqlx::types::time::Date = row.try_get(i).map_err(driver_err)?;
      Value::Timestamp(t.to_string())
    }
    "TIME" => {
      let t: sqlx::types::time::Time = row.try_get(i).map_err(driver_err)?;
      Value::Timestamp(t.to_string())
    }
    // Conservative non-data marker — honest about not decoding this type yet
    // (numeric, arrays, enums, ranges, network types, …). Faithful decode is
    // tracked by #76. Never a faked value.
    _ => Value::Text(format!("<{}>", type_name.to_lowercase())),
  };
  Ok(value)
}

/// Map a PG type name to a column-header `TypeKind`. The conservative long tail
/// (#76) reports `Text` — the marker's own kind.
fn typekind_from_pg(name: &str) -> TypeKind {
  match name {
    "BOOL" => TypeKind::Bool,
    "INT2" | "INT4" | "INT8" => TypeKind::Int,
    "FLOAT4" | "FLOAT8" => TypeKind::Float,
    "TEXT" | "VARCHAR" | "BPCHAR" | "NAME" | "UUID" => TypeKind::Text,
    "BYTEA" => TypeKind::Bytes,
    "JSON" | "JSONB" => TypeKind::Json,
    "TIMESTAMPTZ" | "TIMESTAMP" | "DATE" | "TIME" => TypeKind::Timestamp,
    _ => TypeKind::Text,
  }
}

fn driver_err(e: sqlx::Error) -> VellumError {
  VellumError::Driver(e.to_string())
}
