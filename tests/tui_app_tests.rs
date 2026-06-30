//! State-machine tests for the TUI result table — ratatui-free: they assert
//! navigation and quit *state*, never pixels (CLAUDE.md TUI test taxonomy).
//! The pixel/render path is smoke-tested separately in `tui_view_tests.rs`.

use vellum::model::{Column, QueryResult, TypeKind, Value};
use vellum::tui::app::App;

/// A `cols`×`rows` result grid; each cell holds its flattened `row*cols+col`
/// index so assertions can stay specific without a real database.
fn grid(cols: usize, rows: usize) -> QueryResult {
  let columns = (0..cols)
    .map(|c| Column {
      name: format!("c{c}"),
      kind: TypeKind::Int,
    })
    .collect();
  let rows = (0..rows)
    .map(|r| (0..cols).map(|c| Value::Int((r * cols + c) as i64)).collect())
    .collect();
  QueryResult {
    columns,
    rows,
    affected: None,
  }
}

#[test]
fn on_key_j_advances_cursor() {
  let mut app = App::new(grid(2, 3));
  assert_eq!(app.table().selected(), 0);
  app.on_key('j');
  assert_eq!(app.table().selected(), 1);
}

#[test]
fn on_key_j_is_bounded_at_last_row() {
  let mut app = App::new(grid(2, 3));
  for _ in 0..10 {
    app.on_key('j');
  }
  assert_eq!(app.table().selected(), 2, "cursor must not run past the last row");
}

#[test]
fn on_key_k_moves_up_and_saturates_at_zero() {
  let mut app = App::new(grid(2, 3));
  app.on_key('j');
  app.on_key('j');
  app.on_key('k');
  assert_eq!(app.table().selected(), 1);
  app.on_key('k');
  app.on_key('k');
  assert_eq!(app.table().selected(), 0, "cursor must not go negative");
}

#[test]
fn capital_g_jumps_last_and_g_jumps_first() {
  let mut app = App::new(grid(2, 5));
  app.on_key('G');
  assert_eq!(app.table().selected(), 4);
  app.on_key('g');
  assert_eq!(app.table().selected(), 0);
}

#[test]
fn horizontal_scroll_is_bounded_to_column_count() {
  let mut app = App::new(grid(3, 2));
  assert_eq!(app.table().col_offset(), 0);
  app.on_key('l');
  assert_eq!(app.table().col_offset(), 1);
  for _ in 0..10 {
    app.on_key('l');
  }
  // Bounded to the column count: the last column stays visible, the offset
  // never reaches `cols` (== 3 here).
  assert_eq!(app.table().col_offset(), 2);
  app.on_key('h');
  assert_eq!(app.table().col_offset(), 1);
  for _ in 0..10 {
    app.on_key('h');
  }
  assert_eq!(app.table().col_offset(), 0, "column offset must not go negative");
}

#[test]
fn q_sets_quit() {
  let mut app = App::new(grid(1, 1));
  assert!(!app.should_quit());
  app.on_key('q');
  assert!(app.should_quit());
}

#[test]
fn unbound_key_is_a_noop() {
  let mut app = App::new(grid(2, 2));
  app.on_key('x');
  assert_eq!(app.table().selected(), 0);
  assert_eq!(app.table().col_offset(), 0);
  assert!(!app.should_quit());
}

#[test]
fn navigation_on_empty_result_stays_in_bounds() {
  let mut app = App::new(grid(0, 0));
  app.on_key('j');
  app.on_key('G');
  app.on_key('l');
  assert_eq!(app.table().selected(), 0);
  assert_eq!(app.table().col_offset(), 0);
  assert!(!app.should_quit());
}

// ── Schema sidebar (#14) ──────────────────────────────────────────────────

use vellum::driver::Capabilities;
use vellum::model::catalog::{Catalog, Column as CatColumn, Database, Relation, RelationKind, Schema};
use vellum::tui::app::Focus;
use vellum::tui::state::paginate::PageRequest;
use vellum::tui::state::sidebar::{RelationRef, SidebarKind};

fn cat_column(name: &str) -> CatColumn {
  CatColumn {
    name: name.into(),
    data_type: "int".into(),
    nullable: true,
    primary_key: false,
  }
}

/// A catalog: database `app`, schema `public`, tables `users(id, email)` and
/// `orders(id)`.
fn catalog() -> Catalog {
  Catalog {
    databases: vec![Database {
      name: "app".into(),
      schemas: vec![Schema {
        name: "public".into(),
        relations: vec![
          Relation {
            name: "users".into(),
            kind: RelationKind::Table,
            columns: vec![cat_column("id"), cat_column("email")],
            foreign_keys: vec![],
          },
          Relation {
            name: "orders".into(),
            kind: RelationKind::Table,
            columns: vec![cat_column("id")],
            foreign_keys: vec![],
          },
        ],
      }],
    }],
  }
}

fn caps(schemas: bool) -> Capabilities {
  Capabilities {
    explain: true,
    schemas,
    foreign_keys: true,
  }
}

