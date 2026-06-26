//! Crate-wide error type.
//!
//! User-facing paths return a `VellumError` variant rather than panicking —
//! `unwrap()` is reserved for tests and genuinely infallible spots (see
//! `CLAUDE.md`). New variants are added as modules grow (config parsing,
//! driver I/O, the write/diff engine).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VellumError {
  #[error("I/O error: {0}")]
  Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, VellumError>;
