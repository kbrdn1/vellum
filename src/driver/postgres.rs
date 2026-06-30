//! The PostgreSQL `Driver` — the second impl (Phase 1, #10), behind the same
//! port as SQLite. Read-only by construction in two layers: the shared parser
//! guard, then an explicit transaction-level `READ ONLY` around every query.
//! The parser guard alone is *not* enough for PG — a data-modifying CTE
//! (`WITH t AS (INSERT … RETURNING *) SELECT * FROM t`) parses as a single
//! `Query` yet writes, and a SELECT can flip the session read-only default
//! (`set_config`) — so the per-query READ ONLY transaction is the load-bearing
//! boundary (the session default is only defence in depth).

use std::str::FromStr;

use async_trait::async_trait;
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgRow};
// Trait methods imported anonymously to avoid colliding with the domain
// `Column` / `Row` types.
use sqlx::{Column as _, Row as _, TypeInfo as _, ValueRef as _};

use sqlparser::dialect::PostgreSqlDialect;

use crate::driver::{ensure_single_read_query, Capabilities, Driver};
use crate::error::{Result, VellumError};
use crate::model::catalog;
use crate::model::{Backend, Column, QueryResult, Row, TypeKind, Value};

/// A connection to a PostgreSQL database, backed by a sqlx pool.
pub struct PostgresDriver {
  pool: PgPool,
}

#[async_trait]
impl Driver for PostgresDriver {
  async fn connect(dsn: &str) -> Result<Self> {
    // `default_transaction_read_only = on` is defence in depth — NOT the real
    // guard. It is session-scoped and a SELECT can flip it
    // (`select set_config('default_transaction_read_only','off',false)`), so
    // the actual write boundary is the per-query transaction-level READ ONLY in
    // `query()`. `sslmode` from the DSN is honoured by `PgConnectOptions` (a
    // rustls backend is compiled in). Intentional writes go through the gated
    // write/diff path (#64).
    let options = PgConnectOptions::from_str(dsn)
      .map_err(driver_err)?
      .options([("default_transaction_read_only", "on")]);
    // A single connection: one interactive read-only client to one database
    // needs no pool concurrency, and a single session avoids the divergence
    // where a `SET` lands on one pooled connection but not another.
    let pool = PgPoolOptions::new()
      .max_connections(1)
      .connect_with(options)
      .await
      .map_err(driver_err)?;
    Ok(Self { pool })
  }

