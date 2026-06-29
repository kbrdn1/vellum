//! Tests for the interactive runtime's pure key→action mapping. The full
//! event loop (draw / read / quit) needs a real terminal and is left to the
//! manual Phase 0 gate; `key_action` is the one piece of logic in it, and it is
//! pure — so the arrow/`Esc` aliasing is pinned here without a pty.

use ratatui::crossterm::event::KeyCode;
use vellum::tui::runtime::key_action;

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
fn unbound_keys_map_to_none() {
  // A representative sample of keys the table ignores — they must not be
  // silently turned into a motion.
  for code in [
    KeyCode::Enter,
    KeyCode::Tab,
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
