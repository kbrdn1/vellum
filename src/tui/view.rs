//! Thin ratatui render. No app logic, no `println!`: it turns the immutable
//! [`App`] state into widgets and nothing more. The browse layout is a gwm-style
//! `header / body / status` stack; the body is the `[1] Schema` sidebar plus the
//! `[2] <relation>` result pane, whose bordered block nests the page query, a
//! separator rule, then the grid. The pure line/counter builders
//! ([`header_line`], [`row_counter`], [`sort_indicator`], [`status_line`])
//! return `Line`/`String` with no backend, so they are unit-tested directly in
//! `tui_view_tests.rs`; the render fns just place them.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
    None => render_table(frame, app, frame.area(), false),
  }
}

/// Header line / body / status line, top to bottom. The body is the sidebar
/// beside the result pane.
fn render_browse(frame: &mut Frame, app: &App) {
  let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).split(frame.area());
  render_header(frame, app, rows[0]);
  let body = Layout::horizontal([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(0)]).split(rows[1]);
  render_sidebar(frame, app, body[0]);
  render_table(frame, app, body[1], true);
  render_footer(frame, app, rows[2]);
}

/// Top bar: the engine badge (`sqlite` / `postgres` / `mysql`) and the pinned
/// `vellum <version>` chip.
fn render_header(frame: &mut Frame, app: &App, area: Rect) {
  let engine = app.backend().map(|b| b.as_str()).unwrap_or("");
  frame.render_widget(Paragraph::new(header_line(engine, area.width as usize)), area);
}

/// The schema tree, gwm-style: a `[1] Schema (N)` pane title, each visible node
/// indented by its depth with an expand marker and its optional `(count)`, the
/// cursor prefixed with `▶ `. The border brightens when the pane is focused.
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
      let count = node.count.map(|c| format!(" ({c})")).unwrap_or_default();
      ListItem::new(format!("{indent}{marker}{}{count}", node.label))
    })
    .collect();
  let title = format!(" [1] Schema ({}) ", sidebar.schema_count());
  let list = List::new(items)
    .block(
      Block::bordered()
        .title(title)
        .border_style(focus_style(app.focus() == Focus::Sidebar)),
    )
    .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
    .highlight_symbol("▶ ");
  let mut state = ListState::default();
  if !nodes.is_empty() {
    state.select(Some(sidebar.selected()));
  }
  frame.render_stateful_widget(list, area, &mut state);
}

/// The result pane. In `browse` the bordered block is titled `[2] <relation>
/// (<loaded>)`, shows a non-ascending sort top-right and a `N of N` cursor
/// counter bottom-right, and nests the page query then a separator rule above
/// the grid (the gwm-style mock). The one-shot view (`browse == false`) is the
/// plain `vellum`-titled grid with none of that chrome.
fn render_table(frame: &mut Frame, app: &App, area: Rect, browse: bool) {
  let block = result_block(app, browse);
  let inner = block.inner(area);
  frame.render_widget(block, area);
  if browse {
    let parts = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(0)]).split(inner);
    render_query(frame, app, parts[0]);
    render_rule(frame, parts[1]);
    render_grid(frame, app, parts[2]);
  } else {
    render_grid(frame, app, inner);
  }
}

/// The bordered chrome around the grid: the pane title (`[2] <relation>
/// (<loaded>)` in browse, ` vellum ` one-shot), plus the top-right sort
/// indicator and the bottom-right cursor counter (browse only).
fn result_block(app: &App, browse: bool) -> Block<'static> {
  let block = Block::bordered().border_style(focus_style(app.focus() == Focus::Table));
  if !browse {
    return block.title(" vellum ");
  }
  let title = match app.current_relation() {
    Some(r) => {
      let count = app.page_loaded_label().map(|n| format!(" ({n})")).unwrap_or_default();
      format!(" [2] {}{count} ", r.relation)
    }
    None => " [2] results ".to_string(),
  };
  let mut block = block.title(title);
  if let Some(indicator) = sort_indicator(app.sort()) {
    block = block.title_top(Line::from(indicator).right_aligned());
  }
  if let Some(counter) = row_counter(app.table().selected() + 1, app.table().rows().len()) {
    block = block.title_bottom(Line::from(counter).right_aligned());
  }
  block
}

/// The page query on its own line, dimmed.
fn render_query(frame: &mut Frame, app: &App, area: Rect) {
  let query = truncate(app.displayed_query().unwrap_or(""), area.width as usize);
  frame.render_widget(
    Paragraph::new(Line::from(Span::styled(
      query,
      Style::new().add_modifier(Modifier::DIM),
    ))),
    area,
  );
}

