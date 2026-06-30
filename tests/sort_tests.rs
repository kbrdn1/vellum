//! Unit tests for the pure browse sort state — no terminal, no I/O. `ORDER BY`
//! is asserted as a string (the clause the runtime splices into the page query)
//! and the tri-state toggle is driven directly.

use vellum::tui::state::sort::{toggle_sort, SortDir};

#[test]
fn first_press_sorts_a_column_ascending() {
  let sort = toggle_sort(None, "name").expect("a sort");
  assert_eq!(sort.column(), "name");
  assert_eq!(sort.dir(), SortDir::Asc);
  assert_eq!(sort.order_by_clause(), r#"ORDER BY "name" ASC"#);
}

#[test]
fn second_press_flips_to_descending() {
  let sort = toggle_sort(None, "name");
  let sort = toggle_sort(sort, "name").expect("still sorted");
  assert_eq!(sort.dir(), SortDir::Desc);
  assert_eq!(sort.order_by_clause(), r#"ORDER BY "name" DESC"#);
}

#[test]
fn third_press_on_the_same_column_clears_the_sort() {
  let sort = toggle_sort(None, "name");
  let sort = toggle_sort(sort, "name");
  assert_eq!(toggle_sort(sort, "name"), None, "asc -> desc -> off");
}

#[test]
fn sorting_a_different_column_restarts_ascending() {
  let sort = toggle_sort(None, "name"); // asc on name
  let sort = toggle_sort(sort, "name"); // desc on name
  let sort = toggle_sort(sort, "age").expect("switched columns");
  assert_eq!(sort.column(), "age");
  assert_eq!(sort.dir(), SortDir::Asc, "a fresh column starts ascending");
}

#[test]
fn a_column_name_with_a_quote_is_escaped_not_broken_out_of() {
  let sort = toggle_sort(None, r#"a"b"#).expect("a sort");
  assert_eq!(sort.order_by_clause(), r#"ORDER BY "a""b" ASC"#);
}
