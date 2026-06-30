//! The multi-DB port, **frozen** in Phase 1 (#11) now that three real impls
//! justify the abstraction (SQLite, Postgres, MySQL): `connect` / `query` /
//! `introspect` / `backend` / `capabilities`. The write path (`execute`) and
//! streaming (`query_stream`) are deliberately *not* on the port — `execute`'s
//! shape depends on the changeset model designed with the gated write/diff path
//! (#64), and streaming has no Phase-1 consumer (the browse paginates). Adding
//! either now would be a speculative, untestable stub (YAGNI). They join the
//! port with their phase.

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
use crate::model::{Backend, Catalog, QueryResult};

/// Guard the read path for every backend: reject anything that isn't a single
/// read-only query before it reaches the database. The `dialect` is the
/// engine's own so the parse matches what the server would accept.
///
/// This is the **primary** write-safety boundary, and it also refuses
/// `SELECT … INTO` (a table or file write that wears a `Query`'s clothes, found
/// anywhere in the set expression). But it is *necessary, not sufficient*:
/// Postgres allows data-modifying CTEs
/// (`WITH t AS (INSERT … RETURNING *) SELECT * FROM t`) whose top level parses
/// as a `Query` yet still writes. Each impl pairs this with an engine-level
/// backstop (SQLite opens `SQLITE_OPEN_READONLY`; Postgres wraps every query in
/// a `READ ONLY` transaction; MySQL sets the session `transaction_read_only`),
/// so a write that slips past the parser is still refused. Intentional writes
/// go through the gated write/diff path (#64).
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

/// Whether a parsed `Query` carries a `SELECT … INTO` clause anywhere in its
/// set expression — a write that looks like a read. `INTO <table>` materialises
/// a table (Postgres `CREATE TABLE AS`); `INTO OUTFILE`/`DUMPFILE` writes a file
/// (MySQL) that a READ ONLY transaction does not stop. The clause can sit on a
/// branch of a `UNION`/`INTERSECT`/`EXCEPT` (a `SetOperation`) or inside a
/// parenthesised subquery, not only at the top level — so the whole tree is
/// walked, not just the outer body.
fn query_writes_via_into(query: &Query) -> bool {
  set_expr_writes_via_into(&query.body)
}

fn set_expr_writes_via_into(expr: &SetExpr) -> bool {
  match expr {
    SetExpr::Select(select) => select.into.is_some(),
    SetExpr::Query(query) => set_expr_writes_via_into(&query.body),
    SetExpr::SetOperation { left, right, .. } => set_expr_writes_via_into(left) || set_expr_writes_via_into(right),
    _ => false,
  }
}

/// What a backend supports, so the UI can gate features per engine (the sidebar
/// shows a schema level only where there is one; the editor offers EXPLAIN only
/// where it exists). A small, frozen, copyable record — no speculative fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
  /// The engine can `EXPLAIN` a query (produce a plan). True for all three so
  /// far; kept on the contract because a future backend may lack it.
  pub explain: bool,
  /// The engine has **multiple named schemas within a database** (Postgres).
  /// SQLite and MySQL collapse database and schema to one, so this is `false`
  /// — the sidebar then skips the schema level.
  pub schemas: bool,
  /// The engine declares/introspects foreign keys (so the catalog's
  /// `ForeignKey`s are meaningful and the UI can render relationships).
  pub foreign_keys: bool,
}

#[async_trait]
pub trait Driver: Send + Sync {
  /// Open a connection from a backend-specific DSN. For SQLite this is a
  /// `sqlite:` URL (e.g. `sqlite::memory:` or `sqlite:path/to/file.db`); for
  /// Postgres / MySQL a `postgres:` / `mysql:` URL. Kept off the vtable
  /// (`where Self: Sized`) so the port stays object-safe (`Box<dyn Driver>`).
  async fn connect(dsn: &str) -> Result<Self>
  where
    Self: Sized;

  /// Run a single **read** statement and collect the full result into memory.
  ///
  /// This is the read path. Every impl validates the input with the shared
  /// `ensure_single_read_query` (exactly one `SELECT`-style statement, no
  /// `SELECT … INTO`) and pairs it with an engine-level read-only backstop
  /// (SQLite `SQLITE_OPEN_READONLY`; Postgres a per-query `READ ONLY`
  /// transaction; MySQL a session `transaction_read_only`), so a mutating
  /// statement can't run here. Intentional writes go through the gated
  /// `execute`/apply path (changeset → diff → confirm), a later sacred phase
  /// (the write gate is tracked by #64). Streaming by batch is also a
  /// later-phase concern.
  async fn query(&self, sql: &str) -> Result<QueryResult>;

  /// Read the live schema into the pure [`Catalog`] (databases → schemas →
  /// relations → columns + foreign keys) the sidebar / autocomplete read from.
  async fn introspect(&self) -> Result<Catalog>;

  /// Which engine this driver talks to.
  fn backend(&self) -> Backend;

  /// What this backend supports, for per-engine UI gating.
  fn capabilities(&self) -> Capabilities;
}

#[cfg(test)]
mod tests {
  use super::{ensure_single_read_query, Driver};
  use sqlparser::dialect::{MySqlDialect, PostgreSqlDialect, SQLiteDialect};

  // The frozen port must stay object-safe — the connection manager / TUI hold
  // `Box<dyn Driver>`. This stops compiling if a method is added that takes
  // `self` by value without `where Self: Sized`, or that is generic.
  #[allow(dead_code)]
  fn assert_object_safe(driver: Box<dyn Driver>) -> Box<dyn Driver> {
    driver
  }

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

  #[test]
  fn refuses_into_outfile_buried_in_a_union() {
    // The `INTO` sits on a branch of a `UNION` (the body is a `SetOperation`,
    // not a bare `Select`), so the guard must walk the set expression — a
    // top-level-only check would let this file write through.
    assert!(
      ensure_single_read_query(&MySqlDialect {}, "SELECT 1 UNION SELECT 2 INTO OUTFILE '/tmp/x'").is_err(),
      "UNION … INTO OUTFILE must be refused"
    );
    // Same, nested in a parenthesised subquery.
    assert!(
      ensure_single_read_query(&MySqlDialect {}, "(SELECT 1 INTO OUTFILE '/tmp/x')").is_err(),
      "a parenthesised SELECT … INTO OUTFILE must be refused"
    );
    // Postgres `SELECT … INTO <table>` inside a UNION — `CREATE TABLE AS` on a
    // set-operation branch.
    assert!(
      ensure_single_read_query(&PostgreSqlDialect {}, "SELECT 1 AS a UNION SELECT 2 INTO foo").is_err(),
      "UNION … INTO <table> must be refused"
    );
  }
}
