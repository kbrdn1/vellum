//! The TUI application state for Phase 0: a single result table plus a quit
//! flag. `on_key` is the whole input contract — pure, clamped state
//! transitions with no terminal I/O — so the state machine is fully testable
//! without ratatui (`tests/tui_app_tests.rs`).

use crate::model::QueryResult;
use crate::tui::state::table::TableState;

/// Phase 0 app state: the result table and whether the user asked to quit.
#[derive(Debug)]
pub struct App {
  table: TableState,
  quit: bool,
}

impl App {
  /// Build the app around a query result, cursor on the first row.
  pub fn new(result: QueryResult) -> Self {
    Self {
      table: TableState::new(result),
      quit: false,
    }
  }

  /// The result-table navigation state (read-only, for the view).
  pub fn table(&self) -> &TableState {
    &self.table
  }

  /// Whether the event loop should exit.
  pub fn should_quit(&self) -> bool {
    self.quit
  }

  /// Apply a key press. Vim navigation: `j`/`k` move the row cursor, `g`/`G`
  /// jump to the first/last row, `h`/`l` scroll the column window, `q` quits.
  /// The crossterm event loop (wired with the interactive/one-shot mode) maps
  /// the arrow keys onto `h`/`j`/`k`/`l` before calling this; any other key is
  /// ignored.
  pub fn on_key(&mut self, key: char) {
    match key {
      'j' => self.table.select_next(),
      'k' => self.table.select_prev(),
      'g' => self.table.select_first(),
      'G' => self.table.select_last(),
      'h' => self.table.scroll_left(),
      'l' => self.table.scroll_right(),
      'q' => self.quit = true,
      _ => {}
    }
  }
}
