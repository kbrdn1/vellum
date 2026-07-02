//! The MySQL `Driver` — the third impl (Phase 1, #11), behind the same port as
//! SQLite/Postgres. Read-only by construction in two layers: the shared parser
//! guard (which also rejects `SELECT … INTO OUTFILE`, a file write a read-only
//! transaction does NOT stop), then a session `transaction_read_only = ON` set
//! on every connection — so each autocommit statement runs as a READ ONLY
//! transaction and a write (incl. a writing function called via `SELECT`)
//! errors. Unlike Postgres's session option, this is *not* bypassable: MySQL
//! has no `set_config`-style function to flip it from a `SELECT`, and the guard
//! refuses a bare `SET`. (The PG `BEGIN` + `SET TRANSACTION READ ONLY` pattern
//! does not port — MySQL errors 1568 — and `START TRANSACTION` is rejected by
//! the prepared-statement protocol, error 1295.)

use std::str::FromStr;

use async_trait::async_trait;
use sqlx::mysql::{MySqlColumn, MySqlConnectOptions, MySqlPool, MySqlPoolOptions, MySqlRow};
// Trait methods imported anonymously to avoid colliding with the domain
// `Column` / `Row` types.
use sqlx::{
  AssertSqlSafe, Column as _, Executor as _, Row as _, SqlSafeStr as _, Statement as _, TypeInfo as _, ValueRef as _,
};

use sqlparser::dialect::MySqlDialect;

use crate::driver::{ensure_single_read_query, Capabilities, Driver};
use crate::error::{Result, VellumError};
use crate::model::catalog;
use crate::model::{Backend, Column, QueryResult, Row, TypeKind, Value};

/// A connection to a MySQL database, backed by a sqlx pool.
pub struct MySqlDriver {
  pool: MySqlPool,
}

impl MySqlDriver {
  /// Introspect the connected database into the pure [`catalog::Catalog`].
  ///
  /// Reads `information_schema` for the current database (`DATABASE()`). MySQL
  /// conflates database and schema, so the single connected database maps to
  /// one `Database` / `Schema`, both named after it. Inherent for now; it joins
  /// Backs [`Driver::introspect`] (the frozen port, #11).
  async fn introspect_catalog(&self) -> Result<catalog::Catalog> {
    // `information_schema` string columns have a binary collation — sqlx sees
    // them as `VARBINARY`, which `try_get::<String>` rejects. `CONVERT(_ USING
    // utf8mb4)` forces a character string so they decode as text.
    let db_name: String = sqlx::query("SELECT CONVERT(DATABASE() USING utf8mb4)")
      .fetch_one(&self.pool)
      .await
      .map_err(driver_err)?
      .try_get::<Option<String>, _>(0)
      .map_err(driver_err)?
      .ok_or_else(|| VellumError::Driver("no database selected in the DSN".into()))?;

    let relation_rows = sqlx::query(
      "SELECT CONVERT(TABLE_NAME USING utf8mb4) AS TABLE_NAME, \
              CONVERT(TABLE_TYPE USING utf8mb4) AS TABLE_TYPE \
       FROM information_schema.TABLES \
       WHERE TABLE_SCHEMA = DATABASE() ORDER BY TABLE_NAME",
    )
    .fetch_all(&self.pool)
    .await
    .map_err(driver_err)?;

    let mut relations = Vec::with_capacity(relation_rows.len());
    for row in &relation_rows {
      let name: String = row.try_get("TABLE_NAME").map_err(driver_err)?;
      let type_name: String = row.try_get("TABLE_TYPE").map_err(driver_err)?;
      let kind = if type_name == "VIEW" {
        catalog::RelationKind::View
      } else {
        catalog::RelationKind::Table
      };
      let columns = self.introspect_columns(&name).await?;
      let foreign_keys = self.introspect_foreign_keys(&name).await?;
      relations.push(catalog::Relation {
        name,
        kind,
        columns,
        foreign_keys,
      });
    }

    Ok(catalog::Catalog {
      databases: vec![catalog::Database {
        name: db_name.clone(),
        schemas: vec![catalog::Schema {
          name: db_name,
          relations,
        }],
      }],
    })
  }

  /// Columns of `relation` in ordinal order, from `information_schema.COLUMNS`.
  /// `COLUMN_TYPE` is the declared type verbatim (e.g. `int`, `varchar(255)`).
  async fn introspect_columns(&self, relation: &str) -> Result<Vec<catalog::Column>> {
    let rows = sqlx::query(
      "SELECT CONVERT(COLUMN_NAME USING utf8mb4) AS COLUMN_NAME, \
              CONVERT(COLUMN_TYPE USING utf8mb4) AS COLUMN_TYPE, \
              CONVERT(IS_NULLABLE USING utf8mb4) AS IS_NULLABLE, \
              CONVERT(COLUMN_KEY USING utf8mb4) AS COLUMN_KEY \
       FROM information_schema.COLUMNS \
       WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
    )
    .bind(relation)
    .fetch_all(&self.pool)
    .await
    .map_err(driver_err)?;

    let mut columns = Vec::with_capacity(rows.len());
    for row in &rows {
      let name: String = row.try_get("COLUMN_NAME").map_err(driver_err)?;
      let data_type: String = row.try_get("COLUMN_TYPE").map_err(driver_err)?;
      let is_nullable: String = row.try_get("IS_NULLABLE").map_err(driver_err)?;
      let column_key: String = row.try_get("COLUMN_KEY").map_err(driver_err)?;
      columns.push(catalog::Column {
        name,
        data_type,
        nullable: is_nullable == "YES",
        primary_key: column_key == "PRI",
      });
    }
    Ok(columns)
  }

