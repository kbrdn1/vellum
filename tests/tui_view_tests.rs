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

#[test]
fn browse_colours_the_focused_pane_border() {
  // Focus is marked by border *colour*, not just weight: the focused pane's
  // border is the accent, the idle pane's is dim — a visible colour diff.
  let app = browse_app(); // focus starts on the sidebar
  let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
  terminal.draw(|f| view::render(f, &app)).unwrap();
  let buf = terminal.backend().buffer();
  // Row 1 (below the header) holds both panes' top borders: the sidebar's left
  // corner at col 0, the table's at the sidebar width (28).
  let sidebar_border = buf[(0, 1)].fg;
  let table_border = buf[(28, 1)].fg;
  assert_ne!(sidebar_border, table_border, "focused vs idle border differ in colour");
  assert_eq!(
    sidebar_border,
    Color::Cyan,
    "the focused (sidebar) border is the accent"
  );
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
fn browse_sidebar_uses_nerd_font_icons_per_node_kind() {
  // gwm working-tree style: each node carries a type glyph. The db node shows a
  // database glyph, the opened `users` table shows a table glyph.
  let out = render_to_string(&opened_browse_app(), 80, 14);
  assert!(
    out.contains('\u{f1c0}'),
    "the database node shows a database glyph:\n{out}"
  );
  assert!(out.contains('\u{f0ce}'), "the table node shows a table glyph:\n{out}");
}

#[test]
fn browse_sidebar_draws_tree_guides() {
  // The schema tree shows connector lines (├─ └─ with │ carried down from
  // ancestors), gwm working-tree style — not just indentation.
  let catalog = Catalog {
    databases: vec![Database {
      name: "app".into(),
      schemas: vec![Schema {
        name: "public".into(),
        relations: vec![
          Relation {
            name: "users".into(),
            kind: RelationKind::Table,
            columns: vec![CatColumn {
              name: "id".into(),
              data_type: "int".into(),
              nullable: false,
              primary_key: true,
            }],
            foreign_keys: vec![],
          },
          Relation {
            name: "orders".into(),
            kind: RelationKind::Table,
            columns: vec![],
            foreign_keys: vec![],
          },
        ],
      }],
    }],
  };
  let mut app = App::browse(
    catalog,
    Capabilities {
      explain: true,
      schemas: true,
      foreign_keys: true,
    },
    Backend::Postgres,
  );
  app.on_key(' '); // expand db
  app.on_key('j'); // onto schema
  app.on_key(' '); // expand schema -> users + orders (users has a `├`, orders a `└`)
  app.on_key('j'); // onto users
  app.on_key(' '); // expand users -> column id, carried under a `│`
  let lines = render_lines(&app, 90, 16);
  let joined = lines.join("\n");
  assert!(joined.contains('├'), "a branch connector is drawn:\n{joined}");
  // The column `id` nests under `users`, which is not the last relation, so a
  // vertical guide `│` runs to its left before the `└─` connector. Look inside
  // the sidebar's borders (cols 1..27) so the block borders don't count.
  let id_row = lines.iter().find(|l| l.contains(" id")).expect("column id row");
  let inside: String = id_row.chars().skip(1).take(26).collect();
  assert!(
    inside.contains('│') && inside.contains('└'),
    "the nested column carries a vertical guide + last-child connector:\n{id_row}"
  );
}

/// A browse app (schemas shown) over `app.public` with a table `users` and a
/// view `v_active`, expanded down to the relations.
fn schema_view_app() -> App {
  let catalog = Catalog {
    databases: vec![Database {
      name: "app".into(),
      schemas: vec![Schema {
        name: "public".into(),
        relations: vec![
          Relation {
            name: "users".into(),
            kind: RelationKind::Table,
            columns: vec![],
            foreign_keys: vec![],
          },
          Relation {
            name: "v_active".into(),
            kind: RelationKind::View,
            columns: vec![],
            foreign_keys: vec![],
          },
        ],
      }],
    }],
  };
  let mut app = App::browse(
    catalog,
    Capabilities {
      explain: true,
      schemas: true,
      foreign_keys: true,
    },
    Backend::Postgres,
  );
  app.on_key(' '); // expand db -> schema `public`
  app.on_key('j'); // onto the schema
  app.on_key(' '); // expand schema -> users + v_active
  app
}

#[test]
fn browse_sidebar_colours_schemas_yellow_and_views_magenta() {
  let app = schema_view_app();
  let mut terminal = Terminal::new(TestBackend::new(90, 16)).unwrap();
  terminal.draw(|f| view::render(f, &app)).unwrap();
  let buf = terminal.backend().buffer();
  let any_fg = |c: Color| (0..buf.area.height).any(|y| (0..buf.area.width).any(|x| buf[(x, y)].fg == c));
  assert!(any_fg(Color::Yellow), "a schema node is coloured yellow");
  assert!(any_fg(Color::Magenta), "a view node is coloured magenta");
}

#[test]
fn browse_sidebar_puts_two_spaces_after_the_icon() {
  let mut app = browse_app();
  app.on_key(' '); // expand db -> `users` (a table -> f0ce icon) visible
  let lines = render_lines(&app, 80, 12);
  let row = lines.iter().find(|l| l.contains("users")).expect("users row");
  let chars: Vec<char> = row.chars().collect();
  let gi = chars
    .iter()
    .position(|&c| c == '\u{f0ce}')
    .expect("table icon in the row");
  assert_eq!(
    (chars[gi + 1], chars[gi + 2]),
    (' ', ' '),
    "two spaces separate the icon from the label:\n{row}"
  );
}

#[test]
fn browse_sidebar_shows_a_node_counter() {
  let mut app = browse_app();
  app.on_key(' '); // expand db -> visible [main, users] = 2 nodes, cursor on main
  let out = render_to_string(&app, 80, 12);
  assert!(
    out.contains("1 of 2"),
    "the sidebar pane shows a `N of M` node counter:\n{out}"
  );
}

#[test]
fn browse_sidebar_has_no_expand_glyphs_and_a_left_cursor() {
  // No `▾`/`▸` expand markers; instead a left-pinned cursor `▶` that follows the
  // highlighted (selected) row.
  let mut app = browse_app();
  app.on_key(' '); // expand the db; the cursor stays on the db (index 0)
  let lines = render_lines(&app, 80, 12);
  let joined = lines.join("\n");
  assert!(
    !joined.contains('▾') && !joined.contains('▸'),
    "no expand markers in the tree:\n{joined}"
  );
  let cursor_rows: Vec<&String> = lines.iter().filter(|l| l.contains('▶')).collect();
  assert_eq!(
    cursor_rows.len(),
    1,
    "exactly one cursor row (the selection):\n{joined}"
  );
  assert!(
    cursor_rows[0].contains("main"),
    "the cursor marks the selected (db) row:\n{}",
    cursor_rows[0]
  );
}

#[test]
fn browse_sidebar_icons_cover_every_node_kind() {
  // Pin ALL five glyph arms, not just db/table (#90 review): a catalog with
  // schemas shown, a table AND a view, and an expanded table's columns — so the
  // schema / view / column arms render and a wrong glyph there would fail here.
  let catalog = Catalog {
    databases: vec![Database {
      name: "app".into(),
      schemas: vec![Schema {
        name: "public".into(),
        relations: vec![
          Relation {
            name: "users".into(),
            kind: RelationKind::Table,
            columns: vec![CatColumn {
              name: "id".into(),
              data_type: "int".into(),
              nullable: false,
              primary_key: true,
            }],
            foreign_keys: vec![],
          },
          Relation {
            name: "v_users".into(),
            kind: RelationKind::View,
            columns: vec![],
            foreign_keys: vec![],
          },
        ],
      }],
    }],
  };
  let mut app = App::browse(
    catalog,
    Capabilities {
      explain: true,
      schemas: true,
      foreign_keys: true,
    },
    Backend::Postgres,
  );
  app.on_key(' '); // expand db -> reveals the `public` schema
  app.on_key('j'); // onto the schema
  app.on_key(' '); // expand schema -> reveals users + v_users
  app.on_key('j'); // onto users
  app.on_key(' '); // expand users -> reveals its column `id`
  let out = render_to_string(&app, 90, 16);
  assert!(out.contains('\u{f1c0}'), "database glyph:\n{out}");
  assert!(out.contains('\u{f07c}'), "schema (folder) glyph:\n{out}");
  assert!(out.contains('\u{f0ce}'), "table glyph:\n{out}");
  assert!(out.contains('\u{f06e}'), "view (eye) glyph:\n{out}");
  assert!(out.contains('\u{f0db}'), "column glyph:\n{out}");
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
  // The `[2] …` table pane title border sits directly above the query (so the
  // query is inside the block, not floating above it). `[2]` is unique to the
  // table pane — the sidebar's `[1]` and the header carry neither.
  assert!(
    lines[qi - 1].contains("[2]"),
    "the table pane title border sits directly above the query:\n{}",
    lines[qi - 1]
  );
  // A separator rule sits between the query and the grid. Panes share buffer
  // rows (sidebar on the left), so only assert the dash run — the sidebar text
  // on the same row is expected.
  let sep = &lines[qi + 1];
  assert!(
    sep.matches('─').count() >= 10,
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

use ratatui::style::{Color, Modifier};
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
fn header_line_badges_the_engine_with_a_background_colour() {
  // The engine label reads as a filled badge (a background), not plain text.
  let line = header_line("sqlite", 40);
  let badge = line
    .spans
    .iter()
    .find(|s| s.content.contains("sqlite"))
    .expect("engine badge span");
  assert_eq!(
    badge.style.bg,
    Some(Color::Blue),
    "the engine label is a coloured badge"
  );
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
fn status_line_pins_the_context_on_the_left_before_the_hints() {
  // gwm-style: the context breadcrumb reads on the left; the key hints follow.
  let text = flat(&status_line("main.users", None, 80));
  assert!(text.contains("sort"), "key hints present: {text:?}");
  let ctx = text.find("main.users").expect("context present");
  let hints = text.find("Tab focus").expect("hints present");
  assert!(ctx < hints, "context sits left of the hints: {text:?}");
}

#[test]
fn status_line_keeps_the_context_by_shrinking_the_hints() {
  // At a medium width the full hints + context don't both fit; the context
  // stays (pinned left) and the hints shrink rather than the context vanishing.
  let text = flat(&status_line("main.users", None, 40));
  assert!(text.contains("main.users"), "context stays: {text:?}");
  assert_eq!(text.chars().count(), 40, "still padded to the exact width");
}

#[test]
fn status_line_places_the_hints_right_after_the_context() {
  // Context then hints, adjacent on the left; the right side is left blank
  // (reserved for the log message, #85) — the hints are NOT pinned right.
  let text = flat(&status_line("main.users", None, 80));
  let ctx_end = text.find("main.users").unwrap() + "main.users".len();
  let hints = text.find("Tab focus").expect("hints present");
  assert!(hints <= ctx_end + 2, "hints follow the context immediately: {text:?}");
  assert!(text.ends_with(' '), "the right side is blank (log slot): {text:?}");
}

#[test]
fn status_line_badges_the_context_with_a_background_colour() {
  // "colour" = a filled badge (a background), gwm-style — not just a fg tint.
  let line = status_line("main.users", None, 80);
  let ctx_span = line
    .spans
    .iter()
    .find(|s| s.content.contains("main.users"))
    .expect("context span");
  assert_eq!(
    ctx_span.style.bg,
    Some(Color::Cyan),
    "the context breadcrumb is a coloured badge"
  );
}

#[test]
fn status_line_pins_the_log_message_on_the_right() {
  // #85: a browse fetch error rides the right of the status line, bracketed
  // gwm-style, flush to the edge; the context + hints stay on the left.
  let text = flat(&status_line("main.users", Some("orders: no such table"), 80));
  assert!(text.contains("main.users"), "context still on the left: {text:?}");
  assert!(text.contains("Tab focus"), "hints still present: {text:?}");
  assert!(
    text.trim_end().ends_with("[orders: no such table]"),
    "the log is bracketed and pinned right: {text:?}"
  );
}

#[test]
fn status_line_omits_the_brackets_without_a_message() {
  // No message -> no empty `[]`; the right slot just stays blank (guards against
  // porting gwm's always-bracketed status).
  let text = flat(&status_line("main.users", None, 80));
  assert!(!text.contains('['), "no empty brackets when there is no log: {text:?}");
  assert!(text.ends_with(' '), "the right slot stays blank: {text:?}");
}

#[test]
fn status_line_survives_the_log_when_the_row_is_tight() {
  // Priority floor, gwm-style: when it's narrow the log wins — it clips alone
  // rather than a hint eating the error surface.
  let line = status_line("main.users", Some("boom"), 8);
  let text = flat(&line);
  assert_eq!(text.chars().count(), 8, "still exactly the width");
  assert!(
    text.contains("boom") || text.contains('…'),
    "the log survives tight width: {text:?}"
  );
}

#[test]
fn status_line_styles_the_hint_keys_like_gwm() {
  // gwm footer: the key glyph stands out (bold), the label reads muted — not one
  // flat dim run.
  let line = status_line("main.users", None, 80);
  let key = line
    .spans
    .iter()
    .find(|s| s.content == "Tab")
    .expect("a distinct hint-key span");
  assert!(
    key.style.add_modifier.contains(Modifier::BOLD),
    "the hint key is bold: {:?}",
    key.style
  );
  let label = line
    .spans
    .iter()
    .find(|s| s.content.contains("focus"))
    .expect("a hint-label span");
  assert!(
    !label.style.add_modifier.contains(Modifier::BOLD),
    "the hint label is not bold (muted): {:?}",
    label.style
  );
}

#[test]
fn status_line_never_exceeds_the_requested_width() {
  // Regression (codex #91 P2): when the hints truncate, the `…` marker plus its
  // separating space must fit inside `width` — not spill a cell past it (e.g.
  // `status_line("", None, 10)` was 11 cells).
  for w in 4..48 {
    let logged = flat(&status_line("main.users", Some("a fairly long error message"), w));
    assert!(
      logged.chars().count() <= w,
      "logged, width {w}: {logged:?} = {}",
      logged.chars().count()
    );
    let bare = flat(&status_line("", None, w));
    assert!(
      bare.chars().count() <= w,
      "bare, width {w}: {bare:?} = {}",
      bare.chars().count()
    );
  }
}

#[test]
fn status_line_colours_the_log_message() {
  // The error surface is visibly an error — a red-ish foreground, not the muted
  // hint colour.
  let line = status_line("main.users", Some("boom"), 80);
  let log = line
    .spans
    .iter()
    .find(|s| s.content.contains("boom"))
    .expect("a log span");
  assert_eq!(
    log.style.fg,
    Some(Color::Red),
    "the log reads as an error: {:?}",
    log.style
  );
}
