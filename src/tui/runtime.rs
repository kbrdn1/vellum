//! Interactive TUI runtime: take over the terminal, render the result table,
//! and pump key events into the app until the user quits. The event loop is
//! thin glue over the pure `App` state machine (tested in `tui_app_tests.rs`)
//! and ratatui's `view` render; it owns no navigation logic of its own, so it
//! is covered by the manual Phase 0 gate rather than an e2e pty test.

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};

use crate::error::Result;
use crate::model::QueryResult;
use crate::tui::app::App;
use crate::tui::view;

/// Launch the scrollable table UI over `result`. Enters raw mode + the
/// alternate screen, renders until the user quits (`q` / `Esc`), and restores
/// the terminal on the way out — including on error.
pub fn run(result: QueryResult) -> Result<()> {
  let mut app = App::new(result);
  let mut terminal = ratatui::try_init()?;
  let outcome = event_loop(&mut terminal, &mut app);
  ratatui::try_restore()?;
  outcome
}

/// Draw / read-key / dispatch until `app` asks to quit. Arrow keys alias onto
/// `h`/`j`/`k`/`l`; `Esc` quits like `q`. Everything else is a no-op (the pure
/// `App` ignores unbound keys).
fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
  loop {
    terminal.draw(|frame| view::render(frame, &*app))?;
    if let Event::Key(key) = event::read()? {
      if key.kind == KeyEventKind::Press {
        match key.code {
          KeyCode::Char(c) => app.on_key(c),
          KeyCode::Left => app.on_key('h'),
          KeyCode::Right => app.on_key('l'),
          KeyCode::Up => app.on_key('k'),
          KeyCode::Down => app.on_key('j'),
          KeyCode::Esc => app.on_key('q'),
          _ => {}
        }
      }
    }
    if app.should_quit() {
      break;
    }
  }
  Ok(())
}
