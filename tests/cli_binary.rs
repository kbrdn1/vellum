//! End-to-end tests that invoke the compiled `vellum` binary via assert_cmd.
//! These exercise the user-visible CLI surface (help, version, errors).
//!
//! `help_prints_subcommands` is the canary: every new subcommand added to
//! `src/cli.rs` must be reflected here (see CLAUDE.md / CONTRIBUTING.md).

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_prints_subcommands() {
  let mut cmd = Command::cargo_bin("vellum").unwrap();
  cmd.arg("--help");
  // No subcommands yet — assert the help renders with the usage line and the
  // about blurb. As Phase 0 adds `open` / `query`, assert each `  <name> `
  // row here exactly (two leading spaces, trailing space) like gwm's canary.
  cmd
    .assert()
    .success()
    .stdout(predicate::str::contains("Usage:"))
    .stdout(predicate::str::contains("SQL client"));
}

#[test]
fn version_prints_semver() {
  let mut cmd = Command::cargo_bin("vellum").unwrap();
  cmd.arg("--version");
  cmd
    .assert()
    .success()
    .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn unknown_flag_exits_nonzero() {
  // The exit-code contract: an unrecognised flag must fail (clap exits 2, so
  // assert `.failure()` rather than a specific code). Pins the spine that the
  // one-shot mode (Phase 0) will build its `exit 0 / exit 1` contract on.
  let mut cmd = Command::cargo_bin("vellum").unwrap();
  cmd.arg("--definitely-not-a-real-flag");
  cmd.assert().failure();
}
