//! CLI surface (clap). Phase 0 ships the one-shot mode: `vellum --db <FILE>
//! "<SQL>"` connects the SQLite `Driver`, runs the (read-only) query, and
//! prints the rows to stdout — exit 0 on success, exit 1 on a query/driver
//! error. `--interactive` renders the same result in the scrollable TUI table
//! instead (vim navigation, `q` to quit). `run` is `async` because the driver
//! layer it dispatches to is async on the tokio runtime bootstrapped in `main`.

use std::io::IsTerminal;

use clap::Parser;

use crate::driver::{Driver, SqliteDriver};
use crate::error::{Result, VellumError};
use crate::model::QueryResult;
use crate::tui;
use crate::tui::app::App;

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
      // Open by path (not a DSN string) so the literal file named is queried —
      // see `SqliteDriver::open_readonly`.
      // For `-i`, refuse a non-terminal before doing any work — no point opening
      // the database or running the query if the result can't be rendered.
      if interactive {
        require_terminal()?;
      }
      let driver = SqliteDriver::open_readonly(&db).await?;
      let result = driver.query(&sql).await?;
      if interactive {
        tui::run(result)
      } else {
        print_result(&result)
      }
    }
    // `--db` with no query opens the interactive browse UI: introspect the
    // schema, then navigate it and page through tables live (read-only).
    (Some(db), None) => {
      require_terminal()?;
      let driver = SqliteDriver::open_readonly(&db).await?;
      let catalog = driver.introspect().await?;
      let app = App::browse(catalog, driver.capabilities(), driver.backend());
      tui::browse(driver, app).await
    }
    (None, Some(_)) => Err(VellumError::Arg("--db <FILE> is required to run a query".to_string())),
    // `--interactive` without a database/query is a usage error, not a silent
    // banner: the user asked to open something that isn't there.
    (None, None) if interactive => Err(VellumError::Arg(
      "--interactive needs --db <FILE> and a query, e.g. `vellum --db data.sqlite \"select * from t\" -i`".to_string(),
    )),
    (None, None) => {
      println!(
        "vellum — pass `--db <FILE> \"<SQL>\"` to run a query (add `-i` for the TUI), \
`--db <FILE>` alone to browse the schema, or `--help` for usage."
      );
      Ok(())
    }
  }
}

/// Refuse to launch a full-screen TUI when stdout is not a terminal (piped,
/// redirected, or a CI runner). `ratatui::try_init` does not fail fast on every
/// platform — on Windows under a pipe it can leave `event::read` blocking
/// forever — so we gate up front and fail cleanly instead of hanging.
fn require_terminal() -> Result<()> {
  if std::io::stdout().is_terminal() {
    Ok(())
  } else {
    Err(VellumError::Arg(
      "this view needs an interactive terminal; pipe a query instead, e.g. \
`vellum --db data.sqlite \"select * from t\"`"
        .to_string(),
    ))
  }
}

/// Print a query result to stdout as tab-separated rows (header first). Tabs,
/// newlines, and backslashes inside a cell are escaped so the row/column
/// structure survives arbitrary TEXT values; cells otherwise render via
/// `Value`'s `Display`. Writes through a locked handle and treats a closed
/// stdout (e.g. piped into `head`) as a clean exit rather than a panic.
fn print_result(result: &QueryResult) -> Result<()> {
  use std::io::Write as _;

  let mut out = std::io::stdout().lock();
  let header = result
    .columns
    .iter()
    .map(|c| escape_cell(&c.name))
    .collect::<Vec<_>>()
    .join("\t");
  if let Err(e) = writeln!(out, "{header}") {
    return swallow_broken_pipe(e);
  }
  for row in &result.rows {
    let line = row
      .iter()
      .map(|v| escape_cell(&v.to_string()))
      .collect::<Vec<_>>()
      .join("\t");
    if let Err(e) = writeln!(out, "{line}") {
      return swallow_broken_pipe(e);
    }
  }
  if let Err(e) = out.flush() {
    return swallow_broken_pipe(e);
  }
  Ok(())
}

/// Escape the TSV-structural characters in a cell so a tab or newline in TEXT
/// data can't break the column/row layout. Reversible: `\` → `\\` first, then
/// tab / newline / carriage return.
fn escape_cell(s: &str) -> String {
  s.replace('\\', "\\\\")
    .replace('\t', "\\t")
    .replace('\n', "\\n")
    .replace('\r', "\\r")
}

/// A closed stdout (broken pipe, e.g. piped into `head`) is a normal end, not a
/// failure: return `Ok` instead of letting the write panic (exit 101). Any other
/// I/O error propagates as `VellumError::Io`.
fn swallow_broken_pipe(e: std::io::Error) -> Result<()> {
  if e.kind() == std::io::ErrorKind::BrokenPipe {
    Ok(())
  } else {
    Err(e.into())
  }
}