  /// Foreign keys of `relation`, folded by constraint name (composite keys span
  /// rows ordered by `ORDINAL_POSITION`). MySQL always names the referenced
  /// columns, so there is no implicit-target case (unlike SQLite).
  async fn introspect_foreign_keys(&self, relation: &str) -> Result<Vec<catalog::ForeignKey>> {
    let rows = sqlx::query(
      "SELECT CONVERT(CONSTRAINT_NAME USING utf8mb4) AS CONSTRAINT_NAME, \
              CONVERT(COLUMN_NAME USING utf8mb4) AS COLUMN_NAME, \
              CONVERT(REFERENCED_TABLE_SCHEMA USING utf8mb4) AS REFERENCED_TABLE_SCHEMA, \
              CONVERT(REFERENCED_TABLE_NAME USING utf8mb4) AS REFERENCED_TABLE_NAME, \
              CONVERT(REFERENCED_COLUMN_NAME USING utf8mb4) AS REFERENCED_COLUMN_NAME \
       FROM information_schema.KEY_COLUMN_USAGE \
       WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? AND REFERENCED_TABLE_NAME IS NOT NULL \
       ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION",
    )
    .bind(relation)
    .fetch_all(&self.pool)
    .await
    .map_err(driver_err)?;

    let mut foreign_keys: Vec<catalog::ForeignKey> = Vec::new();
    let mut current: Option<String> = None;
    for row in &rows {
      let constraint: String = row.try_get("CONSTRAINT_NAME").map_err(driver_err)?;
      let from: String = row.try_get("COLUMN_NAME").map_err(driver_err)?;
      // MySQL foreign keys can cross databases (= schemas), so carry the
      // referenced schema rather than assuming same-schema.
      let ref_schema: String = row.try_get("REFERENCED_TABLE_SCHEMA").map_err(driver_err)?;
      let referenced: String = row.try_get("REFERENCED_TABLE_NAME").map_err(driver_err)?;
      let to: String = row.try_get("REFERENCED_COLUMN_NAME").map_err(driver_err)?;

      if current.as_deref() == Some(constraint.as_str()) {
        if let Some(fk) = foreign_keys.last_mut() {
          fk.columns.push(from);
          fk.references.columns.push(to);
        }
      } else {
        current = Some(constraint.clone());
        foreign_keys.push(catalog::ForeignKey {
          name: Some(constraint),
          columns: vec![from],
          references: catalog::Reference {
            schema: Some(ref_schema),
            relation: referenced,
            columns: vec![to],
          },
        });
      }
    }
    Ok(foreign_keys)
  }
}

#[async_trait]
impl Driver for MySqlDriver {
  async fn connect(dsn: &str) -> Result<Self> {
    // `sslmode`/TLS from the DSN is honoured by `MySqlConnectOptions` (a rustls
    // backend is compiled in). Every connection sets the session read-only
    // default (see the module docs) — the load-bearing write guard alongside
    // the parser. A single connection: one interactive read-only client to one
    // database needs no pool concurrency. Intentional writes go through the
    // gated write/diff path (#64).
    let options = MySqlConnectOptions::from_str(dsn).map_err(driver_err)?;
    let pool = MySqlPoolOptions::new()
      .max_connections(1)
      .after_connect(|conn, _meta| {
        Box::pin(async move {
          sqlx::query("SET SESSION transaction_read_only = ON")
            .execute(conn)
            .await?;
          Ok(())
        })
      })
      .connect_with(options)
      .await
      .map_err(driver_err)?;
    Ok(Self { pool })
  }

  async fn query(&self, sql: &str) -> Result<QueryResult> {
    // Parser guard (rejects non-`Query`, multi-statement, and `SELECT … INTO`),
    // then the session read-only backstop (autocommit → each statement is a
    // READ ONLY transaction).
    ensure_single_read_query(&MySqlDialect {}, sql)?;
    let raw_rows = sqlx::query(AssertSqlSafe(sql))
      .fetch_all(&self.pool)
      .await
      .map_err(driver_err)?;

    let mut rows: Vec<Row> = Vec::with_capacity(raw_rows.len());
    for raw in &raw_rows {
      let mut cells = Vec::with_capacity(raw.len());
      for i in 0..raw.len() {
        cells.push(mysql_value_at(raw, i)?);
      }
      rows.push(cells);
    }

    // MySQL reports column types reliably; the header kind comes from the
    // column type. An *empty* result carries no row metadata, so its headers
    // come from the prepared statement's column metadata — an empty relation
    // still renders its columns and stays sortable (#97). `prepare` inspects the
    // statement without executing it, so it is safe on the read path.
    let columns = match raw_rows.first() {
      Some(meta) => columns_from(meta.columns()),
      None => {
        let stmt = (&self.pool)
          .prepare(AssertSqlSafe(sql).into_sql_str())
          .await
          .map_err(driver_err)?;
        columns_from(stmt.columns())
      }
    };

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
    Backend::MySql
  }

