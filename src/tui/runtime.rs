//! Interactive TUI runtime: take over the terminal, render the result table,
//! and pump key events into the app until the user quits. The event loop is
//! thin glue over the pure `App` state machine (tested in `tui_app_tests.rs`)
//! and ratatui's `view` render. Its one piece of logic — the key→action
//! mapping — is factored into the pure [`key_action`] (tested in
//! `tui_runtime_tests.rs`); only the irreducible draw/read/quit glue needs a
//! real terminal and is left to the manual Phase 0 gate.

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};

use crate::driver::Driver;
use crate::error::Result;
use crate::model::{Backend, QueryResult};
use crate::tui::app::{App, PageTarget};
use crate::tui::state::sidebar::RelationRef;
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

/// Launch the interactive browse UI over `driver`: render the schema sidebar +
/// result table, and on each navigation drain the [`PageTarget`] and fetch that
/// page read-only. Restores the terminal on the way out, including on a fetch
/// error (so a query failure can't leave the terminal in raw mode). Takes a
/// `Box<dyn Driver>` so any backend (SQLite by `--db`, or a named PG / MySQL /
/// SQLite connection by `--conn`) browses through the one loop.
pub async fn browse(driver: Box<dyn Driver>, mut app: App) -> Result<()> {
  let mut terminal = match ratatui::try_init() {
    Ok(terminal) => terminal,
    Err(e) => {
      let _ = ratatui::try_restore();
      return Err(e.into());
    }
  };
  let outcome = browse_loop(&mut terminal, &*driver, &mut app).await;
  ratatui::try_restore()?;
  outcome
}

/// Browse event loop: draw, read a key, dispatch, then service one pending page
/// fetch. All coordination lives in [`App::take_page_target`]; this loop only
/// turns the target into a query and feeds the rows back. Manual Phase-1 gate.
async fn browse_loop(terminal: &mut ratatui::DefaultTerminal, driver: &dyn Driver, app: &mut App) -> Result<()> {
  loop {
    terminal.draw(|frame| view::render(frame, &*app))?;
    if let Event::Key(key) = event::read()? {
      if key.kind == KeyEventKind::Press {
        if let Some(c) = key_action(key.code) {
          app.on_key(c);
        }
      }
    }
    if let Some(target) = app.take_page_target() {
      let sql = page_sql(driver.backend(), &target);
      let result = driver.query(&sql).await;
      apply_fetch(app, sql, result, &target.relation);
    }
    if app.should_quit() {
      break;
    }
  }
  Ok(())
}

/// Route a finished browse fetch back into `App`. On success, record the query
/// that ran and feed the rows in (which clears any prior error); on failure,
/// stash the error on the status line and **return normally** so the browse
/// loop keeps going — a query error must never end the session (#85). Pure over
/// `App` state (no terminal / no I/O), so the routing is unit-tested.
pub fn apply_fetch(app: &mut App, sql: String, result: Result<QueryResult>, relation: &RelationRef) {
  match result {
    Ok(rows) => {
      app.set_displayed_query(sql);
      app.apply_page(rows); // also clears any prior fetch error
    }
    Err(e) => {
      app.set_fetch_error(format!("{}.{}: {e}", relation.schema, relation.relation));
      app.clear_page(); // don't leave the old relation's rows under the new title
    }
  }
}

/// Build the read-only page query for a browse fetch: `SELECT * FROM
/// <schema>.<relation> [ORDER BY …] LIMIT n OFFSET m`. Identifiers are quoted
/// for `backend`'s dialect (ANSI double quotes for Postgres / SQLite, backticks
/// for MySQL) with the quote char doubled, so a name like `a"b` can't break out
/// of the quoting. Pure — tested in `tui_runtime_tests.rs`.
pub fn page_sql(backend: Backend, target: &PageTarget) -> String {
  let table = quote_qualified(backend, &target.relation.schema, &target.relation.relation);
  let order = target
    .order_by
    .as_deref()
    .map(|clause| format!(" {clause}"))
    .unwrap_or_default();
  format!(
    "SELECT * FROM {table}{order} LIMIT {} OFFSET {}",
    target.limit, target.offset
  )
}

/// Quote a `schema.relation` identifier for `backend`'s dialect (schema omitted
/// when empty).
fn quote_qualified(backend: Backend, schema: &str, relation: &str) -> String {
  let relation = backend.quote_ident(relation);
  if schema.is_empty() {
    relation
  } else {
    format!("{}.{relation}", backend.quote_ident(schema))
  }
}

/// Map a key press to the `App::on_key` character it triggers, or `None` when
/// the key is unbound. Pure (no terminal), so the arrow/`Esc` aliasing is
/// testable without a pty: `Char(c)` passes through, the arrows alias the vim
/// motions (`←`→`h`, `→`→`l`, `↑`→`k`, `↓`→`j`), `Esc` quits like `q`, and
/// anything else is `None`.
pub fn key_action(code: KeyCode) -> Option<char> {
  match code {
    KeyCode::Char(c) => Some(c),
    // Enter / Tab carry the App's open-relation and focus-toggle actions; the
    // pure state machine already speaks `'\n'` / `'\t'`.
    KeyCode::Enter => Some('\n'),
    KeyCode::Tab => Some('\t'),
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
