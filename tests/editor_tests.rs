//! Unit tests for the pure SQL-editor buffer — no terminal, no I/O (CLAUDE.md
//! test taxonomy: assert buffer state, not pixels).

use vellum::tui::state::editor::EditorState;

fn typed(text: &str) -> EditorState {
  let mut e = EditorState::new();
  for c in text.chars() {
    e.insert(c);
  }
  e
}

#[test]
fn a_fresh_buffer_is_empty() {
  let e = EditorState::new();
  assert!(e.is_empty());
  assert_eq!(e.text(), "");
  assert_eq!(e.cursor(), 0);
}

#[test]
fn typing_accumulates_text_and_advances_the_cursor() {
  let e = typed("select 1");
  assert_eq!(e.text(), "select 1");
  assert_eq!(e.cursor(), 8);
  assert!(!e.is_empty());
}

#[test]
fn newlines_make_the_buffer_multiline() {
  let e = typed("select *\nfrom users");
  assert_eq!(e.text(), "select *\nfrom users");
}

#[test]
fn backspace_deletes_before_the_cursor() {
  let mut e = typed("selectx");
  e.backspace();
  assert_eq!(e.text(), "select");
  assert_eq!(e.cursor(), 6);
}

#[test]
fn backspace_on_an_empty_buffer_is_a_noop() {
  let mut e = EditorState::new();
  e.backspace();
  assert_eq!(e.text(), "");
  assert_eq!(e.cursor(), 0);
}

#[test]
fn left_then_insert_writes_in_the_middle() {
  // "selct" (cursor at 5) -> left,left puts the cursor at index 3, between 'l'
  // and 'c'; inserting 'e' there fixes it to "select".
  let mut e = typed("selct");
  e.left();
  e.left();
  e.insert('e');
  assert_eq!(e.text(), "select");
  assert_eq!(e.cursor(), 4);
}

#[test]
fn cursor_movement_is_clamped_at_both_ends() {
  let mut e = typed("ab");
  e.right(); // already at end (2) -> stays
  assert_eq!(e.cursor(), 2);
  e.left();
  e.left();
  e.left(); // clamps at 0
  assert_eq!(e.cursor(), 0);
  e.left();
  assert_eq!(e.cursor(), 0, "cursor must not go negative");
}
