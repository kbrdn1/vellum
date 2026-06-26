//! Unit tests for the crate-wide `VellumError`.
//!
//! Pins the typed-error contract the rest of the codebase builds on: the
//! `Io` variant converts via `From<std::io::Error>` (so `?` threads I/O
//! failures up user-facing paths instead of panicking), and every category
//! renders a human-readable message through its `thiserror` attribute.

use vellum::error::VellumError;

#[test]
fn io_error_converts_via_from_and_renders() {
  let io = std::io::Error::new(std::io::ErrorKind::NotFound, "missing file");
  let err: VellumError = io.into();
  assert!(matches!(err, VellumError::Io(_)));
  assert!(err.to_string().contains("missing file"));
}

#[test]
fn arg_error_renders_its_message() {
  let err = VellumError::Arg("unknown flag --frobnicate".into());
  assert!(err.to_string().contains("unknown flag --frobnicate"));
}

#[test]
fn driver_error_renders_its_message() {
  let err = VellumError::Driver("connection refused".into());
  assert!(err.to_string().contains("connection refused"));
}
