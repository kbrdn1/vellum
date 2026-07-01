//! Tests for the interactive runtime's pure key→action mapping. The full
//! event loop (draw / read / quit) needs a real terminal and is left to the
//! manual Phase 0 gate; `key_action` is the one piece of logic in it, and it is
//! pure — so the arrow/`Esc` aliasing is pinned here without a pty.

use ratatui::crossterm::event::KeyCode;
use vellum::driver::Capabilities;
use vellum::error::VellumError;
use vellum::model::catalog::{Catalog, Database, Relation, RelationKind, Schema};
use vellum::model::{Backend, Column, QueryResult, TypeKind, Value};
use vellum::tui::app::{App, PageTarget};
use vellum::tui::runtime::{apply_fetch, key_action, page_sql};
use vellum::tui::state::sidebar::RelationRef;

#[test]
fn char_keys_pass_through_unchanged() {
  for c in ['j', 'k', 'g', 'G', 'h', 'l', 'q', 'x', 'Z'] {
    assert_eq!(key_action(KeyCode::Char(c)), Some(c), "Char({c}) should pass through");
  }
}

#[test]
fn arrow_keys_alias_the_vim_motions() {
  assert_eq!(key_action(KeyCode::Left), Some('h'));
  assert_eq!(key_action(KeyCode::Right), Some('l'));
  assert_eq!(key_action(KeyCode::Up), Some('k'));
  assert_eq!(key_action(KeyCode::Down), Some('j'));
}

#[test]
fn esc_quits_like_q() {
  assert_eq!(key_action(KeyCode::Esc), Some('q'));
}

#[test]
fn enter_and_tab_carry_the_open_relation_and_focus_actions() {
  // Browse needs these: Enter opens the selected relation, Tab toggles focus —
  // the App already speaks `'\n'` / `'\t'`.
  assert_eq!(key_action(KeyCode::Enter), Some('\n'));
  assert_eq!(key_action(KeyCode::Tab), Some('\t'));
}

#[test]
fn unbound_keys_map_to_none() {
  // A representative sample of keys with no action — they must not be silently
  // turned into a motion.
  for code in [
    KeyCode::Backspace,
    KeyCode::Home,
    KeyCode::End,
    KeyCode::PageUp,
    KeyCode::PageDown,
    KeyCode::Delete,
    KeyCode::Insert,
    KeyCode::F(1),
  ] {
    assert_eq!(key_action(code), None, "{code:?} should be unbound");
  }
}

// ── Page query builder (#83) ──────────────────────────────────────────────

fn target(schema: &str, relation: &str, order_by: Option<&str>, limit: usize, offset: usize) -> PageTarget {
  PageTarget {
    relation: RelationRef {
      database: "main".into(),
      schema: schema.into(),
      relation: relation.into(),
    },
    limit,
    offset,
    order_by: order_by.map(str::to_string),
  }
}