#[test]
fn browse_starts_focused_on_the_sidebar() {
  let app = App::browse(catalog(), caps(true));
  assert_eq!(app.focus(), Focus::Sidebar);
  assert!(app.sidebar().is_some(), "browse mode has a sidebar");
  // Collapsed: only the database row is visible.
  assert_eq!(app.sidebar().unwrap().visible_nodes().len(), 1);
}

#[test]
fn tab_toggles_focus_between_sidebar_and_table() {
  let mut app = App::browse(catalog(), caps(true));
  app.on_key('\t');
  assert_eq!(app.focus(), Focus::Table);
  app.on_key('\t');
  assert_eq!(app.focus(), Focus::Sidebar);
}

#[test]
fn tab_does_nothing_in_one_shot_mode() {
  // No sidebar to focus — `Tab` is inert, focus stays on the table.
  let mut app = App::new(grid(2, 3));
  assert_eq!(app.focus(), Focus::Table);
  app.on_key('\t');
  assert_eq!(app.focus(), Focus::Table);
}

#[test]
fn space_expands_then_collapses_a_database() {
  let mut app = App::browse(catalog(), caps(true));
  let nodes = |app: &App| app.sidebar().unwrap().visible_nodes().len();
  assert_eq!(nodes(&app), 1, "collapsed: just the database");
  app.on_key(' ');
  assert_eq!(nodes(&app), 2, "expanded: the database + its `public` schema");
  app.on_key(' ');
  assert_eq!(nodes(&app), 1, "collapsed again");
}

#[test]
fn enter_on_a_relation_emits_the_open_browse_intent() {
  // Schema level hidden (SQLite/MySQL): the database expands straight to its
  // relations, but the intent still carries the schema the browse query needs.
  let mut app = App::browse(catalog(), caps(false));
  app.on_key(' '); // expand the database → [app, users, orders]
  app.on_key('j'); // move onto `users`
  assert!(
    app.take_browse_intent().is_none(),
    "no intent until a relation is opened"
  );
  app.on_key('\n'); // open it
  assert_eq!(
    app.take_browse_intent(),
    Some(RelationRef {
      database: "app".into(),
      schema: "public".into(),
      relation: "users".into(),
    })
  );
  assert!(app.take_browse_intent().is_none(), "intent is cleared on read");
}

#[test]
fn without_schemas_the_schema_row_is_hidden() {
  // With `schemas: false` the database expands directly to relations — no
  // schema row — so two relations + the database = three visible nodes.
  let mut app = App::browse(catalog(), caps(false));
  app.on_key(' ');
  let labels: Vec<String> = app
    .sidebar()
    .unwrap()
    .visible_nodes()
    .iter()
    .map(|n| n.label.clone())
    .collect();
  assert_eq!(labels, ["app", "users", "orders"], "no `public` row, got {labels:?}");
}

#[test]
fn with_schemas_the_schema_row_is_shown() {
  let mut app = App::browse(catalog(), caps(true));
  app.on_key(' '); // expand database → schema row appears
  app.on_key('j'); // onto `public`
  app.on_key(' '); // expand schema → relations appear
  let labels: Vec<String> = app
    .sidebar()
    .unwrap()
    .visible_nodes()
    .iter()
    .map(|n| n.label.clone())
    .collect();
  assert_eq!(labels, ["app", "public", "users", "orders"]);
}

#[test]
fn expanding_a_relation_reveals_then_hides_its_columns() {
  // The deepest level: a relation flattens to its columns when expanded. Schema
  // row hidden so the path stays short: db → relation → columns.
  let mut app = App::browse(catalog(), caps(false));
  app.on_key(' '); // expand db → [app, users, orders]
  app.on_key('j'); // onto `users`
  app.on_key(' '); // expand `users` → its columns appear under it

  let nodes = app.sidebar().unwrap().visible_nodes();
  let labels: Vec<&str> = nodes.iter().map(|n| n.label.as_str()).collect();
  assert_eq!(
    labels,
    ["app", "users", "id", "email", "orders"],
    "columns sit under their relation, got {labels:?}"
  );
  // The column rows carry the right kind and indent one level past the relation.
  let id = &nodes[2];
  assert_eq!(id.kind, SidebarKind::Column);
  assert_eq!(id.depth, 2, "columns indent one past a depth-1 relation");
  assert!(!id.expandable, "a column is a leaf");

  app.on_key(' '); // collapse `users` → columns vanish
  let labels: Vec<String> = app
    .sidebar()
    .unwrap()
    .visible_nodes()
    .iter()
    .map(|n| n.label.clone())
    .collect();
  assert_eq!(labels, ["app", "users", "orders"], "columns hidden again");
}

#[test]
fn sidebar_capital_g_jumps_last_and_g_jumps_first() {
  let mut app = App::browse(catalog(), caps(false));
  app.on_key(' '); // [app, users, orders]
  app.on_key('G');
  assert_eq!(app.sidebar().unwrap().selected(), 2, "G jumps to the last node");
  app.on_key('g');
  assert_eq!(app.sidebar().unwrap().selected(), 0, "g jumps to the first node");
}

