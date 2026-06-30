//! Unit tests for the pure LIMIT/OFFSET pagination cursor — no I/O, no terminal
//! (CLAUDE.md test taxonomy: a pure domain transform tested against the core).
//! `record(n)` stands in for "the runtime fetched `n` rows for this page"; the
//! cursor never queries anything itself.

use vellum::tui::state::paginate::Paginator;

#[test]
fn fresh_cursor_is_page_zero_and_over_fetches_by_one() {
  let p = Paginator::new(50);
  assert_eq!(p.offset(), 0, "page 0 starts at offset 0");
  assert_eq!(p.limit(), 51, "fetch one past the page to probe for a next page");
  assert_eq!(p.visible(), 0, "nothing fetched yet");
  assert!(!p.has_next(), "no probe row seen yet");
  assert_eq!(p.counter(), "no rows");
}

#[test]
fn a_full_page_with_a_probe_row_has_a_next() {
  let mut p = Paginator::new(50);
  p.record(51); // page_size + probe
  assert_eq!(p.visible(), 50, "the probe row is not displayed");
  assert!(p.has_next());
  assert_eq!(p.counter(), "rows 1-50");
}

#[test]
fn an_exactly_full_page_without_a_probe_has_no_next() {
  // Relation has exactly page_size rows: LIMIT 51 returns 50, no probe.
  let mut p = Paginator::new(50);
  p.record(50);
  assert_eq!(p.visible(), 50);
  assert!(!p.has_next(), "no probe row -> this is the last page");
  assert_eq!(p.counter(), "rows 1-50");
}

#[test]
fn a_partial_last_page_counts_what_it_has() {
  let mut p = Paginator::new(50);
  p.record(30);
  assert_eq!(p.visible(), 30);
  assert!(!p.has_next());
  assert_eq!(p.counter(), "rows 1-30");
}

#[test]
fn an_empty_page_reports_no_rows() {
  let mut p = Paginator::new(50);
  p.record(0);
  assert_eq!(p.visible(), 0);
  assert!(!p.has_next());
  assert_eq!(p.counter(), "no rows");
}

#[test]
fn next_advances_offset_and_recomputes_the_counter() {
  let mut p = Paginator::new(50);
  p.record(51); // page 0 full, has a next
  assert!(p.next_page(), "moved to page 1");
  assert_eq!(p.offset(), 50, "page 1 starts at offset 50");
  assert_eq!(p.visible(), 0, "page 1 not fetched yet -> loaded reset");
  p.record(20); // page 1 is a partial last page
  assert_eq!(p.counter(), "rows 51-70");
  assert!(!p.has_next());
}

#[test]
fn next_is_bounded_when_there_is_no_probe_row() {
  let mut p = Paginator::new(50);
  p.record(50); // exactly one page, no probe
  assert!(!p.next_page(), "no next page to move to");
  assert_eq!(p.offset(), 0, "cursor stayed on page 0");
}

#[test]
fn prev_saturates_at_the_first_page() {
  let mut p = Paginator::new(50);
  assert!(!p.prev_page(), "already on the first page");
  assert_eq!(p.offset(), 0);
}

#[test]
fn prev_returns_to_the_previous_page() {
  let mut p = Paginator::new(50);
  p.record(51);
  p.next_page(); // page 1
  assert!(p.prev_page(), "back to page 0");
  assert_eq!(p.offset(), 0);
  assert_eq!(p.visible(), 0, "page 0 must be re-fetched after moving back");
}
