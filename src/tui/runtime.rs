//! Interactive TUI runtime: take over the terminal, render the result table,
//! and pump key events into the app until the user quits. The event loop is
//! thin glue over the pure `App` state machine (tested in `tui_app_tests.rs`)
//! and ratatui's `view` render. Its one piece of logic — the key→action
//! mapping — is factored into the pure [`key_action`] (tested in
//! `tui_runtime_tests.rs`); only the irreducible draw/read/quit glue needs a
//! real terminal and is left to the manual Phase 0 gate.

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
  // If init fails *after* enabling raw mode / the alternate screen, restore
  // (best-effort) before propagating so the user's terminal isn't left broken.
  let mut terminal = match ratatui::try_init() {
    Ok(terminal) => terminal,
    Err(e) => {
      let _ = ratatui::try_restore();
      return Err(e.into());
    }
  };
  let outcome = event_loop(&mut terminal, &mut app);
  ratatui::try_restore()?;
  outcome
}

/// Map a key press to the `App::on_key` character it triggers, or `None` when
/// the key is unbound. Pure (no terminal), so the arrow/`Esc` aliasing is
/// testable without a pty: `Char(c)` passes through, the arrows alias the vim
/// motions (`←`→`h`, `→`→`l`, `↑`→`k`, `↓`→`j`), `Esc` quits like `q`, and
/// anything else is `None`.
pub fn key_action(code: KeyCode) -> Option<char> {
  match code {
    KeyCode::Char(c) => Some(c),
    KeyCode::Left => Some('h'),
    KeyCode::Right => Some('l'),
    KeyCode::Up => Some('k'),
    KeyCode::Down => Some('j'),
    KeyCode::Esc => Some('q'),
    _ => None,
  }
}

/// Draw / read-key / dispatch until `app` asks to quit. Key handling is the
/// pure [`key_action`]; this loop only wires it to the terminal.
fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
  loop {
    terminal.draw(|frame| view::render(frame, &*app))?;
    if let Event::Key(key) = event::read()? {
      if key.kind == KeyEventKind::Press {
        if let Some(c) = key_action(key.code) {
          app.on_key(c);
        }
      }
    }
    if app.should_quit() {
      break;
    }
  }
  Ok(())
}
