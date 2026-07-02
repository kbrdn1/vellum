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
//! §4): `Json` gains a parsed `serde_json::Value` payload. Postgres adds
//! `Decimal(String)` (arbitrary-precision `numeric`, kept as exact text) and
//! `Array(Vec<Value>)` (per-element decode) — see #76.

use std::fmt;

/// The static type category of a value or column, normalised across backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
  Null,
  Bool,
  Int,
  Float,
  /// Arbitrary-precision `numeric` / `decimal` — exact, distinct from `Float`.
  Decimal,
  Text,
  Bytes,
  Json,
  Timestamp,
  /// A homogeneous array column (PG `int[]`, `text[]`, …).
  Array,
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
  /// Arbitrary-precision `numeric` / `decimal`, kept as its exact decimal text
  /// (never a lossy `f64`).
  Decimal(String),
  Text(String),
  Bytes(Vec<u8>),
  Json(String),
  Timestamp(String),
  /// A decoded array, one `Value` per element (`Null` for a NULL element).
  Array(Vec<Value>),
}

impl Value {
  /// The `TypeKind` this value belongs to. Total over every variant.
  pub fn kind(&self) -> TypeKind {
    match self {
      Value::Null => TypeKind::Null,
      Value::Bool(_) => TypeKind::Bool,
      Value::Int(_) => TypeKind::Int,
      Value::Float(_) => TypeKind::Float,
      Value::Decimal(_) => TypeKind::Decimal,
      Value::Text(_) => TypeKind::Text,
      Value::Bytes(_) => TypeKind::Bytes,
      Value::Json(_) => TypeKind::Json,
      Value::Timestamp(_) => TypeKind::Timestamp,
      Value::Array(_) => TypeKind::Array,
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
      Value::Text(s) | Value::Json(s) | Value::Timestamp(s) | Value::Decimal(s) => f.write_str(s),
      Value::Bytes(b) => write!(f, "<{} bytes>", b.len()),
      // PG-style `{a,b,c}`, recursive for nested arrays; a NULL element renders
      // as `NULL`. ponytail: elements aren't quoted (a text element with a comma
      // reads ambiguously) — this is a display string, not a round-trippable
      // array literal; the browse/CSV cell only needs to be readable.
      Value::Array(items) => {
        f.write_str("{")?;
        for (idx, item) in items.iter().enumerate() {
          if idx > 0 {
            f.write_str(",")?;
          }
          write!(f, "{item}")?;
        }
        f.write_str("}")
      }
    }
  }
}