  fn capabilities(&self) -> Capabilities {
    // MySQL: `EXPLAIN`; database = schema (no separate schema level); foreign
    // keys declared (InnoDB) and introspected.
    Capabilities {
      explain: true,
      schemas: false,
      foreign_keys: true,
    }
  }
}

/// Decode the `i`th cell of a MySQL row into a normalised `Value`, dispatched on
/// the column's MySQL type. `NULL` is checked first. The long tail (decimal,
/// unsigned 64-bit, bit, enum/set, geometry, …) maps to an honest non-data
/// marker `<typename>` — never a faked value (faithful decode is #76).
fn mysql_value_at(row: &MySqlRow, i: usize) -> Result<Value> {
  let raw = row.try_get_raw(i).map_err(driver_err)?;
  if raw.is_null() {
    return Ok(Value::Null);
  }
  let type_name = raw.type_info().name().to_string();
  let value = match type_name.as_str() {
    "TINYINT" => Value::Int(i64::from(row.try_get::<i8, _>(i).map_err(driver_err)?)),
    "SMALLINT" => Value::Int(i64::from(row.try_get::<i16, _>(i).map_err(driver_err)?)),
    "INT" | "MEDIUMINT" => Value::Int(i64::from(row.try_get::<i32, _>(i).map_err(driver_err)?)),
    "BIGINT" => Value::Int(row.try_get::<i64, _>(i).map_err(driver_err)?),
    "FLOAT" => Value::Float(f64::from(row.try_get::<f32, _>(i).map_err(driver_err)?)),
    "DOUBLE" => Value::Float(row.try_get::<f64, _>(i).map_err(driver_err)?),
    "VARCHAR" | "CHAR" | "TEXT" | "TINYTEXT" | "MEDIUMTEXT" | "LONGTEXT" | "ENUM" | "SET" => {
      Value::Text(row.try_get::<String, _>(i).map_err(driver_err)?)
    }
    "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BINARY" | "VARBINARY" => {
      Value::Bytes(row.try_get::<Vec<u8>, _>(i).map_err(driver_err)?)
    }
    "JSON" => {
      let v: sqlx::types::JsonValue = row.try_get(i).map_err(driver_err)?;
      Value::Json(v.to_string())
    }
    "DATETIME" | "TIMESTAMP" => {
      let t: sqlx::types::time::PrimitiveDateTime = row.try_get(i).map_err(driver_err)?;
      Value::Timestamp(t.to_string())
    }
    "DATE" => {
      let t: sqlx::types::time::Date = row.try_get(i).map_err(driver_err)?;
      Value::Timestamp(t.to_string())
    }
    // `TIME` is a *duration* in MySQL (negative, or up to `838:59:59`), not a
    // wall-clock time — `time::Time` can't hold those and would fail the whole
    // query. It joins the conservative marker tail until a faithful decode
    // (#76).
    // Conservative non-data marker — honest about not decoding this type yet
    // (TIME, decimal, unsigned 64-bit, bit, year, geometry, …). Faithful decode
    // is #76. Never a faked value.
    _ => Value::Text(format!("<{}>", type_name.to_lowercase())),
  };
  Ok(value)
}

/// Build the domain column headers from a slice of MySQL columns — shared by
/// the row-metadata path (a non-empty result) and the `describe` path (an empty
/// one, #97) so both spell the header identically.
fn columns_from(cols: &[MySqlColumn]) -> Vec<Column> {
  cols
    .iter()
    .map(|c| Column {
      name: c.name().to_string(),
      kind: typekind_from_mysql(c.type_info().name()),
    })
    .collect()
}

/// Map a MySQL type name to a column-header `TypeKind`. The conservative long
/// tail (#76) reports `Text` — the marker's own kind.
fn typekind_from_mysql(name: &str) -> TypeKind {
  match name {
    "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "BIGINT" => TypeKind::Int,
    "FLOAT" | "DOUBLE" => TypeKind::Float,
    "VARCHAR" | "CHAR" | "TEXT" | "TINYTEXT" | "MEDIUMTEXT" | "LONGTEXT" | "ENUM" | "SET" => TypeKind::Text,
    "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BINARY" | "VARBINARY" => TypeKind::Bytes,
    "JSON" => TypeKind::Json,
    // `TIME` is a duration (conservative marker, see `mysql_value_at`).
    "DATETIME" | "TIMESTAMP" | "DATE" => TypeKind::Timestamp,
    _ => TypeKind::Text,
  }
}

fn driver_err(e: sqlx::Error) -> VellumError {
  VellumError::Driver(e.to_string())
}
