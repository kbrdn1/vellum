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
fn db_without_query_launches_browse_which_needs_a_terminal() {
  // `--db` alone opens the interactive browse UI. Under assert_cmd stdout is a
  // pipe, so the up-front terminal guard refuses cleanly (exit non-zero) — it
  // must NOT block on a key read (the `ratatui::try_init` failure mode is not
  // fast on every platform; the guard is what makes this deterministic).
  let db = common::seeded_db();
  let mut cmd = Command::cargo_bin("vellum").unwrap();
  cmd.arg("--db").arg(db.path());
  cmd
    .assert()
    .failure()
    .stderr(predicate::str::contains("needs an interactive terminal"));
}

#[test]
fn interactive_without_db_is_a_usage_error() {
  // `-i` with no database/query asks to open something that isn't there; it is
  // a usage error, not a silent banner with exit 0.
  let mut cmd = Command::cargo_bin("vellum").unwrap();
  cmd.arg("--interactive");
  cmd.assert().failure();
}

/// Seed a one-row `items` table at `path` for the DSN-encoding tests.
fn seed_items(path: &std::path::Path) {
  common::seed_sql(
    path,
    &[
      "create table items (id integer, label text)",
      "insert into items (id, label) values (1, 'alpha')",
    ],
  );
}

#[test]
fn one_shot_opens_db_path_with_percent_and_hash() {
  // `%` (percent-decode) and `#` (fragment) are URL-structural for sqlx but
  // valid in filenames on every platform. The file the user named must open,
  // not a DSN-parsed variant. Pins `cli::sqlite_dsn`'s encoding cross-platform.
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("weird%name#1.sqlite");
  seed_items(&path);
  let mut cmd = Command::cargo_bin("vellum").unwrap();
  cmd.arg("--db").arg(&path).arg("select label from items order by id");
  cmd.assert().success().stdout(predicate::str::contains("alpha"));
}

// `?` is a valid filename byte on Unix but forbidden on Windows, so the
// query-separator case (the worst DSN mis-parse) is exercised only where such a
// file can exist.
#[cfg(unix)]
#[test]
fn one_shot_opens_db_path_with_question_mark() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("we?rd.sqlite");
  seed_items(&path);
  let mut cmd = Command::cargo_bin("vellum").unwrap();
  cmd.arg("--db").arg(&path).arg("select label from items order by id");
  cmd.assert().success().stdout(predicate::str::contains("alpha"));
}

#[test]
fn one_shot_escapes_tabs_and_newlines_in_cells() {
  // A TEXT cell containing a tab + newline must not break the TSV row/column
  // structure: it is escaped (`\t`, `\n`) on one line, not split across lines.
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("escape.sqlite");
  common::seed_sql(
    &path,
    &["create table t (v text)", "insert into t (v) values ('a\tb\nc')"],
  );
  let mut cmd = Command::cargo_bin("vellum").unwrap();
  cmd.arg("--db").arg(&path).arg("select v from t");
  cmd
    .assert()
    .success()
    // The literal escaped form appears; the raw tab/newline does not split it.
    .stdout(predicate::str::contains("a\\tb\\nc"));
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

#[test]
fn no_args_prints_banner_and_exits_zero() {
  // No `--db`/query and not interactive: the binary prints a usage banner and
  // exits 0 (the `(None, None)` branch of `cli::run`), not an error.
  let mut cmd = Command::cargo_bin("vellum").unwrap();
  cmd.assert().success().stdout(predicate::str::contains("--db"));
}

// A closed stdout (the reader hangs up, e.g. `vellum … | head`) must not crash
// the binary. On Unix, Rust ignores SIGPIPE, so the write returns `BrokenPipe`
// and `print_result` swallows it for a clean exit — this pins that it is exit
// 0, never a 101 panic. Windows pipe/SIGPIPE semantics differ, so it is
// Unix-only.
#[cfg(unix)]
#[test]
fn one_shot_survives_a_closed_stdout_pipe() {
  use std::io::Read as _;
  use std::process::{Command as Proc, Stdio};

  // A result far larger than the OS pipe buffer (~64 KiB) so the child is
  // guaranteed to be mid-write when we drop the read end.
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("big.sqlite");
  common::seed_sql(
    &path,
    &[
      "create table t (n integer)",
      "with recursive c(n) as (select 1 union all select n + 1 from c where n < 50000) \
       insert into t (n) select n from c",
    ],
  );

  let mut child = Proc::new(env!("CARGO_BIN_EXE_vellum"))
    .arg("--db")
    .arg(&path)
    .arg("select n from t")
    .stdout(Stdio::piped())
    .spawn()
    .expect("spawn vellum");

  // Read a little, then drop stdout — closing the pipe under the child's feet.
  let mut stdout = child.stdout.take().unwrap();
  let mut buf = [0u8; 16];
  let _ = stdout.read(&mut buf);
  drop(stdout);

  let status = child.wait().expect("wait for vellum");
  assert!(
    status.success(),
    "a closed stdout must exit cleanly, not crash: {status:?}"
  );
}
