//! State-machine tests for the TUI result table — ratatui-free: they assert
//! navigation and quit *state*, never pixels (CLAUDE.md TUI test taxonomy).
//! The pixel/render path is smoke-tested separately in `tui_view_tests.rs`.

use vellum::model::{Column, QueryResult, TypeKind, Value};
use vellum::tui::app::App;

/// A `cols`×`rows` result grid; each cell holds its flattened `row*cols+col`
/// index so assertions can stay specific without a real database.
fn grid(cols: usize, rows: usize) -> QueryResult {
  let columns = (0..cols)
    .map(|c| Column {
      name: format!("c{c}"),
      kind: TypeKind::Int,
    })
    .collect();
  let rows = (0..rows)
    .map(|r| (0..cols).map(|c| Value::Int((r * cols + c) as i64)).collect())
    .collect();
  QueryResult {
    columns,
    rows,
    affected: None,
  }
}

#[test]
fn on_key_j_advances_cursor() {
  let mut app = App::new(grid(2, 3));
  assert_eq!(app.table().selected(), 0);
  app.on_key('j');
  assert_eq!(app.table().selected(), 1);
}

#[test]
fn on_key_j_is_bounded_at_last_row() {
  let mut app = App::new(grid(2, 3));
  for _ in 0..10 {
    app.on_key('j');
  }
  assert_eq!(app.table().selected(), 2, "cursor must not run past the last row");
}

#[test]
fn on_key_k_moves_up_and_saturates_at_zero() {
  let mut app = App::new(grid(2, 3));
  app.on_key('j');
  app.on_key('j');
  app.on_key('k');
  assert_eq!(app.table().selected(), 1);
  app.on_key('k');
  app.on_key('k');
  assert_eq!(app.table().selected(), 0, "cursor must not go negative");
}

#[test]
fn capital_g_jumps_last_and_g_jumps_first() {
  let mut app = App::new(grid(2, 5));
  app.on_key('G');
  assert_eq!(app.table().selected(), 4);
  app.on_key('g');
  assert_eq!(app.table().selected(), 0);
}

#[test]
fn horizontal_scroll_is_bounded_to_column_count() {
  let mut app = App::new(grid(3, 2));
  assert_eq!(app.table().col_offset(), 0);
  app.on_key('l');
  assert_eq!(app.table().col_offset(), 1);
  for _ in 0..10 {
    app.on_key('l');
  }
  // Bounded to the column count: the last column stays visible, the offset
  // never reaches `cols` (== 3 here).
  assert_eq!(app.table().col_offset(), 2);
  app.on_key('h');
  assert_eq!(app.table().col_offset(), 1);
  for _ in 0..10 {
    app.on_key('h');
  }
  assert_eq!(app.table().col_offset(), 0, "column offset must not go negative");
}

#[test]
fn q_sets_quit() {
  let mut app = App::new(grid(1, 1));
  assert!(!app.should_quit());
  app.on_key('q');
  assert!(app.should_quit());
}

#[test]
fn unbound_key_is_a_noop() {
  let mut app = App::new(grid(2, 2));
  app.on_key('x');
  assert_eq!(app.table().selected(), 0);
  assert_eq!(app.table().col_offset(), 0);
  assert!(!app.should_quit());
}

#[test]
fn navigation_on_empty_result_stays_in_bounds() {
  let mut app = App::new(grid(0, 0));
  app.on_key('j');
  app.on_key('G');
  app.on_key('l');
  assert_eq!(app.table().selected(), 0);
  assert_eq!(app.table().col_offset(), 0);
  assert!(!app.should_quit());
}
