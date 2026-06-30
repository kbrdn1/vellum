//! The TUI application state. `on_key` is the whole input contract — pure,
//! clamped state transitions, no terminal I/O — so the state machine is fully
//! testable without ratatui (`tests/tui_app_tests.rs`).
//!
//! Two modes share one `App`:
//! - **one-shot** (`App::new`, `vellum … -i`): just a result table, no sidebar.
//! - **browse** (`App::browse`): a schema sidebar (#14) plus an initially-empty
//!   result table that selecting a relation fills (#15). Focus toggles between
//!   the two panes.

use crate::driver::Capabilities;
use crate::model::catalog::Catalog;
use crate::model::QueryResult;
use crate::tui::state::paginate::{PageRequest, Paginator, DEFAULT_PAGE_SIZE};
use crate::tui::state::sidebar::{RelationRef, SidebarState};
use crate::tui::state::table::TableState;

/// Which pane has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
  Sidebar,
  Table,
}

/// App state: the result table, an optional schema sidebar, the focused pane,
/// a pending open-browse intent, the browse pagination cursor (browse mode
/// only) plus its pending page request, and the quit flag.
#[derive(Debug)]
pub struct App {
  table: TableState,
  sidebar: Option<SidebarState>,
  focus: Focus,
  browse_intent: Option<RelationRef>,
  paginator: Option<Paginator>,
  page_request: Option<PageRequest>,
  quit: bool,
}

impl App {
  /// One-shot result mode: just the result table, cursor on the first row, no
  /// sidebar. Unchanged from Phase 0.
  pub fn new(result: QueryResult) -> Self {
    Self {
      table: TableState::new(result),
      sidebar: None,
      focus: Focus::Table,
      browse_intent: None,
      paginator: None,
      page_request: None,
      quit: false,
    }
  }

  /// Browse mode: a schema sidebar over `catalog`, plus an empty result table
  /// that selecting a relation fills (#15). Focus starts on the sidebar.
  /// `capabilities.schemas` collapses the schema level for engines without one.
  pub fn browse(catalog: Catalog, capabilities: Capabilities) -> Self {
    let empty = QueryResult {
      columns: Vec::new(),
      rows: Vec::new(),
      affected: None,
    };
    Self {
      table: TableState::new(empty),
      sidebar: Some(SidebarState::new(catalog, capabilities.schemas)),
      focus: Focus::Sidebar,
      browse_intent: None,
      paginator: Some(Paginator::new(DEFAULT_PAGE_SIZE)),
      page_request: None,
      quit: false,
    }
  }

  /// The result-table navigation state (read-only, for the view).
  pub fn table(&self) -> &TableState {
    &self.table
  }

  /// The schema sidebar state, if in browse mode (read-only, for the view).
  pub fn sidebar(&self) -> Option<&SidebarState> {
    self.sidebar.as_ref()
  }

  /// Which pane currently has focus.
  pub fn focus(&self) -> Focus {
    self.focus
  }

  /// Whether the event loop should exit.
  pub fn should_quit(&self) -> bool {
    self.quit
  }

  /// Take the pending open-browse intent (cleared on read). Set when the user
  /// opens a relation from the sidebar; the browse loader (#15) consumes it.
  pub fn take_browse_intent(&mut self) -> Option<RelationRef> {
    self.browse_intent.take()
  }

  /// Take the pending page request (cleared on read). Set when `n`/`p` move to a
  /// page that exists; the runtime fetches `paginator`'s `limit`/`offset` for the
  /// open relation and feeds the rows back through [`apply_page`](Self::apply_page).
  pub fn take_page_request(&mut self) -> Option<PageRequest> {
    self.page_request.take()
  }

  /// The browse status-line counter (`"rows 51-70"` / `"no rows"`), or `None` in
  /// one-shot mode where there is no pagination.
  pub fn page_counter(&self) -> Option<String> {
    self.paginator.as_ref().map(Paginator::counter)
  }

  /// Feed a freshly-fetched page (up to `limit` rows, the last being the probe)
  /// into the table. Records the fetched count so the counter and `has_next` are
  /// known, and trims the probe row off the display. No-op in one-shot mode.
  pub fn apply_page(&mut self, mut result: QueryResult) {
    if let Some(paginator) = self.paginator.as_mut() {
      paginator.record(result.rows.len());
      result.rows.truncate(paginator.visible());
      self.table = TableState::new(result);
    }
  }

  /// Move the browse cursor a page if that page exists, recording the request
  /// for the runtime to fetch. No-op in one-shot mode or at a boundary.
  fn request_page(&mut self, request: PageRequest) {
    if let Some(paginator) = self.paginator.as_mut() {
      let moved = match request {
        PageRequest::Next => paginator.next_page(),
        PageRequest::Prev => paginator.prev_page(),
      };
      if moved {
        self.page_request = Some(request);
      }
    }
  }

  /// Apply a key press. `q` quits; `Tab` toggles focus between the sidebar and
  /// the table (only when a sidebar exists); every other key routes to the
  /// focused pane. The crossterm loop maps the arrow keys / Enter onto these
  /// characters before calling `on_key`.
  pub fn on_key(&mut self, key: char) {
    match key {
      'q' => self.quit = true,
      '\t' => {
        if self.sidebar.is_some() {
          self.focus = match self.focus {
            Focus::Sidebar => Focus::Table,
            Focus::Table => Focus::Sidebar,
          };
        }
      }
      _ => match self.focus {
        Focus::Sidebar => {
          if let Some(sidebar) = self.sidebar.as_mut() {
            on_sidebar_key(sidebar, key, &mut self.browse_intent);
          }
        }
        // In the table pane, `n`/`p` page the browse cursor; everything else is
        // vim table navigation. In one-shot mode `request_page` is inert.
        Focus::Table => match key {
          'n' => self.request_page(PageRequest::Next),
          'p' => self.request_page(PageRequest::Prev),
          _ => on_table_key(&mut self.table, key),
        },
      },
    }
  }
}

/// Vim navigation over the result table: `j`/`k` rows, `g`/`G` first/last,
/// `h`/`l` column scroll.
fn on_table_key(table: &mut TableState, key: char) {
  match key {
    'j' => table.select_next(),
    'k' => table.select_prev(),
    'g' => table.select_first(),
    'G' => table.select_last(),
    'h' => table.scroll_left(),
    'l' => table.scroll_right(),
    _ => {}
  }
}

/// Sidebar keys: `j`/`k`/`g`/`G` navigate; Space expands/collapses the selected
/// node; Enter opens the selected relation (or toggles a database/schema).
fn on_sidebar_key(sidebar: &mut SidebarState, key: char, intent: &mut Option<RelationRef>) {
  match key {
    'j' => sidebar.select_next(),
    'k' => sidebar.select_prev(),
    'g' => sidebar.select_first(),
    'G' => sidebar.select_last(),
    ' ' => sidebar.toggle(),
    '\n' | '\r' => match sidebar.selected_relation() {
      Some(relation) => *intent = Some(relation),
      None => sidebar.toggle(),
    },
    _ => {}
  }
}
