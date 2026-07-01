//! The browse TUI colour scheme (#92).
//!
//! A role-based [`Theme`]: every visual signal maps to a semantic role
//! (`focus`, `accent`, `text`, `dim`, `muted`, `selection_bg`, `schema`,
//! `view`, `error`) rather than a hardcoded `Color::Cyan`. The renderer reads
//! `theme.<role>`, so the palette lives in one place and the readability
//! decisions (which grey is a NULL, which orange is a focused border) are
//! stated once.
//!
//! vellum ships a single palette, **claude-dark** (the Anthropic orange
//! scheme), as the default — and, for now, the only one. A configurable
//! `[theme]` block in `.vellum.toml` (like gwm's) is a deliberate follow-up;
//! this module carries only the subset of roles the browse surface renders.
//! The role model mirrors gwm's `src/tui/theme.rs`.

use ratatui::style::Color;

/// Role-based colour scheme for the browse TUI. Every field is a
/// [`ratatui::style::Color`]; the renderer reads them instead of hard-coding
/// palette values. `Copy`, so it threads through the render path as a plain
/// value (in practice a single `const`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
  /// Focused pane border. Deeper than `accent` so a focused border reads as a
  /// distinct signal, not just a brighter badge.
  pub focus: Color,
  /// Primary accent: the engine badge, the status context chip, hint keys, and
  /// the grid header row.
  pub accent: Color,
  /// Primary cell text.
  pub text: Color,
  /// Secondary text: the page query line above the grid.
  pub dim: Color,
  /// De-emphasised: hint labels, `NULL` cells, idle pane borders, the
  /// query/grid separator rule.
  pub muted: Color,
  /// Selected-row background (grid + sidebar) — a surface fill, replacing the
  /// harsh full-row reverse so the row text stays on its own foreground.
  pub selection_bg: Color,
  /// Schema nodes in the sidebar tree.
  pub schema: Color,
  /// View nodes in the sidebar tree.
  pub view: Color,
  /// Error log on the status line.
  pub error: Color,
}

impl Default for Theme {
  /// claude-dark — the default and, for now, only palette.
  fn default() -> Self {
    Self::claude_dark()
  }
}

impl Theme {
  /// The Anthropic "Pure Dark" orange scheme. Palette ported from gwm's
  /// `Theme::claude_dark()`: the signature orange drives focus (`#C15F3C`
  /// borders) and accent (`#D4825D` badges/keys/header); the warm greys map to
  /// text/dim/muted/selection; yellow→schema, purple→view, red→error.
  pub const fn claude_dark() -> Self {
    Self {
      focus: Color::Rgb(0xC1, 0x5F, 0x3C),
      accent: Color::Rgb(0xD4, 0x82, 0x5D),
      text: Color::Rgb(0xE0, 0xE0, 0xE0),
      dim: Color::Rgb(0xB0, 0xB0, 0xB0),
      muted: Color::Rgb(0x99, 0x99, 0x99),
      selection_bg: Color::Rgb(0x38, 0x38, 0x38),
      schema: Color::Rgb(0xFF, 0xDF, 0x61),
      view: Color::Rgb(0xC7, 0x9B, 0xFF),
      error: Color::Rgb(0xFF, 0x7A, 0x7A),
    }
  }
}
