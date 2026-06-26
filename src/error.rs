//! Crate-wide error type.
//!
//! User-facing paths return a `VellumError` variant rather than panicking —
//! `unwrap()` is reserved for tests and genuinely infallible spots (see
//! `CLAUDE.md`). New variants are added as modules grow (config parsing,
//! driver I/O, the write/diff engine).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VellumError {
  /// I/O failure at a filesystem or process boundary. Converts from
  /// `std::io::Error` via `?` so I/O paths thread errors up instead of
  /// panicking.
  #[error("I/O error: {0}")]
  Io(#[from] std::io::Error),

  /// Bad CLI argument or usage. First constructed when the one-shot
  /// argument surface lands (Phase 0).
  #[error("argument error: {0}")]
  Arg(String),

  /// Database driver failure — connect, query, or apply. First constructed
  /// by the SQLite driver (Phase 0); the category is frozen here so callers
  /// can match on it before the concrete drivers exist.
  #[error("driver error: {0}")]
  Driver(String),
}

pub type Result<T> = std::result::Result<T, VellumError>;
