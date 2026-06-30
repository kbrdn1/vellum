//! Pure LIMIT/OFFSET pagination cursor for browsing a relation without loading
//! it all into RAM. Zero ratatui, zero I/O — unit-tested in
//! `tests/paginate_tests.rs`.
//!
//! **No `COUNT(*)`.** A total would mean a second round-trip and is meaningless
//! on a live, growing table. Instead the cursor over-fetches by one: [`limit`]
//! returns `page_size + 1`, the runtime fetches that many and reports the count
//! back via [`record`]. The extra "probe" row — present iff the fetch returned
//! more than `page_size` — is how [`has_next`] (and thus the [`next`] bound and
//! the [`counter`]) is known without a total. The probe row is never displayed.
//!
//! [`limit`]: Paginator::limit
//! [`record`]: Paginator::record
//! [`has_next`]: Paginator::has_next
//! [`next`]: Paginator::next
//! [`counter`]: Paginator::counter

/// Rows shown per browse page. Fixed for now; a later runtime integration can
/// size it to the terminal viewport.
pub const DEFAULT_PAGE_SIZE: usize = 50;

/// A page move the table pane asks the runtime to service — the pagination
/// analogue of the sidebar's open-browse intent. The runtime fetches the new
/// page and feeds it back through [`super::super::app::App::apply_page`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageRequest {
  Next,
  Prev,
}

/// A LIMIT/OFFSET cursor over one relation's rows.
#[derive(Debug)]
pub struct Paginator {
  page_size: usize,
  /// 0-based page index.
  page: usize,
  /// Rows the last fetch returned (`0` before any fetch), clamped to
  /// `page_size + 1` — the only count past `page_size` is the single probe row.
  loaded: usize,
}

impl Paginator {
  /// A cursor on page 0, nothing fetched yet. `page_size` must be non-zero.
  pub fn new(page_size: usize) -> Self {
    debug_assert!(page_size > 0, "page_size must be non-zero");
    Self {
      page_size: page_size.max(1),
      page: 0,
      loaded: 0,
    }
  }

  /// The `LIMIT` to fetch: one past the page, so the extra row probes for a
  /// next page without a `COUNT`.
  pub fn limit(&self) -> usize {
    self.page_size + 1
  }

  /// The `OFFSET` of the current page's first row.
  pub fn offset(&self) -> usize {
    self.page * self.page_size
  }

  /// Record how many rows the runtime fetched for the current page (clamped to
  /// `limit()`; anything past the probe row is ignored).
  pub fn record(&mut self, fetched: usize) {
    self.loaded = fetched.min(self.limit());
  }

  /// Rows to actually display: the page minus the probe row.
  pub fn visible(&self) -> usize {
    self.loaded.min(self.page_size)
  }

  /// Whether the probe row came back, i.e. there is at least one more page.
  pub fn has_next(&self) -> bool {
    self.loaded > self.page_size
  }

  /// Advance one page if there is a next; returns whether it moved. Clears the
  /// loaded count — the new page is unknown until the runtime [`record`]s it.
  ///
  /// [`record`]: Paginator::record
  pub fn next(&mut self) -> bool {
    if self.has_next() {
      self.page += 1;
      self.loaded = 0;
      true
    } else {
      false
    }
  }

  /// Retreat one page, saturating at the first; returns whether it moved.
  /// Clears the loaded count like [`next`](Paginator::next).
  pub fn prev(&mut self) -> bool {
    if self.page > 0 {
      self.page -= 1;
      self.loaded = 0;
      true
    } else {
      false
    }
  }

  /// A 1-based, inclusive row counter for the status line — `"rows 51-70"` —
  /// or `"no rows"` when the page is empty. No total (see the module docs).
  pub fn counter(&self) -> String {
    let visible = self.visible();
    if visible == 0 {
      return "no rows".to_string();
    }
    let start = self.offset() + 1;
    let end = self.offset() + visible;
    format!("rows {start}-{end}")
  }
}
