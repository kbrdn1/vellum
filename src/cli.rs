//! CLI surface (clap). Phase 0 ships the one-shot mode: `vellum --db <FILE>
//! "<SQL>"` connects the SQLite `Driver`, runs the (read-only) query, and
//! prints the rows to stdout — exit 0 on success, exit 1 on a query/driver
//! error. `--interactive` renders the same result in the scrollable TUI table
//! instead (vim navigation, `q` to quit). `run` is `async` because the driver
//! layer it dispatches to is async on the tokio runtime bootstrapped in `main`.

use std::io::IsTerminal;

use clap::{Parser, Subcommand};

use crate::driver::{Driver, SqliteDriver};
use crate::error::{Result, VellumError};
use crate::keyring_store::{store_secret, KeyringStore};
use crate::model::QueryResult;
use crate::secrets::{SecretStore, SecretString};
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
  /// Subcommands (e.g. `connect`). When omitted, the default one-shot / browse
  /// surface (`--db`, `[SQL]`, `--interactive`) applies.
  #[command(subcommand)]
  pub command: Option<Command>,

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

/// vellum subcommands. Phase 1 adds `connect`; the default (no subcommand)
/// surface stays the flag-based one-shot / browse mode.
#[derive(Debug, Subcommand)]
pub enum Command {
  /// Store a connection's password in the OS keyring (prompted, no echo).
  ///
  /// Reads the password interactively and stores it under `vellum:<name>` so a
  /// secret never lives in `.vellum.toml`. The `<name>` is stored as given —
  /// validating it against a `.vellum.toml` entry lands with the connection
  /// wiring that reads the config.
  Connect {
    /// The connection name to store a password for (the `[connections.<name>]`
    /// key it will resolve against).
    #[arg(value_name = "NAME")]
    name: String,
  },
}

/// Run the resolved CLI. Returns `Ok(())` on success; the binary maps an `Err`
/// to a non-zero exit in `main` (the one-shot `exit 0 / exit 1` contract).
pub async fn run(cli: Cli) -> Result<()> {
  // A subcommand takes the whole run; the default surface applies only when
  // none is given.
  if let Some(command) = cli.command {
    return run_command(command);
  }
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
      tui::browse(Box::new(driver), app).await
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

/// Dispatch a subcommand. Kept sync — `connect` prompts and hits the OS
/// keyring, no async work — and called before any async path in [`run`].
fn run_command(command: Command) -> Result<()> {
  match command {
    Command::Connect { name } => connect(&name),
  }
}

/// `vellum connect <name>`: read a password with no terminal echo and store it
/// in the OS keyring under the connection name. Thin wrapper — it injects the
/// two untestable edges (the `rpassword` prompt and the real [`KeyringStore`])
/// into [`connect_with`], then prints the confirmation.
fn connect(name: &str) -> Result<()> {
  connect_with(&KeyringStore::new(), name, || {
    rpassword::prompt_password(format!("Password for `{name}`: "))
      .map_err(|e| VellumError::Secret(format!("could not read the password: {e}")))
  })?;
  println!("stored the password for `{name}` in the system keyring");
  Ok(())
}

/// The core of `vellum connect`, with the password source and the secret store
/// injected so the whole path — read the password, wrap it in a
/// [`SecretString`], store it under `name` — is exercisable without a tty or a
/// real keychain. `read_password` yields the plaintext (an aborted / failed
/// prompt is an `Err`, and nothing is stored); the real command passes
/// `rpassword` + a [`KeyringStore`], tests pass a fake reader + `MemoryStore`.
pub fn connect_with(store: &dyn SecretStore, name: &str, read_password: impl FnOnce() -> Result<String>) -> Result<()> {
  let password = read_password()?;
  store_secret(store, name, &SecretString::from(password))
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
