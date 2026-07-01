//! Thin ratatui render. No app logic, no `println!`: it turns the immutable
//! [`App`] state into widgets and nothing more. The browse layout is a gwm-style
//! `header / body / status` stack; the body is the schema sidebar plus a result
//! pane (the page query on its own line, then the bordered table). The pure
//! line/counter builders ([`header_line`], [`row_counter`], [`sort_indicator`],
//! [`status_line`]) return `Line`/`String` with no backend, so they are
//! unit-tested directly in `tui_view_tests.rs`; the render fns just place them.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState};
use ratatui::Frame;

use crate::tui::app::{App, Focus};
use crate::tui::state::sort::{Sort, SortDir};

/// Upper bound on a single column's rendered width, so one very wide cell can't
/// push every other column off-screen.
const MAX_COL_WIDTH: usize = 40;

/// Fixed sidebar width — a schema tree needs far less than a percentage of a
/// wide terminal, and a stable width reads calmer than a reflowing split.
const SIDEBAR_WIDTH: u16 = 28;

/// Render `app`: the browse layout when there is a sidebar, otherwise the
/// one-shot full-frame result table.
pub fn render(frame: &mut Frame, app: &App) {
  match app.sidebar() {
    Some(_) => render_browse(frame, app),
    None => render_table(frame, app, frame.area()),
  }
}

/// Header line / body / status line, top to bottom. The body is the sidebar
/// beside the result pane.
fn render_browse(frame: &mut Frame, app: &App) {
  let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).split(frame.area());
  render_header(frame, app, rows[0]);
  let body = Layout::horizontal([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(0)]).split(rows[1]);
  render_sidebar(frame, app, body[0]);
  render_result(frame, app, body[1]);
  render_footer(frame, app, rows[2]);
}

/// Top bar: the database badge and the pinned `vellum <version>` chip.
fn render_header(frame: &mut Frame, app: &App, area: Rect) {
  let line = header_line(app.database_name().unwrap_or(""), area.width as usize);
  frame.render_widget(Paragraph::new(line), area);
}

/// The result pane: the page query on its own line, then the bordered table
/// below it (the table's top border is the separator).
fn render_result(frame: &mut Frame, app: &App, area: Rect) {
  let parts = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
  let query = truncate(app.displayed_query().unwrap_or(""), parts[0].width as usize);
  frame.render_widget(
    Paragraph::new(Line::from(Span::styled(
      query,
      Style::new().add_modifier(Modifier::DIM),
    ))),
    parts[0],
  );
  render_table(frame, app, parts[1]);
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

/// The result grid into `area`. The block is titled with the open relation's
/// name; a non-ascending sort shows top-right and a `N of N` cursor counter
/// bottom-right. Columns are widthed to their widest visible cell (clamped).
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

  let title = app
    .current_relation()
    .map(|r| r.relation.clone())
    .unwrap_or_else(|| "results".to_string());
  let mut block = Block::bordered()
    .title(title)
    .border_style(focus_style(app.focus() == Focus::Table));
  if let Some(indicator) = sort_indicator(app.sort()) {
    block = block.title_top(Line::from(indicator).right_aligned());
  }
  if let Some(counter) = row_counter(state.selected() + 1, state.rows().len()) {
    block = block.title_bottom(Line::from(counter).right_aligned());
  }

  let table = Table::new(rows, widths)
    .header(header)
    .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
    .highlight_symbol(">> ")
    .block(block);

  let mut ts = TableState::default();
  if !state.rows().is_empty() {
    ts.select(Some(state.selected()));
  }
  frame.render_stateful_widget(table, area, &mut ts);
}

/// Bottom bar: key hints on the left, the page range (`rows X-Y`) pinned right.
fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
  let line = status_line(app.page_counter().as_deref().unwrap_or(""), area.width as usize);
  frame.render_widget(Paragraph::new(line), area);
}

// ── Pure builders (no ratatui backend — unit-tested in tui_view_tests.rs) ──

/// The header line: a ` <database> ` badge on the left, the ` vellum <version> `
/// chip pinned right, space-padded to exactly `width`. Narrower than the chip,
/// the chip is clipped alone; zero width is an empty line.
pub fn header_line(database: &str, width: usize) -> Line<'static> {
  if width == 0 {
    return Line::default();
  }
  let version = format!(" vellum {} ", env!("CARGO_PKG_VERSION"));
  let version_w = version.chars().count();
  let version_style = Style::new().add_modifier(Modifier::REVERSED);
  if width <= version_w {
    return Line::from(Span::styled(truncate(&version, width), version_style));
  }
  let mut spans: Vec<Span<'static>> = Vec::new();
  let mut used = 0usize;
  if !database.is_empty() {
    let badge = truncate(&format!(" {database} "), width - version_w);
    used = badge.chars().count();
    spans.push(Span::styled(badge, Style::new().add_modifier(Modifier::BOLD)));
  }
  spans.push(Span::raw(" ".repeat(width.saturating_sub(used + version_w))));
  spans.push(Span::styled(version, version_style));
  Line::from(spans)
}

/// Key hints on the left, the page range pinned to the right, padded to `width`.
/// An empty range just leaves the hints.
pub fn status_line(range: &str, width: usize) -> Line<'static> {
  const HINTS: &str = " Tab focus · Enter open · n/p page · s sort · q quit ";
  if width == 0 {
    return Line::default();
  }
  let hints = truncate(HINTS, width);
  let hints_w = hints.chars().count();
  let range_text = if range.is_empty() {
    String::new()
  } else {
    format!(" {range} ")
  };
  let range_w = range_text.chars().count();
  let mut spans = vec![Span::styled(hints, Style::new().add_modifier(Modifier::DIM))];
  if range_w > 0 && hints_w + range_w <= width {
    spans.push(Span::raw(" ".repeat(width - hints_w - range_w)));
    spans.push(Span::styled(range_text, Style::new().add_modifier(Modifier::BOLD)));
  }
  Line::from(spans)
}

/// Bottom-right cursor counter, gwm-style: `" <selected> of <visible> "`, or
/// `None` when the page is empty (so the footer disappears rather than showing
/// ` 1 of 0 `). `selected` is 1-based.
pub fn row_counter(selected: usize, visible: usize) -> Option<String> {
  if visible == 0 {
    None
  } else {
    Some(format!(" {selected} of {visible} "))
  }
}

/// Top-right sort indicator — shown only when the direction is **not** the
/// default ascending (i.e. descending): `" <column> ↓ "`. Ascending or no sort
/// yields `None`, so the corner stays clean at rest.
pub fn sort_indicator(sort: Option<&Sort>) -> Option<String> {
  match sort {
    Some(sort) if sort.dir() == SortDir::Desc => Some(format!(" {} ↓ ", sort.column())),
    _ => None,
  }
}

/// Clamp `s` to `max` characters, appending `…` when it overflows.
fn truncate(s: &str, max: usize) -> String {
  if s.chars().count() <= max {
    s.to_string()
  } else if max == 0 {
    String::new()
  } else {
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
  }
}

/// Bold border when a pane has focus, plain otherwise.
fn focus_style(focused: bool) -> Style {
  if focused {
    Style::new().add_modifier(Modifier::BOLD)
  } else {
    Style::new()
  }
}
