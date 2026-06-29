//! The schema tree — `Catalog → Database → Schema → Relation → Column` plus
//! `ForeignKey`. Pure data, zero I/O: this is what `Driver::introspect()`
//! returns and what the sidebar, autocomplete and ERD all read from
//! (ARCHITECTURE §4).
//!
//! The four levels are kept even though engines collapse them (SQLite has one
//! database / `main` schema; MySQL's databases ≈ schemas). This model is the
//! general tree; the per-engine populator (#13) maps each backend onto it.
//!
//! Ordering is whatever the populator inserts — `Vec`s preserve it (columns
//! must stay in ordinal order). Navigation is by name via the `*(name)`
//! helpers; a name-sorted view, if ever needed, is an accessor to add then.

/// A connection's schema tree — one or more databases.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
  pub databases: Vec<Database>,
}

/// A database: a named set of schemas.
#[derive(Debug, Clone, PartialEq)]
pub struct Database {
  pub name: String,
  pub schemas: Vec<Schema>,
}

/// A schema: a named set of relations (tables and views).
#[derive(Debug, Clone, PartialEq)]
pub struct Schema {
  pub name: String,
  pub relations: Vec<Relation>,
}

/// Whether a relation is a base table or a view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationKind {
  Table,
  View,
}

/// A table or view: ordered columns plus any foreign keys it declares.
#[derive(Debug, Clone, PartialEq)]
pub struct Relation {
  pub name: String,
  pub kind: RelationKind,
  pub columns: Vec<Column>,
  pub foreign_keys: Vec<ForeignKey>,
}

/// A column. The `data_type` is the engine's type name verbatim (conservative,
/// like the `Value` model — normalisation is a separate concern).
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
  pub name: String,
  pub data_type: String,
  pub nullable: bool,
  pub primary_key: bool,
}

/// A foreign key: local columns referencing another relation's columns.
#[derive(Debug, Clone, PartialEq)]
pub struct ForeignKey {
  /// The constraint name, if the engine reports one.
  pub name: Option<String>,
  /// The local columns that make up the key, in order.
  pub columns: Vec<String>,
  /// What they reference.
  pub references: Reference,
}

/// The target of a [`ForeignKey`]. `schema` is `None` when the reference is in
/// the same schema as the owning relation.
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
  pub schema: Option<String>,
  pub relation: String,
  pub columns: Vec<String>,
}

impl Catalog {
  /// The database with this name, if present.
  pub fn database(&self, name: &str) -> Option<&Database> {
    self.databases.iter().find(|d| d.name == name)
  }
}

impl Database {
  /// The schema with this name, if present.
  pub fn schema(&self, name: &str) -> Option<&Schema> {
    self.schemas.iter().find(|s| s.name == name)
  }

  /// Resolve a foreign key declared in `from_schema` to the relation it
  /// references — following `references.schema` when set, else staying in
  /// `from_schema`. `None` if the target schema or relation is absent.
  pub fn resolve(&self, fk: &ForeignKey, from_schema: &str) -> Option<&Relation> {
    let target_schema = fk.references.schema.as_deref().unwrap_or(from_schema);
    self.schema(target_schema)?.relation(&fk.references.relation)
  }
}

impl Schema {
  /// The relation with this name, if present.
  pub fn relation(&self, name: &str) -> Option<&Relation> {
    self.relations.iter().find(|r| r.name == name)
  }
}

impl Relation {
  /// The column with this name, if present.
  pub fn column(&self, name: &str) -> Option<&Column> {
    self.columns.iter().find(|c| c.name == name)
  }
}
