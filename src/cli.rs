//! CLI surface (clap). Phase 0 ships the one-shot mode: `vellum --db <FILE>
//! "<SQL>"` connects the SQLite `Driver`, runs the (read-only) query, and
//! prints the rows to stdout — exit 0 on success, exit 1 on a query/driver
//! error. `--interactive` renders the same result in the scrollable TUI table
//! instead (vim navigation, `q` to quit). `run` is `async` because the driver
//! layer it dispatches to is async on the tokio runtime bootstrapped in `main`.

use std::path::Path;

use clap::Parser;

use crate::driver::{Driver, SqliteDriver};
use crate::error::{Result, VellumError};
use crate::model::QueryResult;
use crate::tui;

#[derive(Debug, Parser)]
#[command(
  name = "vellum",
  bin_name = "vellum",
  version,
  about = "TUI SQL client — browse, query, edit databases in the terminal",
  long_about = "vellum — a terminal SQL client (browse, query, and safely edit \
databases with a GitHub-like diff). Phase 0: one-shot `--db <FILE> \"<SQL>\"` \
prints a read-only query; add `--interactive` for the scrollable TUI table."
)]
pub struct Cli {
  /// Path to the SQLite database file to open (read-only).
  #[arg(long, value_name = "FILE")]
  pub db: Option<std::path::PathBuf>,

  /// SQL query to run against `--db`. Read-only: only a single `SELECT`-style
  /// statement is accepted; writes are refused (the write gate is a later
  /// phase).
  #[arg(value_name = "SQL")]
  pub sql: Option<String>,

  /// Render the result in the scrollable TUI table (vim navigation, `q` to
  /// quit) instead of printing it to stdout.
  #[arg(short = 'i', long)]
  pub interactive: bool,
}

/// Run the resolved CLI. Returns `Ok(())` on success; the binary maps an `Err`
/// to a non-zero exit in `main` (the one-shot `exit 0 / exit 1` contract).
pub async fn run(cli: Cli) -> Result<()> {
  let interactive = cli.interactive;
  match (cli.db, cli.sql) {
    (Some(db), Some(sql)) => {
      let dsn = sqlite_dsn(&db);
      let driver = SqliteDriver::connect(&dsn).await?;
      let result = driver.query(&sql).await?;
      if interactive {
        tui::run(result)
      } else {
        print_result(&result);
        Ok(())
      }
    }
    (Some(_), None) => Err(VellumError::Arg(
      "a SQL query is required with --db, e.g. `vellum --db data.sqlite \"select * from t\"`".to_string(),
    )),
    (None, Some(_)) => Err(VellumError::Arg("--db <FILE> is required to run a query".to_string())),
    (None, None) => {
      println!(
        "vellum — pass `--db <FILE> \"<SQL>\"` to run a query (add `-i` for the TUI), \
or `--help` for usage."
      );
      Ok(())
    }
  }
}

/// Build the SQLite DSN the driver expects from a filesystem path.
fn sqlite_dsn(path: &Path) -> String {
  format!("sqlite:{}", path.display())
}

/// Print a query result to stdout as tab-separated rows (header first). Stable
/// and scriptable; cell rendering follows `Value`'s `Display`.
fn print_result(result: &QueryResult) {
  let header = result
    .columns
    .iter()
    .map(|c| c.name.as_str())
    .collect::<Vec<_>>()
    .join("\t");
  println!("{header}");
  for row in &result.rows {
    let line = row.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\t");
    println!("{line}");
  }
}
