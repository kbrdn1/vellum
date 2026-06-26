//! The row-oriented `QueryResult` container returned by a `Driver` query.
//!
//! Pure data — no I/O. Row-oriented is the conservative default that serves
//! the transactional path; a columnar Arrow-backed representation may be
//! added for big-volume backends later (ARCHITECTURE §4), as a superset.

use crate::model::value::{TypeKind, Value};

/// A result column: its name plus the normalised `TypeKind` of its cells.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
  pub name: String,
  pub kind: TypeKind,
}

/// One result row — a positional vector of cells aligned with `columns`.
pub type Row = Vec<Value>;

/// The outcome of a query: the column headers, the rows, and — for
/// `INSERT` / `UPDATE` / `DELETE` — the number of affected rows (`None` for
/// `SELECT`).
#[derive(Debug, Clone, PartialEq)]
pub struct QueryResult {
  pub columns: Vec<Column>,
  pub rows: Vec<Row>,
  pub affected: Option<u64>,
}
