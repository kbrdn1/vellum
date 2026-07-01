//! Pure navigation state over a `Catalog` — the schema sidebar tree. Zero
//! ratatui, zero I/O: the tree is flattened to a list of *visible* nodes
//! (respecting expand/collapse), the cursor is a clamped index into that list,
//! and selecting a relation yields a [`RelationRef`] the browse view (#15) acts
//! on. Unit-tested in `tests/tui_app_tests.rs`.
//!
//! Node identity is an **index path** into the catalog, not a name path: the
//! catalog is introspected once and static, so indices are stable and cheap to
//! hash for the expanded set. The visible list is rebuilt on demand (the
//! catalog is tiny — caching would be speculative).
//!
//! Backends without real schemas (`capabilities().schemas == false`: SQLite,
//! MySQL) hide the schema *row* — relations sit directly under the database —
//! but a selected relation still carries its schema name, which the browse
//! query needs.

use std::collections::HashSet;

use crate::model::catalog::{Catalog, RelationKind, Schema};

/// Identifies a relation to browse: the path the query needs, by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationRef {
  pub database: String,
  pub schema: String,
  pub relation: String,
}

/// A node's index path into the catalog (stable while the catalog is static).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum NodeId {
  Database(usize),
  Schema(usize, usize),
  Relation(usize, usize, usize),
  Column(usize, usize, usize, usize),
}

/// The kind of a visible node — for the view to icon / indent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarKind {
  Database,
  Schema,
  Table,
  View,
  Column,
}

/// A flattened, currently-visible tree node.
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarNode {
  pub depth: usize,
  pub kind: SidebarKind,
  pub label: String,
  pub expandable: bool,
  pub expanded: bool,
  /// A child count to render in parentheses after the label (gwm-style), e.g.
  /// the number of relations under a database node. `None` for nodes without a
  /// meaningful count (relations, columns).
  pub count: Option<usize>,
  /// Whether this node is the last among its siblings — the view draws `└` vs
  /// `├` and carries `│` / ` ` down to descendants. (With `show_schemas ==
  /// false` and *several* schemas, relations of different schemas are flattened
  /// under the database, so this is per-schema, not per-database — a cosmetic
  /// edge case; the common single-schema / schemas-shown cases are exact.)
  pub is_last: bool,
}

/// Cursor + expand state over a catalog tree.
#[derive(Debug)]
pub struct SidebarState {
  catalog: Catalog,
  show_schemas: bool,
  expanded: HashSet<NodeId>,
  selected: usize,
}

impl SidebarState {
  /// Wrap a catalog, everything collapsed, cursor on the first node.
  /// `show_schemas` is `capabilities().schemas` — `false` collapses the schema
  /// level (SQLite / MySQL).
  pub fn new(catalog: Catalog, show_schemas: bool) -> Self {
    Self {
      catalog,
      show_schemas,
      expanded: HashSet::new(),
      selected: 0,
    }
  }

  /// The visible nodes, top to bottom (rebuilt on demand).
  pub fn visible_nodes(&self) -> Vec<SidebarNode> {
    self.visible().into_iter().map(|(_, node)| node).collect()
  }

  /// Index of the cursor within [`visible_nodes`](Self::visible_nodes).
  pub fn selected(&self) -> usize {
    self.selected
  }

  /// The first database's name — the browse connection's identity, for the
  /// header line. `None` on an empty catalog.
  pub fn database_name(&self) -> Option<&str> {
    self.catalog.databases.first().map(|db| db.name.as_str())
  }

  /// Total number of schemas across the catalog — the `[1] Schema (N)` pane
  /// title count (gwm-style). Counts every schema even when the schema *row* is
  /// hidden (`show_schemas == false`), since the catalog still models one.
  pub fn schema_count(&self) -> usize {
    self.catalog.databases.iter().map(|db| db.schemas.len()).sum()
  }

  /// Move the cursor down one visible node, clamped to the last.
  pub fn select_next(&mut self) {
    let last = self.visible().len().saturating_sub(1);
    if self.selected < last {
      self.selected += 1;
    }
  }

  /// Move the cursor up one visible node, clamped to the first.
  pub fn select_prev(&mut self) {
    self.selected = self.selected.saturating_sub(1);
  }

  /// Jump to the first visible node.
  pub fn select_first(&mut self) {
    self.selected = 0;
  }