/// A horizontal separator rule spanning `area`, dimmed — the divider between the
/// query line and the grid.
fn render_rule(frame: &mut Frame, area: Rect) {
  let rule = "─".repeat(area.width as usize);
  frame.render_widget(
    Paragraph::new(Line::from(Span::styled(rule, Style::new().add_modifier(Modifier::DIM)))),
    area,
  );
}

/// The result grid into `area` (no block — the caller draws the chrome). The
/// header is bold, the selected row reversed; columns are widthed to their
/// widest visible cell (clamped) after skipping the horizontally-scrolled
/// leading columns.
fn render_grid(frame: &mut Frame, app: &App, area: Rect) {
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

  let table = Table::new(rows, widths)
    .header(header)
    .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
    .highlight_symbol(">> ");

  let mut ts = TableState::default();
  if !state.rows().is_empty() {
    ts.select(Some(state.selected()));
  }
  frame.render_stateful_widget(table, area, &mut ts);
}

/// Bottom bar: key hints on the left, the context breadcrumb
/// (`schema.relation`) pinned right.
fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
  let line = status_line(&app.context_label(), area.width as usize);
  frame.render_widget(Paragraph::new(line), area);
}

// ── Pure builders (no ratatui backend — unit-tested in tui_view_tests.rs) ──

/// The header line: a ` <label> ` badge on the left (the engine tag), the
/// ` vellum <version> ` chip pinned right, space-padded to exactly `width`.
/// Narrower than the chip, the chip is clipped alone; zero width is an empty
/// line.
pub fn header_line(label: &str, width: usize) -> Line<'static> {
  if width == 0 {
    return Line::default();
  }
  let version = format!(" vellum {} ", env!("CARGO_PKG_VERSION"));
  let version_w = version.width();
  let version_style = Style::new().add_modifier(Modifier::REVERSED);
  if width <= version_w {
    return Line::from(Span::styled(truncate(&version, width), version_style));
  }
  let mut spans: Vec<Span<'static>> = Vec::new();
  let mut used = 0usize;
  if !label.is_empty() {
    let badge = truncate(&format!(" {label} "), width - version_w);
    used = badge.width();
    spans.push(Span::styled(
      badge,
      Style::new()
        .bg(Color::Blue)
        .fg(Color::White)
        .add_modifier(Modifier::BOLD),
    ));
  }
  spans.push(Span::raw(" ".repeat(width.saturating_sub(used + version_w))));
  spans.push(Span::styled(version, version_style));
  Line::from(spans)
}

/// gwm-style: the coloured context breadcrumb pinned to the left, the key hints
/// immediately after it (dim), then blank padding to `width`. The right edge is
/// left free for the log / status message (#85) — the hints are *not* pinned
/// right. The context is reserved first so it survives when the hints must
/// shrink; it is dropped only when even it can't fit.
pub fn status_line(context: &str, width: usize) -> Line<'static> {
  const HINTS: &str = " Tab focus · Enter open · n/p page · s sort · q quit ";
  if width == 0 {
    return Line::default();
  }
  let context_text = if context.is_empty() {
    String::new()
  } else {
    format!(" {context} ")
  };
  let context_w = context_text.width();
  let fits = context_w > 0 && context_w <= width;
  let shown_context_w = if fits { context_w } else { 0 };
  let hints = truncate(HINTS, width - shown_context_w);
  let hints_w = hints.width();
  let mut spans = Vec::new();
  if fits {
    spans.push(Span::styled(
      context_text,
      Style::new()
        .bg(Color::Cyan)
        .fg(Color::Black)
        .add_modifier(Modifier::BOLD),
    ));
  }
  spans.push(Span::styled(hints, Style::new().add_modifier(Modifier::DIM)));
  spans.push(Span::raw(" ".repeat(width - shown_context_w - hints_w)));
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

/// Clamp `s` to `max` terminal cells, appending `…` when it overflows. Width is
/// measured in display columns (a CJK/emoji char is 2), not `char` count, so the
/// result never exceeds `max` cells even on wide input.
fn truncate(s: &str, max: usize) -> String {
  if s.width() <= max {
    return s.to_string();
  }
  if max == 0 {
    return String::new();
  }
  // Reserve one cell for the ellipsis, then take whole chars until the next one
  // would spill past the budget (a 2-cell char can't half-fit).
  let budget = max - 1;
  let mut out = String::new();
  let mut w = 0usize;
  for ch in s.chars() {
    let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
    if w + cw > budget {
      break;
    }
    out.push(ch);
    w += cw;
  }
  out.push('…');
  out
}

/// Bold border when a pane has focus, plain otherwise.
fn focus_style(focused: bool) -> Style {
  if focused {
    Style::new().add_modifier(Modifier::BOLD)
  } else {
    Style::new()
  }
}
