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
