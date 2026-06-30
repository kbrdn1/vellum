//! The multi-DB port. **Sketch** — deliberately minimal (`connect` / `query`
//! / `kind`) while SQLite is the only impl. It freezes into the richer port
//! (capabilities, introspect, streaming, transactional execute —
//! ARCHITECTURE §4) in Phase 1, once Postgres is the second impl. No
//! speculative abstraction now (YAGNI).

pub mod mysql;
pub mod postgres;
pub mod sqlite;

pub use mysql::MySqlDriver;
pub use postgres::PostgresDriver;
pub use sqlite::SqliteDriver;

use async_trait::async_trait;

use sqlparser::ast::{Query, SetExpr, Statement};
use sqlparser::dialect::Dialect;
use sqlparser::parser::Parser;

use crate::error::{Result, VellumError};
use crate::model::{Backend, QueryResult};

/// Guard the read path for every backend: reject anything that isn't a single
/// read-only query before it reaches the database. The `dialect` is the
/// engine's own so the parse matches what the server would accept.
///
/// This is the **primary** write-safety boundary, but it is *necessary, not
/// sufficient*: Postgres allows data-modifying CTEs
/// (`WITH t AS (INSERT … RETURNING *) SELECT * FROM t`) whose top level parses
/// as a `Query` yet still writes. Each impl pairs this with an engine-level
/// backstop (SQLite opens `SQLITE_OPEN_READONLY`; Postgres runs the session
/// `default_transaction_read_only = on`) so a write that slips past the parser
/// is still refused. Intentional writes go through the gated write/diff path
/// (#64).
pub(crate) fn ensure_single_read_query(dialect: &dyn Dialect, sql: &str) -> Result<()> {
  // Fail closed: run only what we can verify is one read-only statement.
  // Unparsed input is refused, never handed to the database.
  let statements = Parser::parse_sql(dialect, sql)
    .map_err(|e| VellumError::Driver(format!("read-only path: could not parse SQL ({e})")))?;
  match statements.as_slice() {
    // A single SELECT-style query (covers `WITH … SELECT`, `VALUES`, unions),
    // or empty / comment-only input (harmless).
    [Statement::Query(query)] if !query_writes_via_into(query) => Ok(()),
    [] => Ok(()),
    // `SELECT … INTO` is a write that a `Query` smuggles past the statement
    // shape: `INTO <table>` is `CREATE TABLE AS` (Postgres), and `INTO
    // OUTFILE/DUMPFILE` writes a file (MySQL) — the latter is NOT stopped by a
    // READ ONLY transaction. Refuse it at the parser, engine-agnostically.
    [Statement::Query(_)] => Err(VellumError::Driver(
      "read-only path: `SELECT … INTO` writes (a table or a file) and is refused; \
       reads go through the write/diff gate"
        .into(),
    )),
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

/// Whether a parsed `Query` carries a `SELECT … INTO` clause — a write that
/// looks like a read. `INTO <table>` materialises a table (Postgres
/// `CREATE TABLE AS`); `INTO OUTFILE`/`DUMPFILE` writes a file (MySQL) that a
/// READ ONLY transaction does not stop. The clause is only valid at the top
/// level of a `SELECT`, so the outer body is the only place to look.
fn query_writes_via_into(query: &Query) -> bool {
  matches!(&*query.body, SetExpr::Select(select) if select.into.is_some())
}

#[async_trait]
pub trait Driver: Send + Sync {
  /// Open a connection from a backend-specific DSN. For SQLite this is a
  /// `sqlite:` URL (e.g. `sqlite::memory:` or `sqlite:path/to/file.db`).
  async fn connect(dsn: &str) -> Result<Self>
  where
    Self: Sized;

  /// Run a single **read** statement and collect the full result into memory.
  ///
  /// This is the read path. The SQLite impl validates the input with
  /// `sqlparser` (exactly one `SELECT`-style statement — `INSERT` / `UPDATE` /
  /// `DELETE` / DDL, `CREATE TEMP`, and multi-statement payloads are refused)
  /// and opens its connections read-only (`SQLITE_OPEN_READONLY`) as a
  /// backstop, so a mutating statement can't run here. Intentional writes go
  /// through the gated `execute`/apply path (changeset → diff → confirm), a
  /// later sacred phase (ARCHITECTURE §4 splits read `query` from write
  /// `execute`; the write gate is tracked by #64). Streaming by batch is also
  /// a later-phase concern.
  async fn query(&self, sql: &str) -> Result<QueryResult>;

  /// Which engine this driver talks to.
  fn kind(&self) -> Backend;
}

#[cfg(test)]
mod tests {
  use super::ensure_single_read_query;
  use sqlparser::dialect::{MySqlDialect, PostgreSqlDialect, SQLiteDialect};

  // The guard is `pub(crate)`, so it is unit-tested here (an integration test
  // in `tests/` can't reach it). It is a pure parser-level function.

  #[test]
  fn allows_a_single_plain_select_on_every_dialect() {
    assert!(ensure_single_read_query(&SQLiteDialect {}, "SELECT 1").is_ok());
    assert!(ensure_single_read_query(&PostgreSqlDialect {}, "SELECT 1").is_ok());
    assert!(ensure_single_read_query(&MySqlDialect {}, "SELECT 1").is_ok());
  }

  #[test]
  fn refuses_select_into_outfile_and_dumpfile() {
    // `SELECT … INTO OUTFILE/DUMPFILE` writes a *file* — a write that a MySQL
    // `READ ONLY` transaction does NOT block (it restricts table writes, not
    // file writes). It parses as a top-level `Query`, so it must be refused at
    // the parser guard, not left to the engine.
    assert!(
      ensure_single_read_query(&MySqlDialect {}, "SELECT 1 INTO OUTFILE '/tmp/x'").is_err(),
      "SELECT … INTO OUTFILE must be refused"
    );
    assert!(
      ensure_single_read_query(&MySqlDialect {}, "SELECT 1 INTO DUMPFILE '/tmp/x'").is_err(),
      "SELECT … INTO DUMPFILE must be refused"
    );
  }

  #[test]
  fn refuses_select_into_table() {
    // Postgres `SELECT … INTO newtable` is `CREATE TABLE AS` — a write. The
    // per-engine read-only transaction catches it, but rejecting the INTO at
    // the parser is defence in depth and keeps the guard engine-agnostic.
    assert!(
      ensure_single_read_query(&PostgreSqlDialect {}, "SELECT 1 INTO foo").is_err(),
      "SELECT … INTO <table> must be refused"
    );
  }
}
