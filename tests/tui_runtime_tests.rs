//! Tests for the interactive runtime's pure key→action mapping. The full
//! event loop (draw / read / quit) needs a real terminal and is left to the
//! manual Phase 0 gate; `key_action` is the one piece of logic in it, and it is
//! pure — so the arrow/`Esc` aliasing is pinned here without a pty.

use ratatui::crossterm::event::KeyCode;
use vellum::tui::app::PageTarget;
use vellum::tui::runtime::{key_action, page_sql};
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