  /// Jump to the last visible node.
  pub fn select_last(&mut self) {
    self.selected = self.visible().len().saturating_sub(1);
  }

  /// Expand or collapse the selected node, if it is expandable. Collapsing can
  /// shrink the visible list above the cursor, so the cursor is re-clamped.
  pub fn toggle(&mut self) {
    let toggle_id = self
      .visible()
      .get(self.selected)
      .filter(|(_, node)| node.expandable)
      .map(|(id, _)| *id);
    if let Some(id) = toggle_id {
      if self.expanded.contains(&id) {
        self.expanded.remove(&id);
      } else {
        self.expanded.insert(id);
      }
    }
    // Defensive re-clamp: with toggle acting on the *selected* node the cursor
    // can't dangle (the node it sits on stays visible), but a collapse that ever
    // shrinks the list above the cursor would — keep the cursor in range.
    let last = self.visible().len().saturating_sub(1);
    if self.selected > last {
      self.selected = last;
    }
  }

  /// The relation under the cursor, if the selected node is one — the
  /// open-browse target. Carries the schema name even when the schema row is
  /// hidden.
  pub fn selected_relation(&self) -> Option<RelationRef> {
    let visible = self.visible();
    let (id, _) = visible.get(self.selected)?;
    match id {
      NodeId::Relation(di, si, ri) => {
        let db = &self.catalog.databases[*di];
        let schema = &db.schemas[*si];
        Some(RelationRef {
          database: db.name.clone(),
          schema: schema.name.clone(),
          relation: schema.relations[*ri].name.clone(),
        })
      }
      _ => None,
    }
  }

  /// Walk the tree into visible `(id, node)` pairs, honouring expand state and
  /// the schema-level toggle.
  fn visible(&self) -> Vec<(NodeId, SidebarNode)> {
    let mut out = Vec::new();
    for (di, db) in self.catalog.databases.iter().enumerate() {
      let id = NodeId::Database(di);
      let expandable = if self.show_schemas {
        !db.schemas.is_empty()
      } else {
        db.schemas.iter().any(|s| !s.relations.is_empty())
      };
      let expanded = self.expanded.contains(&id);
      out.push((
        id,
        SidebarNode {
          depth: 0,
          kind: SidebarKind::Database,
          label: db.name.clone(),
          expandable,
          expanded,
          count: Some(db.schemas.iter().map(|s| s.relations.len()).sum()),
          is_last: di == self.catalog.databases.len() - 1,
        },
      ));
      if !expanded {
        continue;
      }
      for (si, schema) in db.schemas.iter().enumerate() {
        if self.show_schemas {
          let sid = NodeId::Schema(di, si);
          let s_expanded = self.expanded.contains(&sid);
          out.push((
            sid,
            SidebarNode {
              depth: 1,
              kind: SidebarKind::Schema,
              label: schema.name.clone(),
              expandable: !schema.relations.is_empty(),
              expanded: s_expanded,
              count: None,
              is_last: si == db.schemas.len() - 1,
            },
          ));
          if s_expanded {
            self.push_relations(&mut out, di, si, schema, 2);
          }
        } else {
          // Schema row hidden: relations sit directly under the database.
          self.push_relations(&mut out, di, si, schema, 1);
        }
      }
    }
    out
  }

  fn push_relations(&self, out: &mut Vec<(NodeId, SidebarNode)>, di: usize, si: usize, schema: &Schema, depth: usize) {
    for (ri, rel) in schema.relations.iter().enumerate() {
      let rid = NodeId::Relation(di, si, ri);
      let r_expanded = self.expanded.contains(&rid);
      out.push((
        rid,
        SidebarNode {
          depth,
          kind: match rel.kind {
            RelationKind::Table => SidebarKind::Table,
            RelationKind::View => SidebarKind::View,
          },
          label: rel.name.clone(),
          expandable: !rel.columns.is_empty(),
          expanded: r_expanded,
          count: None,
          is_last: ri == schema.relations.len() - 1,
        },
      ));
      if r_expanded {
        for (ci, col) in rel.columns.iter().enumerate() {
          out.push((
            NodeId::Column(di, si, ri, ci),
            SidebarNode {
              depth: depth + 1,
              kind: SidebarKind::Column,
              label: col.name.clone(),
              expandable: false,
              expanded: false,
              count: None,
              is_last: ci == rel.columns.len() - 1,
            },
          ));
        }
      }
    }
  }
}
