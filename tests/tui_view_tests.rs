//! Render smoke test for the result table. Unlike `tui_app_tests.rs` (pure
//! state), this drives a real render through ratatui's `TestBackend` to catch
//! panics in the column-windowing / width math that state tests can't see. It
//! asserts that rendering succeeds and surfaces content — not exact pixels.

use ratatui::backend::TestBackend;
use ratatui::Terminal;

use vellum::model::{Backend, Column, QueryResult, TypeKind, Value};
use vellum::tui::app::App;
use vellum::tui::view;

/// A `cols`×`rows` app, cells holding `v{row}_{col}` text so we can assert a
/// specific value made it into the rendered buffer.
fn app_with(cols: usize, rows: usize) -> App {
  let columns = (0..cols)
    .map(|c| Column {
      name: format!("c{c}"),
      kind: TypeKind::Text,
    })
    .collect();
  let rows = (0..rows)
    .map(|r| (0..cols).map(|c| Value::Text(format!("v{r}_{c}"))).collect())
    .collect();
  App::new(QueryResult {
    columns,
    rows,
    affected: None,
  })
}

/// Render once into an off-screen `w`×`h` buffer and flatten it to a string.
fn render_to_string(app: &App, w: u16, h: u16) -> String {
  let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
  terminal.draw(|f| view::render(f, app)).unwrap();
  let buf = terminal.backend().buffer();
  let area = buf.area;
  let mut out = String::new();
  for y in 0..area.height {
    for x in 0..area.width {
      out.push_str(buf[(x, y)].symbol());
    }
    out.push('\n');
  }
  out
}

#[test]
fn renders_header_and_cells() {
  let app = app_with(2, 3);
  let out = render_to_string(&app, 40, 10);
  assert!(out.contains("c0"), "header column should appear:\n{out}");
  assert!(out.contains("v0_0"), "first cell value should appear:\n{out}");
}

#[test]
fn renders_empty_result_without_panicking() {
  // The assertion is that the render above does not panic on a 0×0 result.
  let _ = render_to_string(&app_with(0, 0), 20, 5);
}

#[test]
fn horizontal_scroll_reveals_later_columns() {
  let mut app = app_with(3, 2);
  app.on_key('l'); // scroll one column right: c0 scrolls off, c1 leads
  let out = render_to_string(&app, 40, 10);
  assert!(out.contains("c1"), "scrolled-to column should appear:\n{out}");
}

#[test]
fn one_shot_table_keeps_the_vellum_title_and_no_browse_counter() {
  // The one-shot result view (no sidebar) must not inherit the browse-only
  // chrome (#86 was scoped browse-only): it stays titled `vellum` with no
  // `N of N` cursor counter leaking in from the browse path.
  let out = render_to_string(&app_with(2, 3), 40, 10);
  assert!(out.contains("vellum"), "one-shot keeps the vellum title:\n{out}");
  assert!(!out.contains(" of "), "no browse row counter in one-shot:\n{out}");
}

// ── Browse two-pane render (#83) ──────────────────────────────────────────

use vellum::driver::Capabilities;
use vellum::model::catalog::{Catalog, Column as CatColumn, Database, Relation, RelationKind, Schema};

/// A browse app over one database `main` with a `users(id)` table.
fn browse_app() -> App {
  let catalog = Catalog {
    databases: vec![Database {
      name: "main".into(),
      schemas: vec![Schema {
        name: "main".into(),
        relations: vec![Relation {
          name: "users".into(),
          kind: RelationKind::Table,
          columns: vec![CatColumn {
            name: "id".into(),
            data_type: "int".into(),
            nullable: false,
            primary_key: true,
          }],
          foreign_keys: vec![],
        }],
      }],
    }],
  };
  App::browse(
    catalog,
    Capabilities {
      explain: true,
      schemas: false,
      foreign_keys: true,
    },
    Backend::Sqlite,
  )
}

