//! Pure server-side sort state for the browse view — zero ratatui, zero I/O
//! (unit-tested in `tests/sort_tests.rs`). Sorting re-issues the *paginated*
//! query with an `ORDER BY`; it never sorts rows in memory, so it stays
//! consistent with the virtualised browse (only the visible page is loaded).
//!
//! One column sorts at a time, tri-state: pressing sort on a column cycles
//! ascending → descending → off. The runtime reads [`Sort::order_by_clause`]
//! when it rebuilds the page query.

/// Sort direction for the active column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
  Asc,
  Desc,
}

/// The active sort: one column and its direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sort {
  column: String,
  dir: SortDir,
}

impl Sort {
  /// The `ORDER BY "col" ASC|DESC` clause for the page query. The identifier is
  /// double-quoted with embedded quotes doubled, so a column named `a"b` can't
  /// break out of the quoting.
  pub fn order_by_clause(&self) -> String {
    let col = self.column.replace('"', "\"\"");
    let dir = match self.dir {
      SortDir::Asc => "ASC",
      SortDir::Desc => "DESC",
    };
    format!("ORDER BY \"{col}\" {dir}")
  }

  /// The sorted column's name.
  pub fn column(&self) -> &str {
    &self.column
  }

  /// The sort direction.
  pub fn dir(&self) -> SortDir {
    self.dir
  }
}

/// Cycle the sort for `column`: a new column (or none) starts ascending; the
/// same column goes ascending → descending → off. Returns the next sort state.
pub fn toggle_sort(current: Option<Sort>, column: &str) -> Option<Sort> {
  match current {
    Some(sort) if sort.column == column => match sort.dir {
      SortDir::Asc => Some(Sort {
        column: sort.column,
        dir: SortDir::Desc,
      }),
      // Third press on the same column clears the sort.
      SortDir::Desc => None,
    },
    _ => Some(Sort {
      column: column.to_string(),
      dir: SortDir::Asc,
    }),
  }
}