#[test]
fn enter_on_a_database_toggles_it_without_intent() {
  // Enter on a non-relation node (the database) behaves like Space: it toggles
  // expansion and emits no browse intent.
  let mut app = App::browse(catalog(), caps(false));
  app.on_key('\n');
  assert_eq!(
    app.sidebar().unwrap().visible_nodes().len(),
    3,
    "Enter expanded the database"
  );
  assert!(app.take_browse_intent().is_none(), "a database opens nothing");
  app.on_key('\n');
  assert_eq!(
    app.sidebar().unwrap().visible_nodes().len(),
    1,
    "Enter collapsed it again"
  );
}

#[test]
fn a_view_relation_maps_to_the_view_kind() {
  // Tables and views are distinct kinds so the view can icon them apart.
  let cat = Catalog {
    databases: vec![Database {
      name: "app".into(),
      schemas: vec![Schema {
        name: "public".into(),
        relations: vec![Relation {
          name: "active_users".into(),
          kind: RelationKind::View,
          columns: vec![cat_column("id")],
          foreign_keys: vec![],
        }],
      }],
    }],
  };
  let mut app = App::browse(cat, caps(false));
  app.on_key(' '); // expand db → the view row appears
  let view = &app.sidebar().unwrap().visible_nodes()[1];
  assert_eq!(view.label, "active_users");
  assert_eq!(view.kind, SidebarKind::View, "a view maps to the View kind, not Table");
}

#[test]
fn sidebar_cursor_clamps_at_both_ends() {
  let mut app = App::browse(catalog(), caps(false));
  app.on_key(' '); // [app, users, orders]
  for _ in 0..5 {
    app.on_key('j');
  }
  assert_eq!(
    app.sidebar().unwrap().selected(),
    2,
    "cursor must not run past the last node"
  );
  for _ in 0..5 {
    app.on_key('k');
  }
  assert_eq!(app.sidebar().unwrap().selected(), 0, "cursor must not go negative");
}

// ── Paginated browse (#15) ────────────────────────────────────────────────
//
// `apply_page(result)` stands in for the runtime fetching a page: the result
// carries up to `page_size + 1` rows (50 + 1 probe here), the App trims the
// probe off the display and updates the counter. `n`/`p` page the table pane.

#[test]
fn browse_shows_a_row_counter_once_a_page_loads() {
  let mut app = App::browse(catalog(), caps(false));
  assert_eq!(app.page_counter().as_deref(), Some("no rows"), "nothing fetched yet");
  app.apply_page(grid(2, 51)); // a full page plus the probe row
  assert_eq!(app.page_counter().as_deref(), Some("rows 1-50"));
  assert_eq!(app.table().rows().len(), 50, "the probe row is trimmed off the display");
}

#[test]
fn n_requests_the_next_page_and_the_counter_advances() {
  let mut app = App::browse(catalog(), caps(false));
  app.apply_page(grid(2, 51)); // page 0, has a next
  app.on_key('\t'); // focus the table pane — pagination lives there
  app.on_key('n');
  assert_eq!(app.take_page_request(), Some(PageRequest::Next));
  assert!(app.take_page_request().is_none(), "request cleared on read");
  app.apply_page(grid(2, 20)); // runtime fetched page 1 (a partial last page)
  assert_eq!(app.page_counter().as_deref(), Some("rows 51-70"));
}

#[test]
fn n_is_inert_on_the_last_page() {
  let mut app = App::browse(catalog(), caps(false));
  app.apply_page(grid(2, 30)); // partial page, no probe -> no next page
  app.on_key('\t');
  app.on_key('n');
  assert!(app.take_page_request().is_none(), "no next page to request");
  assert_eq!(
    app.page_counter().as_deref(),
    Some("rows 1-30"),
    "still on the only page"
  );
}

#[test]
fn p_requests_the_previous_page() {
  let mut app = App::browse(catalog(), caps(false));
  app.apply_page(grid(2, 51));
  app.on_key('\t');
  app.on_key('n'); // -> page 1 requested
  app.take_page_request();
  app.apply_page(grid(2, 20));
  app.on_key('p');
  assert_eq!(app.take_page_request(), Some(PageRequest::Prev));
}

#[test]
fn n_and_p_do_nothing_until_the_table_is_focused() {
  // Browse opens focused on the sidebar; `n`/`p` there must not page.
  let mut app = App::browse(catalog(), caps(false));
  app.apply_page(grid(2, 51));
  assert_eq!(app.focus(), Focus::Sidebar);
  app.on_key('n');
  app.on_key('p');
  assert!(app.take_page_request().is_none(), "sidebar focus -> no paging");
}

#[test]
fn one_shot_mode_has_no_pagination() {
  let mut app = App::new(grid(2, 3));
  assert_eq!(app.page_counter(), None, "one-shot has no paginator");
  app.on_key('n'); // unbound in one-shot — must not panic or page
  app.on_key('p');
  assert!(app.take_page_request().is_none());
  assert_eq!(app.table().rows().len(), 3, "table untouched");
}