#[test]
fn browse_renders_the_header_sidebar_and_status_without_panicking() {
  let mut app = browse_app();
  app.on_key(' '); // expand the database so `users` shows under it
  let out = render_to_string(&app, 80, 14);
  assert!(out.contains("sqlite"), "the header engine badge should appear:\n{out}");
  assert!(out.contains("main"), "the database appears in the sidebar:\n{out}");
  assert!(out.contains("users"), "the expanded relation should appear:\n{out}");
  assert!(out.contains("sort"), "the status-line key hints should appear:\n{out}");
}

/// Render once and split into rows, for structural (row-relative) assertions.
fn render_lines(app: &App, w: u16, h: u16) -> Vec<String> {
  render_to_string(app, w, h).lines().map(String::from).collect()
}

/// Open `users` and feed it a two-row page — the shared setup for the browse
/// chrome tests below.
fn opened_browse_app() -> App {
  let mut app = browse_app();
  app.on_key(' '); // expand db
  app.on_key('j'); // onto users
  app.on_key('\n'); // open it
  app.take_page_target(); // drain the open (the runtime would fetch)
  app.set_displayed_query(r#"SELECT * FROM "main"."users" ORDER BY "id" DESC LIMIT 51 OFFSET 0"#.into());
  app.apply_page(QueryResult {
    columns: vec![Column {
      name: "id".into(),
      kind: TypeKind::Int,
    }],
    rows: vec![vec![Value::Int(1)], vec![Value::Int(2)]],
    affected: None,
  });
  app
}

#[test]
fn browse_status_line_shows_the_context_breadcrumb() {
  // The status line carries the current context (`schema.relation`), not the
  // page range (#86 feedback — the `N of N` counter already gives position).
  let out = render_to_string(&opened_browse_app(), 80, 14);
  assert!(
    out.contains("main.users"),
    "status shows the context breadcrumb:\n{out}"
  );
  assert!(!out.contains("rows 1"), "the redundant page range is gone:\n{out}");
}

#[test]
fn browse_sidebar_pane_is_numbered_and_the_db_node_counts_relations() {
  let out = render_to_string(&opened_browse_app(), 80, 14);
  assert!(out.contains("[1] Schema"), "numbered sidebar pane title:\n{out}");
  assert!(out.contains("main (1)"), "db node shows its relation count:\n{out}");
}

#[test]
fn browse_table_pane_is_numbered_with_the_relation_and_loaded_count() {
  let out = render_to_string(&opened_browse_app(), 80, 14);
  assert!(
    out.contains("[2] users"),
    "numbered table pane title with the relation:\n{out}"
  );
  assert!(out.contains("(2)"), "loaded-rows count in the table title:\n{out}");
}

#[test]
fn browse_nests_the_query_inside_the_block_with_a_separator() {
  // The query sits INSIDE the bordered block (title border above it), then a
  // horizontal rule, then the grid — matching the gwm-style mock.
  let lines = render_lines(&opened_browse_app(), 80, 14);
  let qi = lines
    .iter()
    .position(|l| l.contains("SELECT"))
    .expect("query is rendered");
  assert!(
    lines[qi - 1].contains("users"),
    "the titled top border sits directly above the query:\n{}",
    lines[qi - 1]
  );
  let sep = &lines[qi + 1];
  assert!(
    sep.matches('─').count() >= 10 && !sep.contains("users"),
    "a separator rule sits between the query and the grid:\n{sep}"
  );
}

#[test]
fn browse_titles_the_table_with_the_relation_shows_query_and_counter() {
  let mut app = browse_app();
  app.on_key(' '); // expand db
  app.on_key('j'); // onto `users`
  app.on_key('\n'); // open it -> current relation = users
  app.take_page_target(); // drain the open (the runtime would fetch)
  app.set_displayed_query(r#"SELECT * FROM "main"."users" LIMIT 51 OFFSET 0"#.into());
  app.apply_page(QueryResult {
    columns: vec![Column {
      name: "id".into(),
      kind: TypeKind::Int,
    }],
    rows: vec![vec![Value::Int(1)], vec![Value::Int(2)]],
    affected: None,
  });
  let out = render_to_string(&app, 80, 14);
  assert!(out.contains("users"), "table titled with the relation name:\n{out}");
  assert!(
    out.contains("SELECT"),
    "the page query line appears above the table:\n{out}"
  );
  assert!(out.contains("of 2"), "the `N of N` row counter appears:\n{out}");
}

#[test]
fn browse_renders_an_unopened_table_pane_without_panicking() {
  // Before a relation is opened the result table is empty; the render must not
  // panic on the empty grid (and shows no counter).
  let _ = render_to_string(&browse_app(), 80, 12);
}

// ── Pure line/counter builders (#86, gwm-style — no ratatui backend) ───────

use ratatui::text::Line;
use vellum::tui::state::sort::toggle_sort;
use vellum::tui::view::{header_line, row_counter, sort_indicator, status_line};

/// Flatten a `Line`'s spans to their text, for content/width assertions.
fn flat(line: &Line) -> String {
  line.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[test]
fn header_line_pins_the_version_and_pads_to_width() {
  let text = flat(&header_line("main", 40));
  assert!(text.contains("main"), "db badge: {text:?}");
  assert!(text.contains("vellum"), "version chip: {text:?}");
  assert_eq!(text.chars().count(), 40, "padded to the exact width");
}

#[test]
fn header_line_without_a_database_is_just_the_padded_version() {
  let text = flat(&header_line("", 30));
  assert!(text.contains("vellum"));
  assert_eq!(text.chars().count(), 30);
}

#[test]
fn header_line_zero_width_is_empty() {
  assert_eq!(flat(&header_line("main", 0)), "");
}

#[test]
fn header_line_pads_wide_characters_by_display_width() {
  // A CJK database name is 2 terminal cells per char. Padding must be measured
  // in display width, not char count, or the line overflows `width` and shoves
  // the pinned version chip off-screen (#88).
  let line = header_line("数据库", 40);
  assert!(
    flat(&line).contains("vellum"),
    "version chip preserved: {:?}",
    flat(&line)
  );
  assert_eq!(line.width(), 40, "padded to the exact display width, not char count");
}

#[test]
fn header_line_truncates_a_wide_database_by_display_width() {
  // A database name far wider than the budget must truncate by display width so
  // the badge + chip still fit exactly, never overflowing (#88).
  let long = "数据库".repeat(20); // 60 CJK chars = 120 cells
  let line = header_line(&long, 30);
  assert!(
    flat(&line).contains("vellum"),
    "version chip survives truncation: {:?}",
    flat(&line)
  );
  assert_eq!(line.width(), 30, "truncated + padded to the exact display width");
}

#[test]
fn row_counter_is_one_based_and_hidden_when_empty() {
  assert_eq!(row_counter(3, 50).as_deref(), Some(" 3 of 50 "));
  assert_eq!(row_counter(1, 0), None, "an empty page shows no counter");
}

#[test]
fn sort_indicator_shows_only_the_descending_case() {
  let asc = toggle_sort(None, "name"); // ascending
  assert_eq!(sort_indicator(asc.as_ref()), None, "ascending stays clean");
  let desc = toggle_sort(asc, "name"); // descending
  assert_eq!(sort_indicator(desc.as_ref()).as_deref(), Some(" name ↓ "));
  assert_eq!(sort_indicator(None), None, "no sort -> nothing");
}

#[test]
fn status_line_shows_the_hints_and_pins_the_range() {
  let text = flat(&status_line("rows 1-50", 80));
  assert!(text.contains("sort"), "key hints present: {text:?}");
  assert!(text.contains("rows 1-50"), "range pinned: {text:?}");
}

#[test]
fn status_line_keeps_the_range_pinned_by_shrinking_the_hints() {
  // At a medium width the full hints + range don't both fit; the range must
  // stay pinned right (the hints shrink to make room) rather than vanish.
  let text = flat(&status_line("rows 1-50", 60));
  assert!(text.contains("rows 1-50"), "range stays pinned: {text:?}");
  assert_eq!(text.chars().count(), 60, "still padded to the exact width");
}
