//! The browse TUI colour scheme (#92). `Theme` is a role-based palette — every
//! visual signal maps to a semantic role, so the render path reads `theme.<role>`
//! instead of a hardcoded `Color::Cyan`. vellum ships a single palette,
//! claude-dark, as the default; these tests pin the default and the exact RGB of
//! the roles the browse surface renders.

use ratatui::style::Color;
use vellum::tui::theme::Theme;

#[test]
fn default_is_claude_dark() {
  assert_eq!(
    Theme::default(),
    Theme::claude_dark(),
    "the default palette is claude-dark"
  );
}

#[test]
fn claude_dark_maps_roles_to_the_anthropic_palette() {
  let t = Theme::claude_dark();
  assert_eq!(t.focus, Color::Rgb(0xC1, 0x5F, 0x3C), "focused border = orange dark");
  assert_eq!(t.accent, Color::Rgb(0xD4, 0x82, 0x5D), "accent = primary orange");
  assert_eq!(t.text, Color::Rgb(0xE0, 0xE0, 0xE0), "primary text");
  assert_eq!(t.dim, Color::Rgb(0xB0, 0xB0, 0xB0), "secondary text");
  assert_eq!(t.muted, Color::Rgb(0x99, 0x99, 0x99), "muted");
  assert_eq!(
    t.selection_bg,
    Color::Rgb(0x38, 0x38, 0x38),
    "selection surface, not a reverse"
  );
  assert_eq!(t.schema, Color::Rgb(0xFF, 0xDF, 0x61), "schema nodes = warning yellow");
  assert_eq!(t.view, Color::Rgb(0xC7, 0x9B, 0xFF), "view nodes = special purple");
  assert_eq!(t.error, Color::Rgb(0xFF, 0x7A, 0x7A), "error log = red");
}
