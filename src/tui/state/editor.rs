//! Pure text buffer for the SQL editor pane — zero ratatui, zero I/O, so the
//! edit transitions are unit-tested without a terminal (`tests/editor_tests.rs`).
//!
//! The buffer is a flat `Vec<char>` with a single cursor index; a newline is
//! just a `'\n'` in the buffer, so multiline falls out without line bookkeeping.
//! This is the substrate the editor *intent* runs on (`App::submit_query`); the
//! rendered widget, syntax highlight, and history are later phases — the
//! cursor-positioning a real edit UX needs lands with the editor render.

/// A flat character buffer with one insertion cursor.
#[derive(Debug, Default)]
pub struct EditorState {
  chars: Vec<char>,
  /// Insertion point, a char index in `0..=chars.len()`.
  cursor: usize,
}

impl EditorState {
  /// An empty buffer, cursor at the start.
  pub fn new() -> Self {
    Self::default()
  }

  /// Insert a character at the cursor and step the cursor past it. A `'\n'`
  /// inserts a line break like any other char.
  pub fn insert(&mut self, c: char) {
    self.chars.insert(self.cursor, c);
    self.cursor += 1;
  }

  /// Delete the character before the cursor, if any (backspace).
  pub fn backspace(&mut self) {
    if self.cursor > 0 {
      self.cursor -= 1;
      self.chars.remove(self.cursor);
    }
  }

  /// Move the cursor one character left, clamped at the start.
  pub fn left(&mut self) {
    self.cursor = self.cursor.saturating_sub(1);
  }

  /// Move the cursor one character right, clamped at the end.
  pub fn right(&mut self) {
    if self.cursor < self.chars.len() {
      self.cursor += 1;
    }
  }

  /// The cursor's char index in the buffer.
  pub fn cursor(&self) -> usize {
    self.cursor
  }

  /// Whether the buffer is empty.
  pub fn is_empty(&self) -> bool {
    self.chars.is_empty()
  }

  /// The full buffer text (the SQL to run).
  pub fn text(&self) -> String {
    self.chars.iter().collect()
  }
}