  async fn query(&self, sql: &str) -> Result<QueryResult> {
    // Two-layer write guard:
    //   (1) The parser guard rejects anything that isn't a single `Query`. It
    //       is necessary but NOT sufficient for PG: a data-modifying CTE parses
    //       as a `Query`, and a SELECT can flip the *session* read-only default
    //       (`select set_config('default_transaction_read_only','off',false)`),
    //       which a reused pooled connection would inherit.
    //   (2) So run every query inside an explicit transaction-level READ ONLY.
    //       That can't be undone by a single statement: a write (incl. a
    //       data-modifying CTE) errors, and a `set_config` flip only changes
    //       *future* transactions — each re-wrapped READ ONLY here. The session
    //       default (set in `connect`) stays as defence in depth.
    ensure_single_read_query(&PostgreSqlDialect {}, sql)?;
    let mut tx = self.pool.begin().await.map_err(driver_err)?;
    sqlx::query("SET TRANSACTION READ ONLY")
      .execute(&mut *tx)
      .await
      .map_err(driver_err)?;
    let raw_rows = sqlx::query(sql).fetch_all(&mut *tx).await.map_err(driver_err)?;
    // Read-only: nothing to commit. Rollback closes the transaction (and would
    // discard a write, if the layers above ever let one through).
    tx.rollback().await.map_err(driver_err)?;

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

  async fn introspect(&self) -> Result<catalog::Catalog> {
    self.introspect_catalog().await
  }

  fn backend(&self) -> Backend {
    Backend::Postgres
  }

  fn capabilities(&self) -> Capabilities {
    // Postgres: `EXPLAIN`; multiple named schemas within a database; foreign
    // keys declared and introspected.
    Capabilities {
      explain: true,
      schemas: true,
      foreign_keys: true,
    }
  }
}

impl PostgresDriver {
  /// Backs [`Driver::introspect`]. Reads `information_schema` + `pg_catalog`
  /// for the connected database. Postgres has multiple named schemas, so the
  /// catalog is one `Database` (the connected db) holding every user schema
  /// (system schemas — `pg_catalog`, `information_schema`, `pg_*` — excluded).
  async fn introspect_catalog(&self) -> Result<catalog::Catalog> {
    let db_name: String = sqlx::query_scalar("SELECT current_database()")
      .fetch_one(&self.pool)
      .await
      .map_err(driver_err)?;

    let schema_rows = sqlx::query(
      // `pg\_%` (escaped `_`) excludes only the *literal* reserved `pg_` prefix
      // (pg_catalog, pg_toast, pg_temp_*). A bare `pg_%` would treat `_` as a
      // wildcard and also drop legal user schemas like `pgx` or `pgapp`.
      "SELECT schema_name FROM information_schema.schemata \
       WHERE schema_name NOT IN ('pg_catalog', 'information_schema') \
         AND schema_name NOT LIKE 'pg\\_%' ESCAPE '\\' ORDER BY schema_name",
    )
    .fetch_all(&self.pool)
    .await
    .map_err(driver_err)?;

    let mut schemas = Vec::with_capacity(schema_rows.len());
    for srow in &schema_rows {
      let schema_name: String = srow.try_get("schema_name").map_err(driver_err)?;
      let relations = self.introspect_relations(&schema_name).await?;
      schemas.push(catalog::Schema {
        name: schema_name,
        relations,
      });
    }

    Ok(catalog::Catalog {
      databases: vec![catalog::Database { name: db_name, schemas }],
    })
  }

  /// Tables and views of `schema`, in name order.
  async fn introspect_relations(&self, schema: &str) -> Result<Vec<catalog::Relation>> {
    let rows = sqlx::query(
      "SELECT table_name, table_type FROM information_schema.tables \
       WHERE table_schema = $1 AND table_type IN ('BASE TABLE', 'VIEW') \
       ORDER BY table_name",
    )
    .bind(schema)
    .fetch_all(&self.pool)
    .await
    .map_err(driver_err)?;

    let mut relations = Vec::with_capacity(rows.len());
    for row in &rows {
      let name: String = row.try_get("table_name").map_err(driver_err)?;
      let table_type: String = row.try_get("table_type").map_err(driver_err)?;
      let kind = if table_type == "VIEW" {
        catalog::RelationKind::View
      } else {
        catalog::RelationKind::Table
      };
      let columns = self.introspect_columns(schema, &name).await?;
      let foreign_keys = self.introspect_foreign_keys(schema, &name).await?;
      relations.push(catalog::Relation {
        name,
        kind,
        columns,
        foreign_keys,
      });
    }
    Ok(relations)
  }