#[test]
fn page_sql_qualifies_and_bounds_the_select() {
  let sql = page_sql(&target("main", "users", None, 51, 0));
  assert_eq!(sql, r#"SELECT * FROM "main"."users" LIMIT 51 OFFSET 0"#);
}

#[test]
fn page_sql_splices_the_order_by_before_the_limit() {
  let sql = page_sql(&target("main", "users", Some(r#"ORDER BY "name" ASC"#), 51, 50));
  assert_eq!(
    sql,
    r#"SELECT * FROM "main"."users" ORDER BY "name" ASC LIMIT 51 OFFSET 50"#
  );
}

#[test]
fn page_sql_quotes_identifiers_so_a_quote_cannot_break_out() {
  let sql = page_sql(&target("main", r#"a"b"#, None, 10, 0));
  assert_eq!(sql, r#"SELECT * FROM "main"."a""b" LIMIT 10 OFFSET 0"#);
}

#[test]
fn page_sql_omits_the_schema_when_empty() {
  let sql = page_sql(&target("", "users", None, 10, 0));
  assert_eq!(sql, r#"SELECT * FROM "users" LIMIT 10 OFFSET 0"#);
}

// ── Fetch routing (#85) ────────────────────────────────────────────────────

/// A browse `App` (one table so `apply_page` has a paginator to feed).
fn browse_app() -> App {
  let catalog = Catalog {
    databases: vec![Database {
      name: "main".into(),
      schemas: vec![Schema {
        name: String::new(),
        relations: vec![Relation {
          name: "users".into(),
          kind: RelationKind::Table,
          columns: vec![],
          foreign_keys: vec![],
        }],
      }],
    }],
  };
  let caps = Capabilities {
    explain: true,
    schemas: false,
    foreign_keys: true,
  };
  App::browse(catalog, caps, Backend::Sqlite)
}

fn rel(schema: &str, relation: &str) -> RelationRef {
  RelationRef {
    database: "main".into(),
    schema: schema.into(),
    relation: relation.into(),
  }
}

#[test]
fn apply_fetch_surfaces_the_error_and_keeps_the_session_on_failure() {
  // The whole point of #85: a page-query failure must NOT ride `?` out of the
  // loop and end the TUI. `apply_fetch` records it and returns normally.
  let mut app = browse_app();
  apply_fetch(
    &mut app,
    r#"SELECT * FROM "public"."orders""#.into(),
    Err(VellumError::Driver("no such table: orders".into())),
    &rel("public", "orders"),
  );
  let err = app.fetch_error().expect("the failure is surfaced");
  assert!(err.contains("public.orders"), "names the relation: {err:?}");
  assert!(err.contains("no such table"), "carries the driver message: {err:?}");
  assert!(!app.should_quit(), "the session stays alive after a failed fetch");
  assert_eq!(
    app.displayed_query(),
    None,
    "a failed fetch is not recorded as the displayed query"
  );
}

#[test]
fn apply_fetch_clears_the_error_and_records_the_query_on_success() {
  let mut app = browse_app();
  app.set_fetch_error("stale error from a prior page".into());
  let result = QueryResult {
    columns: vec![Column {
      name: "id".into(),
      kind: TypeKind::Int,
    }],
    rows: vec![vec![Value::Int(1)]],
    affected: None,
  };
  apply_fetch(
    &mut app,
    r#"SELECT * FROM "main"."users""#.into(),
    Ok(result),
    &rel("", "users"),
  );
  assert_eq!(app.fetch_error(), None, "a successful fetch clears the prior error");
  assert_eq!(
    app.displayed_query(),
    Some(r#"SELECT * FROM "main"."users""#),
    "records the query it ran"
  );
  assert_eq!(
    app.page_loaded_label().as_deref(),
    Some("1"),
    "the page rows are applied"
  );
}

#[test]
fn apply_fetch_clears_the_stale_page_on_failure() {
  // A good page of `users` is showing, then opening `orders` fails to fetch:
  // the old `users` rows must not linger under the new relation's title (codex
  // #91 P2). The displayed page is cleared alongside recording the error.
  let mut app = browse_app();
  apply_fetch(
    &mut app,
    r#"SELECT * FROM "main"."users""#.into(),
    Ok(QueryResult {
      columns: vec![Column {
        name: "id".into(),
        kind: TypeKind::Int,
      }],
      rows: vec![vec![Value::Int(1)], vec![Value::Int(2)]],
      affected: None,
    }),
    &rel("", "users"),
  );
  assert_eq!(app.page_loaded_label().as_deref(), Some("2"), "two rows are showing");

  apply_fetch(
    &mut app,
    r#"SELECT * FROM "public"."orders""#.into(),
    Err(VellumError::Driver("no such table: orders".into())),
    &rel("public", "orders"),
  );
  assert!(app.fetch_error().is_some(), "the error is recorded");
  assert_eq!(
    app.page_loaded_label().as_deref(),
    Some("0"),
    "the stale page is cleared, not left under the new title"
  );
  assert_eq!(
    app.displayed_query(),
    None,
    "the old successful SQL is cleared too — not shown over an empty grid"
  );
}
