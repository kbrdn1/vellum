//! The TUI application state. `on_key` is the whole input contract — pure,
//! clamped state transitions, no terminal I/O — so the state machine is fully
//! testable without ratatui (`tests/tui_app_tests.rs`).
//!
//! Three modes share one `App`:
//! - **one-shot** (`App::new`, `vellum … -i`): just a result table, no panes.
//! - **browse** (`App::browse`): a schema sidebar (#14) plus an initially-empty
//!   result table that selecting a relation fills (#15).
//! - **query** (`App::query`): a multiline SQL editor (#16) over a result table;
//!   submitting runs the buffer.
//!
//! Focus toggles between the table and whichever side pane the mode has.

use crate::driver::Capabilities;
use crate::model::catalog::Catalog;
use crate::model::{Backend, QueryResult};
use crate::tui::state::editor::EditorState;
use crate::tui::state::paginate::{PageRequest, Paginator, DEFAULT_PAGE_SIZE};
use crate::tui::state::sidebar::{RelationRef, SidebarState};
use crate::tui::state::sort::{toggle_sort, Sort};
use crate::tui::state::table::TableState;

/// Which pane has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
  Sidebar,
  Editor,
  Table,
}

/// App state: the result table, the optional side panes (schema sidebar / SQL
/// editor), the focused pane, the runtime intents (open-browse, page request,
/// run-query, re-query), the browse pagination cursor and sort, and the quit
/// flag. The intents are pure signals a real runtime drains and services.
#[derive(Debug)]
pub struct App {
  table: TableState,
  sidebar: Option<SidebarState>,
  editor: Option<EditorState>,
  focus: Focus,
  browse_intent: Option<RelationRef>,
  paginator: Option<Paginator>,
  page_request: Option<PageRequest>,
  run_query: Option<String>,
  sort: Option<Sort>,
  requery: bool,
  current_relation: Option<RelationRef>,
  displayed_query: Option<String>,
  /// The engine this browse session talks to (`None` in one-shot / query mode) —
  /// the `[sqlite]`-style header badge.
  backend: Option<Backend>,
  quit: bool,
}

/// What the runtime should fetch for the browse table, derived entirely from
/// `App` state by [`App::take_page_target`]: the open relation, the page bounds,
/// and the (optional) sort clause. The runtime turns this into a single
/// read-only `SELECT … LIMIT/OFFSET` and feeds the rows back via
/// [`App::apply_page`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageTarget {
  pub relation: RelationRef,
  pub limit: usize,
  pub offset: usize,
  pub order_by: Option<String>,
}

impl App {
  /// One-shot result mode: just the result table, cursor on the first row, no
  /// sidebar. Unchanged from Phase 0.
  pub fn new(result: QueryResult) -> Self {
    Self {
      table: TableState::new(result),
      sidebar: None,
      editor: None,
      focus: Focus::Table,
      browse_intent: None,
      paginator: None,
      page_request: None,
      run_query: None,
      sort: None,
      requery: false,
      current_relation: None,
      displayed_query: None,
      backend: None,
      quit: false,
    }
  }

  /// Browse mode: a schema sidebar over `catalog`, plus an empty result table
  /// that selecting a relation fills (#15). Focus starts on the sidebar.
  /// `capabilities.schemas` collapses the schema level for engines without one.
  pub fn browse(catalog: Catalog, capabilities: Capabilities, backend: Backend) -> Self {
    let empty = QueryResult {
      columns: Vec::new(),
      rows: Vec::new(),
      affected: None,
    };
    Self {
      table: TableState::new(empty),
      sidebar: Some(SidebarState::new(catalog, capabilities.schemas)),
      editor: None,
      focus: Focus::Sidebar,
      browse_intent: None,
      paginator: Some(Paginator::new(DEFAULT_PAGE_SIZE)),
      page_request: None,
      run_query: None,
      sort: None,
      requery: false,
      current_relation: None,
      displayed_query: None,
      backend: Some(backend),
      quit: false,
    }
  }

