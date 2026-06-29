//! Pure navigation state over a `QueryResult` — the row cursor (vertical) and
//! the horizontal column window. Zero ratatui, zero I/O: every transition is a
//! plain, clamped index update, unit-tested in `tests/tui_app_tests.rs`.
//!
//! Vertical scroll-into-view is intentionally *not* stored here: keeping the
//! cursor on screen needs the viewport height, a render-time concern. The view
//! delegates it to ratatui's stateful `TableState` keyed on [`selected`]; this
//! module owns only what is testable without a terminal — the cursor index and
//! the leftmost visible column.
//!
//! [`selected`]: TableState::selected

use crate::model::{Column, QueryResult, Row};

/// Row cursor + horizontal-scroll state over a result set.
#[derive(Debug)]
pub struct TableState {
  result: QueryResult,
  selected: usize,
  col_offset: usize,
}

impl TableState {
  /// Wrap a result, cursor on the first row and first column.
  pub fn new(result: QueryResult) -> Self {
    Self {
      result,
      selected: 0,
      col_offset: 0,
    }
  }

  /// The column headers.
  pub fn columns(&self) -> &[Column] {
    &self.result.columns
  }

  /// The result rows.
  pub fn rows(&self) -> &[Row] {
    &self.result.rows
  }

  /// Index of the selected (cursor) row.
  pub fn selected(&self) -> usize {
    self.selected
  }

  /// Index of the leftmost visible column (horizontal scroll position).
  pub fn col_offset(&self) -> usize {
    self.col_offset
  }

  /// Move the cursor down one row, clamped to the last row.
  pub fn select_next(&mut self) {
    let last = self.result.rows.len().saturating_sub(1);
    if self.selected < last {
      self.selected += 1;
    }
  }

  /// Move the cursor up one row, clamped to the first row.
  pub fn select_prev(&mut self) {
    self.selected = self.selected.saturating_sub(1);
  }

  /// Jump the cursor to the first row.
  pub fn select_first(&mut self) {
    self.selected = 0;
  }

  /// Jump the cursor to the last row (first row when the result is empty).
  pub fn select_last(&mut self) {
    self.selected = self.result.rows.len().saturating_sub(1);
  }

  /// Scroll one column left, clamped to the first column.
  pub fn scroll_left(&mut self) {
    self.col_offset = self.col_offset.saturating_sub(1);
  }

  /// Scroll one column right, clamped so the offset never reaches the column
  /// count — the last column always stays visible.
  pub fn scroll_right(&mut self) {
    let last = self.result.columns.len().saturating_sub(1);
    if self.col_offset < last {
      self.col_offset += 1;
    }
  }
}