  /// Columns of `schema.relation` in ordinal order. `information_schema` gives
  /// the type and nullability; a separate constraint query flags primary-key
  /// columns.
  async fn introspect_columns(&self, schema: &str, relation: &str) -> Result<Vec<catalog::Column>> {
    let pk_rows = sqlx::query(
      // Join on the table too, not just the constraint name/schema — Postgres
      // allows the same constraint name on different tables of one schema, so a
      // name-only join would pull a sibling table's PK columns into this set and
      // wrongly flag a homonym column as a primary key.
      "SELECT kcu.column_name FROM information_schema.table_constraints tc \
       JOIN information_schema.key_column_usage kcu \
         ON tc.constraint_catalog = kcu.constraint_catalog \
        AND tc.constraint_schema = kcu.constraint_schema \
        AND tc.constraint_name = kcu.constraint_name \
        AND tc.table_schema = kcu.table_schema \
        AND tc.table_name = kcu.table_name \
       WHERE tc.constraint_type = 'PRIMARY KEY' \
         AND tc.table_schema = $1 AND tc.table_name = $2",
    )
    .bind(schema)
    .bind(relation)
    .fetch_all(&self.pool)
    .await
    .map_err(driver_err)?;
    let mut pk: std::collections::HashSet<String> = std::collections::HashSet::new();
    for row in &pk_rows {
      pk.insert(row.try_get("column_name").map_err(driver_err)?);
    }

    let rows = sqlx::query(
      "SELECT column_name, data_type, is_nullable FROM information_schema.columns \
       WHERE table_schema = $1 AND table_name = $2 ORDER BY ordinal_position",
    )
    .bind(schema)
    .bind(relation)
    .fetch_all(&self.pool)
    .await
    .map_err(driver_err)?;

    let mut columns = Vec::with_capacity(rows.len());
    for row in &rows {
      let name: String = row.try_get("column_name").map_err(driver_err)?;
      let data_type: String = row.try_get("data_type").map_err(driver_err)?;
      let is_nullable: String = row.try_get("is_nullable").map_err(driver_err)?;
      columns.push(catalog::Column {
        primary_key: pk.contains(&name),
        nullable: is_nullable == "YES",
        name,
        data_type,
      });
    }
    Ok(columns)
  }

  /// Foreign keys of `schema.relation` from `pg_catalog`. `unnest(conkey,
  /// confkey) WITH ORDINALITY` pairs local and referenced columns in order, so
  /// composite keys fold correctly (the `information_schema` route mispairs
  /// them). The reference carries its own schema — Postgres FKs can be
  /// cross-schema.
  async fn introspect_foreign_keys(&self, schema: &str, relation: &str) -> Result<Vec<catalog::ForeignKey>> {
    let rows = sqlx::query(
      "SELECT con.conname AS constraint_name, \
              att.attname AS column_name, \
              ref_ns.nspname AS ref_schema, \
              ref_cl.relname AS ref_table, \
              ref_att.attname AS ref_column \
       FROM pg_constraint con \
       JOIN pg_class cl ON cl.oid = con.conrelid \
       JOIN pg_namespace ns ON ns.oid = cl.relnamespace \
       JOIN pg_class ref_cl ON ref_cl.oid = con.confrelid \
       JOIN pg_namespace ref_ns ON ref_ns.oid = ref_cl.relnamespace \
       JOIN LATERAL unnest(con.conkey, con.confkey) WITH ORDINALITY AS u(att, refatt, ord) ON true \
       JOIN pg_attribute att ON att.attrelid = con.conrelid AND att.attnum = u.att \
       JOIN pg_attribute ref_att ON ref_att.attrelid = con.confrelid AND ref_att.attnum = u.refatt \
       WHERE con.contype = 'f' AND ns.nspname = $1 AND cl.relname = $2 \
       ORDER BY con.conname, u.ord",
    )
    .bind(schema)
    .bind(relation)
    .fetch_all(&self.pool)
    .await
    .map_err(driver_err)?;

    let mut foreign_keys: Vec<catalog::ForeignKey> = Vec::new();
    let mut current: Option<String> = None;
    for row in &rows {
      let constraint: String = row.try_get("constraint_name").map_err(driver_err)?;
      let from: String = row.try_get("column_name").map_err(driver_err)?;
      let ref_schema: String = row.try_get("ref_schema").map_err(driver_err)?;
      let ref_table: String = row.try_get("ref_table").map_err(driver_err)?;
      let to: String = row.try_get("ref_column").map_err(driver_err)?;

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
            relation: ref_table,
            columns: vec![to],
          },
        });
      }
    }
    Ok(foreign_keys)
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
