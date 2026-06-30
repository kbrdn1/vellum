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
/// This is **best-effort early rejection, not the write-safety guarantee** — no
/// parser can catch every side effect (a function that writes, `nextval()`, …).
/// The guarantee is the per-engine read-only **backstop**: SQLite opens
/// `SQLITE_OPEN_READONLY`, Postgres wraps every query in a `READ ONLY`
/// transaction, MySQL sets the session `transaction_read_only` — so a write
/// that slips past this parser check is still refused by the engine (proven by
/// the `*_lands_no_write` integration tests). The one backstop gap — MySQL
/// `INTO OUTFILE`/`DUMPFILE`, a *file* write a read-only transaction does not
/// stop — is closed here, where it must be (MySQL only allows it at the top
/// level / a final union branch, both covered). So this guard refuses the
/// obvious writes early (non-`Query` statements, `SELECT … INTO`, a
/// data-modifying CTE whose body or `WITH` writes); deeper writes hidden in a
/// subquery that no engine will execute as a write are left to the backstop
/// rather than chased through the whole AST. Intentional writes go through the
/// gated write/diff path (#64).
pub(crate) fn ensure_single_read_query(dialect: &dyn Dialect, sql: &str) -> Result<()> {
  // Fail closed: run only what we can verify is one read-only statement.
  // Unparsed input is refused, never handed to the database.
  let statements = Parser::parse_sql(dialect, sql)
    .map_err(|e| VellumError::Driver(format!("read-only path: could not parse SQL ({e})")))?;
  match statements.as_slice() {
    // A single read-only query (covers `WITH … SELECT`, `VALUES`, `TABLE x`,
    // unions), or empty / comment-only input (harmless).
    [Statement::Query(query)] if query_is_read_only(query) => Ok(()),
    [] => Ok(()),
    // A `Query` that still writes: `SELECT … INTO` (a table — Postgres
    // `CREATE TABLE AS` — or a file — MySQL `INTO OUTFILE`, which a READ ONLY
    // transaction does NOT stop), or a data-modifying CTE whose top level is
    // the write (`WITH c AS (…) INSERT/UPDATE/DELETE …`, which `sqlparser`
    // models as a `Query` body). Refused at the parser, engine-agnostically.
    [Statement::Query(_)] => Err(VellumError::Driver(
      "read-only path: a write disguised as a query (`SELECT … INTO`, or a \
       data-modifying CTE) is refused; writes go through the write/diff gate"
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

/// Whether a parsed `Query` is a pure read. Defined as a **whitelist** (so an
/// unrecognised `sqlparser` variant fails closed → refused): a `SELECT` with no
/// `INTO`, a `VALUES`/`TABLE` read, or a set operation / parenthesised subquery
/// whose every branch is itself read-only. Everything else — a `SELECT … INTO`
/// (table or file write), or a data-modifying CTE that `sqlparser` models as a
/// `Query` with an `INSERT`/`UPDATE`/`DELETE` body — is **not** read-only.
fn query_is_read_only(query: &Query) -> bool {
  // The `WITH` clause's CTEs must each be read-only too: a data-modifying CTE
  // (`WITH w AS (INSERT … RETURNING *) SELECT * FROM w`) has a read-only body
  // but writes inside the CTE. (Postgres' per-query READ ONLY tx also catches
  // this, but the parser guard must be fail-closed on its own.)
  let with_is_read_only = query
    .with
    .as_ref()
    .is_none_or(|with| with.cte_tables.iter().all(|cte| query_is_read_only(&cte.query)));
  with_is_read_only && set_expr_is_read_only(&query.body)
}

fn set_expr_is_read_only(expr: &SetExpr) -> bool {
  match expr {
    SetExpr::Select(select) => select.into.is_none(),
    // Recurse through `query_is_read_only` (not just the body) so a nested
    // subquery's own `WITH` is validated too.
    SetExpr::Query(query) => query_is_read_only(query),
    SetExpr::SetOperation { left, right, .. } => set_expr_is_read_only(left) && set_expr_is_read_only(right),
    SetExpr::Values(_) | SetExpr::Table(_) => true,
    // `Insert` / `Update` / `Delete` bodies (data-modifying CTEs) and any
    // future variant — refused, fail-closed.
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
  fn refuses_data_modifying_cte_with_a_write_body() {
    // `sqlparser` models `WITH … INSERT/UPDATE/DELETE` as a `Query` whose *body*
    // is the write (#10 covered the inverse — a `SELECT` wrapping a writing
    // CTE). The read-only whitelist must refuse it, not pass it to the backend.
    // Verified red against a fail-open `_ => true` predicate.
    for sql in [
      "WITH c AS (SELECT 1 AS x) INSERT INTO t (x) SELECT x FROM c",
      "WITH c AS (SELECT 1) UPDATE t SET x = 1",
      "WITH c AS (SELECT 1) DELETE FROM t",
    ] {
      assert!(
        ensure_single_read_query(&PostgreSqlDialect {}, sql).is_err(),
        "a data-modifying CTE must be refused on the read path: {sql}"
      );
    }
  }

  #[test]
  fn refuses_a_writing_cte_under_a_read_only_body() {
    // The classic Postgres data-modifying CTE — the body is a plain `SELECT`,
    // but a CTE in the `WITH` clause writes. The whitelist must validate the
    // CTEs, not just the body (else it relies solely on the engine backstop).
    assert!(
      ensure_single_read_query(
        &PostgreSqlDialect {},
        "WITH w AS (INSERT INTO t VALUES (1) RETURNING *) SELECT * FROM w"
      )
      .is_err(),
      "a writing CTE under a read-only body must be refused"
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
