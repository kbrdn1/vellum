//! Thin ratatui render of the result table. No app logic, no `println!`: it
//! turns the immutable [`App`] state into a `Table` widget and nothing more.
//! Vertical scroll-into-view is delegated to ratatui's stateful `TableState`
//! (keyed on the cursor); horizontal scroll honours `col_offset` by skipping
//! the hidden leading columns.

use ratatui::layout::Constraint;
use ratatui::style::Style;
use ratatui::widgets::{Block, Cell, Row, Table, TableState};
use ratatui::Frame;

use crate::tui::app::App;

/// Upper bound on a single column's rendered width, so one very wide cell can't
/// push every other column off-screen.
const MAX_COL_WIDTH: usize = 40;

/// Render `app`'s result table into the whole frame area.
pub fn render(frame: &mut Frame, app: &App) {
  let state = app.table();
  let offset = state.col_offset();
  let col_count = state.columns().len();

  let header =
    Row::new(state.columns().iter().skip(offset).map(|c| Cell::from(c.name.clone()))).style(Style::new().bold());

  let rows = state
    .rows()
    .iter()
    .map(|row| Row::new(row.iter().skip(offset).map(|v| Cell::from(v.to_string()))));

  // Width each visible column to its widest cell (header included), clamped so
  // the layout stays readable on both tiny and very wide values.
  let widths: Vec<Constraint> = (offset..col_count)
    .map(|i| {
      let mut w = state.columns()[i].name.len();
      for row in state.rows() {
        if let Some(cell) = row.get(i) {
          w = w.max(cell.to_string().len());
        }
      }
      Constraint::Length(w.clamp(3, MAX_COL_WIDTH) as u16)
    })
    .collect();

  let table = Table::new(rows, widths)
    .header(header)
    .row_highlight_style(Style::new().reversed())
    .highlight_symbol(">> ")
    .block(Block::bordered().title("vellum"));

  // ratatui's `TableState` computes the vertical scroll offset that keeps the
  // selected row on screen; we only feed it the cursor. No selection on an
  // empty result.
  let mut ts = TableState::default();
  if !state.rows().is_empty() {
    ts.select(Some(state.selected()));
  }
  frame.render_stateful_widget(table, frame.area(), &mut ts);
}
