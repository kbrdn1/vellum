//! Thin ratatui render. No app logic, no `println!`: it turns the immutable
//! [`App`] state into widgets and nothing more. Two layouts: the one-shot
//! full-frame result table, and the browse two-pane (schema sidebar + result
//! table + a status line). Vertical scroll-into-view is delegated to ratatui's
//! stateful `TableState`/`ListState` (keyed on the cursor); horizontal table
//! scroll honours `col_offset` by skipping the hidden leading columns. The
//! render path is smoke-tested in `tui_view_tests.rs` (no logic to unit-test).

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState};
use ratatui::Frame;

use crate::tui::app::{App, Focus};

/// Upper bound on a single column's rendered width, so one very wide cell can't
/// push every other column off-screen.
const MAX_COL_WIDTH: usize = 40;

/// Render `app`: the browse two-pane when there is a sidebar, otherwise the
/// one-shot full-frame result table.
pub fn render(frame: &mut Frame, app: &App) {
  match app.sidebar() {
    Some(_) => render_browse(frame, app),
    None => render_table(frame, app, frame.area()),
  }
}

/// Browse: schema sidebar on the left, the result table over a status line on
/// the right.
fn render_browse(frame: &mut Frame, app: &App) {
  let columns = Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(70)]).split(frame.area());
  render_sidebar(frame, app, columns[0]);
  let right = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(columns[1]);
  render_table(frame, app, right[0]);
  render_status(frame, app, right[1]);
}

/// The schema tree: each visible node indented by its depth, expandable nodes
/// marked, the cursor highlighted. The border brightens when the pane is focused.
fn render_sidebar(frame: &mut Frame, app: &App, area: Rect) {
  let Some(sidebar) = app.sidebar() else { return };
  let nodes = sidebar.visible_nodes();
  let items: Vec<ListItem> = nodes
    .iter()
    .map(|node| {
      let indent = "  ".repeat(node.depth);
      let marker = if node.expandable {
        if node.expanded {
          "▾ "
        } else {
          "▸ "
        }
      } else {
        ""
      };
      ListItem::new(format!("{indent}{marker}{}", node.label))
    })
    .collect();
  let list = List::new(items)
    .block(
      Block::bordered()
        .title("schema")
        .border_style(focus_style(app.focus() == Focus::Sidebar)),
    )
    .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
  let mut state = ListState::default();
  if !nodes.is_empty() {
    state.select(Some(sidebar.selected()));
  }
  frame.render_stateful_widget(list, area, &mut state);
}

/// The result grid into `area`. The header is bold, the selected row reversed;
/// columns are widthed to their widest visible cell (clamped). The border
/// brightens when the table pane is focused.
fn render_table(frame: &mut Frame, app: &App, area: Rect) {
  let state = app.table();
  let offset = state.col_offset();
  let col_count = state.columns().len();

  let header = Row::new(state.columns().iter().skip(offset).map(|c| Cell::from(c.name.clone())))
    .style(Style::new().add_modifier(Modifier::BOLD));

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
    .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
    .highlight_symbol(">> ")
    .block(
      Block::bordered()
        .title("vellum")
        .border_style(focus_style(app.focus() == Focus::Table)),
    );

  // ratatui's `TableState` computes the vertical scroll offset that keeps the
  // selected row on screen; we only feed it the cursor. No selection on an
  // empty result.
  let mut ts = TableState::default();
  if !state.rows().is_empty() {
    ts.select(Some(state.selected()));
  }
  frame.render_stateful_widget(table, area, &mut ts);
}

/// One-line status: the row counter, the active sort, and the key hints.
fn render_status(frame: &mut Frame, app: &App, area: Rect) {
  let counter = app.page_counter().unwrap_or_default();
  let sort = app.sort().map(|s| s.order_by_clause()).unwrap_or_default();
  let hints = "Tab focus · Enter open · n/p page · s sort · q quit";
  let line = [counter.as_str(), sort.as_str(), hints]
    .iter()
    .filter(|part| !part.is_empty())
    .cloned()
    .collect::<Vec<_>>()
    .join("   ");
  frame.render_widget(Paragraph::new(Line::from(line)), area);
}

/// Bold border when a pane has focus, plain otherwise.
fn focus_style(focused: bool) -> Style {
  if focused {
    Style::new().add_modifier(Modifier::BOLD)
  } else {
    Style::new()
  }
}
