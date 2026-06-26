//! The normalised, cross-database cell `Value` and its static `TypeKind`.
//!
//! `Value` is the seam that absorbs per-backend type divergence (ARCHITECTURE
//! risk #1). It is deliberately conservative for Phase 0: SQLite has only
//! NULL / INTEGER / REAL / TEXT / BLOB storage classes, so the SQLite driver
//! only ever builds `Null` / `Int` / `Float` / `Text` / `Bytes`. `Bool`,
//! `Json`, and `Timestamp` are contract placeholders for later backends —
//! present in the type, not yet constructed by any driver.
//!
//! Extension path (when the backend that needs it lands — see ARCHITECTURE
//! §4): `Json` gains a parsed `serde_json::Value` payload, and Postgres adds
//! `Decimal(String)`, `Uuid(..)`, and `Array(Vec<Value>)`.

use std::fmt;

/// The static type category of a value or column, normalised across backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
  Null,
  Bool,
  Int,
  Float,
  Text,
  Bytes,
  Json,
  Timestamp,
}

/// A single cell value, normalised across database engines.
///
/// `Json` and `Timestamp` carry their raw text for now (SQLite stores both as
/// TEXT); they gain typed payloads when a backend that needs them lands.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
  Null,
  Bool(bool),
  Int(i64),
  Float(f64),
  Text(String),
  Bytes(Vec<u8>),
  Json(String),
  Timestamp(String),
}

impl Value {
  /// The `TypeKind` this value belongs to. Total over every variant.
  pub fn kind(&self) -> TypeKind {
    match self {
      Value::Null => TypeKind::Null,
      Value::Bool(_) => TypeKind::Bool,
      Value::Int(_) => TypeKind::Int,
      Value::Float(_) => TypeKind::Float,
      Value::Text(_) => TypeKind::Text,
      Value::Bytes(_) => TypeKind::Bytes,
      Value::Json(_) => TypeKind::Json,
      Value::Timestamp(_) => TypeKind::Timestamp,
    }
  }
}

impl fmt::Display for Value {
  /// Canonical cell rendering used by the one-shot CLI and the TUI table.
  /// Bytes render as `<N bytes>` rather than a raw blob dump.
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Value::Null => f.write_str("NULL"),
      Value::Bool(b) => write!(f, "{b}"),
      Value::Int(i) => write!(f, "{i}"),
      Value::Float(x) => write!(f, "{x}"),
      Value::Text(s) | Value::Json(s) | Value::Timestamp(s) => f.write_str(s),
      Value::Bytes(b) => write!(f, "<{} bytes>", b.len()),
    }
  }
}
