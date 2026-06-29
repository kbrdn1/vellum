//! End-to-end tests that invoke the compiled `vellum` binary via assert_cmd.
//! These exercise the user-visible CLI surface (help, version, the one-shot
//! query mode, and the exit-code contract).
//!
//! `help_prints_subcommands` is the canary: every new flag/subcommand added to
//! `src/cli.rs` must be reflected here (see CLAUDE.md / CONTRIBUTING.md).

use assert_cmd::Command;
use predicates::prelude::*;

mod common;

#[test]
fn help_prints_subcommands() {
  let mut cmd = Command::cargo_bin("vellum").unwrap();
  cmd.arg("--help");
  // No subcommands yet — the Phase 0 surface is flag-based: `--db <FILE>`, a
  // positional `[SQL]`, and `--interactive`. Assert the usage line, the about
  // blurb, and each option marker. As the surface grows, add each new
  // flag/subcommand row here (this canary is the gate).
  cmd
    .assert()
    .success()
    .stdout(predicate::str::contains("Usage:"))
    .stdout(predicate::str::contains("SQL client"))
    .stdout(predicate::str::contains("--db"))
    .stdout(predicate::str::contains("--interactive"));
}

#[test]
fn one_shot_query_prints_rows_and_exits_zero() {
  let db = common::seeded_db();
  let mut cmd = Command::cargo_bin("vellum").unwrap();
  cmd
    .arg("--db")
    .arg(db.path())
    .arg("select id, label from items order by id");
  cmd
    .assert()
    .success()
    .stdout(predicate::str::contains("alpha"))
    .stdout(predicate::str::contains("beta"))
    .stdout(predicate::str::contains("gamma"));
}

#[test]
fn one_shot_invalid_sql_exits_one() {
  // A syntactically broken query is rejected before it reaches the database;
  // the binary maps the driver error to exit code 1 (the contract `unknown_flag`
  // pins at the clap layer, here at the query layer).
  let db = common::seeded_db();
  let mut cmd = Command::cargo_bin("vellum").unwrap();
  cmd.arg("--db").arg(db.path()).arg("selct * from items");
  cmd.assert().failure().code(1);
}

#[test]
fn one_shot_rejected_write_exits_one() {
  // The read path refuses non-SELECT statements (the write gate is a separate,
  // later phase). A `delete` must not run — it exits 1, it does not silently
  // succeed.
  let db = common::seeded_db();
  let mut cmd = Command::cargo_bin("vellum").unwrap();
  cmd.arg("--db").arg(db.path()).arg("delete from items");
  cmd.assert().failure().code(1);
}

#[test]
fn db_without_query_is_a_usage_error() {
  // `--db` alone has nothing to run; it is a usage error (exit non-zero), not
  // a silent no-op.
  let db = common::seeded_db();
  let mut cmd = Command::cargo_bin("vellum").unwrap();
  cmd.arg("--db").arg(db.path());
  cmd.assert().failure();
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