  /// SQL-console mode: a multiline editor over an initially-empty result table
  /// (#16). Focus starts on the editor; `Tab` toggles editor↔table; submitting
  /// (Ctrl-Enter) emits a run-query intent the runtime services read-only.
  pub fn query() -> Self {
    let empty = QueryResult {
      columns: Vec::new(),
      rows: Vec::new(),
      affected: None,
    };
    Self {
      table: TableState::new(empty),
      sidebar: None,
      editor: Some(EditorState::new()),
      focus: Focus::Editor,
      browse_intent: None,
      paginator: None,
      page_request: None,
      run_query: None,
      sort: None,
      requery: false,
      current_relation: None,
      displayed_query: None,
      backend: None,
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

  /// The relation currently being browsed, if one is open — the table title.
  pub fn current_relation(&self) -> Option<&RelationRef> {
    self.current_relation.as_ref()
  }

  /// The browse connection's database name (from the catalog), for the header.
  pub fn database_name(&self) -> Option<&str> {
    self.sidebar.as_ref().and_then(SidebarState::database_name)
  }

  /// The engine this browse session talks to — the header badge.
  pub fn backend(&self) -> Option<Backend> {
    self.backend
  }

  /// The status-line context breadcrumb: `schema.relation` once a relation is
  /// open, else the database name (nothing selected yet). Empty in one-shot mode.
  pub fn context_label(&self) -> String {
    if let Some(r) = self.current_relation() {
      format!("{}.{}", r.schema, r.relation)
    } else {
      self.database_name().unwrap_or_default().to_string()
    }
  }

  /// The `[2] <table> (N)` count for the open relation's pane title: the number
  /// of rows loaded on the current page, with a `+` when a next page exists
  /// (no `COUNT(*)` — the browse path never counts). `None` when no page is
  /// loaded or outside browse mode.
  pub fn page_loaded_label(&self) -> Option<String> {
    let paginator = self.paginator.as_ref()?;
    let more = if paginator.has_next() { "+" } else { "" };
    Some(format!("{}{more}", paginator.visible()))
  }

  /// The SQL that produced the currently-displayed page, for the query line.
  /// Set by the runtime after each successful fetch.
  pub fn displayed_query(&self) -> Option<&str> {
    self.displayed_query.as_deref()
  }

  /// Record the SQL the runtime just ran for the displayed page.
  pub fn set_displayed_query(&mut self, sql: String) {
    self.displayed_query = Some(sql);
  }

  /// The SQL editor buffer, if in query mode (read-only, for the view).
  pub fn editor(&self) -> Option<&EditorState> {
    self.editor.as_ref()
  }

  /// Take the pending run-query intent (cleared on read). Set by `submit_query`
  /// (Ctrl-Enter); the runtime runs it read-only and replaces the result table.
  pub fn take_run_query(&mut self) -> Option<String> {
    self.run_query.take()
  }

  /// Submit the editor buffer to be run (Ctrl-Enter): emit a run-query intent
  /// carrying the current text. No-op when there is no editor.
  pub fn submit_query(&mut self) {
    if let Some(editor) = self.editor.as_ref() {
      self.run_query = Some(editor.text());
    }
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

  /// The active browse sort, if any (read-only, for the view and the page query).
  pub fn sort(&self) -> Option<&Sort> {
    self.sort.as_ref()
  }

  /// Take the pending re-query flag (cleared on read). Set when the sort changes;
  /// the runtime re-fetches page 0 of the open relation with the new `ORDER BY`.
  pub fn take_requery(&mut self) -> bool {
    std::mem::take(&mut self.requery)
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

  /// Open a relation picked in the sidebar: record the intent for the loader to
  /// fetch, and **restart pagination from page 0** — a freshly-opened relation
  /// must not inherit the previous one's page offset, page request, or sort
  /// (its columns differ).
  fn open_relation(&mut self, relation: RelationRef) {
    self.current_relation = Some(relation.clone());
    self.browse_intent = Some(relation);
    self.paginator = Some(Paginator::new(DEFAULT_PAGE_SIZE));
    self.page_request = None;
    self.sort = None;
    self.requery = false;
    self.displayed_query = None;
  }

  /// Drain any pending browse fetch — a relation just opened, a page just moved,
  /// or the sort just changed — and return what to fetch, derived from `App`
  /// state. Returns `None` when nothing is pending (or before a relation is
  /// open). Keeping the priority/staleness logic here keeps it in the
  /// unit-tested layer; the runtime stays a thin `query → apply_page`.
  pub fn take_page_target(&mut self) -> Option<PageTarget> {
    let opened = self.browse_intent.take().is_some();
    let paged = self.page_request.take().is_some();
    let resorted = std::mem::take(&mut self.requery);
    if !(opened || paged || resorted) {
      return None;
    }
    let relation = self.current_relation.clone()?;
    let paginator = self.paginator.as_ref()?;
    Some(PageTarget {
      relation,
      limit: paginator.limit(),
      offset: paginator.offset(),
      order_by: self.sort.as_ref().map(Sort::order_by_clause),
    })
  }

  /// Toggle the server-side sort on the column under the horizontal cursor
  /// (browse only — sorting re-issues the paginated query). Cycles the column
  /// ascending → descending → off, restarts pagination from page 0, and raises
  /// the re-query flag. No-op outside browse or on an empty result.
  fn sort_current_column(&mut self) {
    if self.paginator.is_none() {
      return; // sort is a server-side re-query — browse mode only
    }
    let column = match self.table.columns().get(self.table.col_offset()) {
      Some(column) => column.name.clone(),
      None => return, // no columns to sort
    };
    self.sort = toggle_sort(self.sort.take(), &column);
    self.paginator = Some(Paginator::new(DEFAULT_PAGE_SIZE));
    self.page_request = None;
    self.requery = true;
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

  /// Apply a key press. The crossterm loop maps arrow keys / Enter onto these
  /// characters before calling `on_key`.
  ///
  /// In the **editor** pane every printable key is text — `q` types `q`, it does
  /// not quit (the runtime maps Esc → quit and Ctrl-Enter → [`submit_query`]
  /// there); only `Tab` is intercepted, to cycle focus. Outside the editor, `q`
  /// quits and `Tab` toggles focus; other keys route to the focused pane.
  ///
  /// [`submit_query`]: Self::submit_query
  pub fn on_key(&mut self, key: char) {
    if self.focus == Focus::Editor {
      match key {
        '\t' => self.toggle_focus(),
        c => {
          if let Some(editor) = self.editor.as_mut() {
            editor.insert(c);
          }
        }
      }
      return;
    }
    match key {
      'q' => self.quit = true,
      '\t' => self.toggle_focus(),
      _ => match self.focus {
        Focus::Sidebar => {
          // Resolve the opened relation (if any) before touching `self` again —
          // the sidebar borrow must end before `open_relation` takes `&mut self`.
          let opened = self.sidebar.as_mut().and_then(|sidebar| on_sidebar_key(sidebar, key));
          if let Some(relation) = opened {
            self.open_relation(relation);
          }
        }
        // In the table pane, `n`/`p` page the browse cursor and `s` sorts the
        // current column; everything else is vim table navigation. In one-shot
        // mode `request_page` / `sort_current_column` are inert.
        Focus::Table => match key {
          'n' => self.request_page(PageRequest::Next),
          'p' => self.request_page(PageRequest::Prev),
          's' => self.sort_current_column(),
          _ => on_table_key(&mut self.table, key),
        },
        // Handled by the early return above.
        Focus::Editor => {}
      },
    }
  }

  /// Cycle focus to the next pane: a side pane (sidebar or editor) toggles with
  /// the table; the table toggles back to whichever side pane the mode has. In
  /// one-shot mode (no side pane) focus stays on the table.
  fn toggle_focus(&mut self) {
    self.focus = match self.focus {
      Focus::Sidebar | Focus::Editor => Focus::Table,
      Focus::Table => {
        if self.sidebar.is_some() {
          Focus::Sidebar
        } else if self.editor.is_some() {
          Focus::Editor
        } else {
          Focus::Table
        }
      }
    };
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
/// node; Enter opens the selected relation — returned to the caller so it can
/// reset pagination — or, on a database/schema, toggles it.
fn on_sidebar_key(sidebar: &mut SidebarState, key: char) -> Option<RelationRef> {
  match key {
    'j' => sidebar.select_next(),
    'k' => sidebar.select_prev(),
    'g' => sidebar.select_first(),
    'G' => sidebar.select_last(),
    ' ' => sidebar.toggle(),
    '\n' | '\r' => {
      if let Some(relation) = sidebar.selected_relation() {
        return Some(relation);
      }
      sidebar.toggle();
    }
    _ => {}
  }
  None
}
