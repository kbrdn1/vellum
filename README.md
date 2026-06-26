# vellum — TUI SQL client

[![ci](https://github.com/kbrdn1/vellum/actions/workflows/ci.yml/badge.svg)](https://github.com/kbrdn1/vellum/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE.md)
[![rust](https://img.shields.io/badge/rust-1.86%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![status](https://img.shields.io/badge/status-pre--0.1%20%C2%B7%20WIP-yellow)](#status)

Browse, query, and **safely edit** databases from the terminal — with a
**GitHub-like diff** for every write. Rust + `ratatui`, single static binary,
instant start. **MIT · local-first · zero subscription, zero account, zero
telemetry.**

> ⚠️ **Early WIP.** The project scaffold is in place; **Phase 0** (first
> `Driver` + query + table render) is next. **Not yet usable** — the binary
> only answers `--help` / `--version` today. Roadmap and architecture are
> tracked privately and shipped phase by phase.

## why vellum

Paid desktop clients ([DB Pro](https://www.dbpro.app/),
[TablePlus](https://tableplus.com/)) are powerful but heavy, GUI-only, and
behind a licence. Open-source terminal clients are lighter but thin on pro
features, or slow to start. `vellum` takes the opposite bet: **the fastest,
most ergonomic SQL client in the terminal** — fully open-source, everything
local.

| Tool                  | Tech            | Form    | Trade-off to exploit                     |
|:----------------------|:----------------|:--------|:-----------------------------------------|
| LazySQL               | Rust / ratatui  | TUI     | few DBs, thin on pro features            |
| Harlequin             | Python / Textual| TUI     | heavy, slow start, not single-binary     |
| pgcli / mycli         | Python          | REPL    | mono-DB, no visual browse                |
| DB Pro / TablePlus    | GUI             | Desktop | paid, not in the terminal                |
| **vellum**            | **Rust / ratatui** | **TUI** | **early WIP — single binary, OSS, free** |

> ratatui = TUI only. A GUI variant would be a **separate** project, out of scope here.

## what vellum will do

The goal (tracked privately, shipped one phase at a time — **none of this is
usable yet**):

- **Multi-DB browse** — PostgreSQL / MySQL / SQLite first; DuckDB / ClickHouse
  / MSSQL later, behind opt-in Cargo features so the base binary stays light.
- **SQL editor** — schema-aware autocomplete, syntax highlight, history, saved
  queries, `EXPLAIN`, multi-result tabs, a `:` command palette.
- **Safe writes with a GitHub-like diff** — every data / DDL change is staged
  into a changeset, rendered as a diff, dry-run, then applied **in a
  transaction** — never auto-committed. Parametrised SQL (anti-injection),
  PK-targeted edits, a **prod safe-mode** guard.
- **Export / import** — CSV / JSON / Parquet, scriptable from the CLI **and**
  driven from the TUI.
- **Terminal-native ergonomics** — instant start, low RAM, lazygit / vim
  muscle memory, role-based themes, a remappable keymap, PTY drop into
  `psql` / `usql`.
- **Machine contract** — a frozen `--format=json` surface + a JSON-RPC daemon,
  so editors, statuslines, and an external AI assistant integrate without
  re-shelling per query.

## install

Until the first tagged release, build from source:

```bash
git clone https://github.com/kbrdn1/vellum.git
cd vellum
cargo build --release      # → target/release/vellum
cargo run -- --help
```

| Channel          | Command                                              |
|:-----------------|:-----------------------------------------------------|
| Cargo (source)   | `cargo install --path .`                             |
| cargo-binstall   | `cargo binstall vellum` *(with the first release)*   |
| Homebrew (macOS) | `brew tap kbrdn1/tap && brew install vellum` *(soon)*|
| Prebuilt         | [Releases](https://github.com/kbrdn1/vellum/releases) *(with the first tag)* |

## status

Pre-`0.1`, built in the open with the same engineering discipline as
[`gwm`](https://github.com/kbrdn1/gwm-cli):

- **TDD is mandatory** — red → green → refactor; no production code without a
  failing test that pinned the behaviour first.
- **Contracts frozen at `1.0`** (config + `--format=json`) — SemVer once stable.
- **The write path is sacred** — no auto-commit, parametrised SQL, dry-run,
  prod safe-mode (see [CLAUDE.md](CLAUDE.md)).
- Every commit is `cargo fmt` + `cargo clippy -D warnings` clean; CI runs the
  suite on Linux / macOS / Windows.

## contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for conventions (branches, Gitmoji +
Conventional Commits, the red → green → refactor loop) and
[CLAUDE.md](CLAUDE.md) / [AGENTS.md](AGENTS.md) for the house rules AI
assistants must follow here.

## license

MIT — see [LICENSE.md](LICENSE.md). © 2026 Kylian Bardini.

## related docs

- [`CHANGELOG.md`](CHANGELOG.md) — release index (root = `[Unreleased]`; per-version archives under [`changelogs/`](changelogs/))
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — branch / commit / PR conventions
- [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md)
- [`.github/LABELS.md`](.github/LABELS.md)
