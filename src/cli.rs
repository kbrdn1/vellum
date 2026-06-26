//! CLI surface (clap). The surface grows phase by phase — Phase 0 adds the
//! one-shot `--db <file> "<sql>"` mode (and its TUI counterpart) against the
//! first `Driver`. For now the binary is an early scaffold: `--help` /
//! `--version` work, and a no-arg invocation prints a short banner. `run` is
//! `async` because the driver layer it will dispatch to is async on the
//! tokio runtime bootstrapped in `main`.

use crate::error::Result;
use clap::Parser;

#[derive(Debug, Parser)]
#[command(
  name = "vellum",
  bin_name = "vellum",
  version,
  about = "TUI SQL client — browse, query, edit databases in the terminal",
  long_about = "vellum — a terminal SQL client (browse, query, and safely edit \
databases with a GitHub-like diff). Early scaffold: no subcommands yet."
)]
pub struct Cli {}

/// Run the resolved CLI. Returns `Ok(())` on success; the binary maps an
/// `Err` to a non-zero exit in `main`. Async to match the driver layer it
/// will dispatch to (Phase 0).
pub async fn run(_cli: Cli) -> Result<()> {
  println!("vellum — early scaffold. No subcommands yet; run `vellum --help`.");
  Ok(())
}
