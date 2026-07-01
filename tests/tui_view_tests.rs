//! Render smoke test for the result table. Unlike `tui_app_tests.rs` (pure
//! state), this drives a real render through ratatui's `TestBackend` to catch
//! panics in the column-windowing / width math that state tests can't see. It
//! asserts that rendering succeeds and surfaces content — not exact pixels.

use ratatui::backend::TestBackend;
use ratatui::Terminal;

use vellum::model::{Column, QueryResult, TypeKind, Value};
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
  )
}

#[test]
fn browse_renders_the_header_sidebar_and_status_without_panicking() {
  let mut app = browse_app();
  app.on_key(' '); // expand the database so `users` shows under it
  let out = render_to_string(&app, 80, 14);
  assert!(out.contains("vellum"), "the header version chip should appear:\n{out}");
  assert!(out.contains("main"), "the database appears (header + sidebar):\n{out}");
  assert!(out.contains("users"), "the expanded relation should appear:\n{out}");
  assert!(out.contains("sort"), "the status-line key hints should appear:\n{out}");
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
