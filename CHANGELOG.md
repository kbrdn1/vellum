# Changelog

All notable changes to `vellum` are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

> This file is the **in-progress index**. When a release is cut, the
> `## [Unreleased]` entries are moved into a per-version file under
> `changelogs/<version>.md` (stable) or
> `changelogs/pre-releases/<version>.md` (rc/alpha/beta), and a one-line
> pointer is added under **Past releases**. The release workflows source
> their notes from the per-version file, never from this index.

## [Unreleased]

### Added

- **Phase 0 — result table TUI (#6):** render a `QueryResult` as a scrollable
  ratatui table with vim navigation, split TEA-style — a pure
  `tui/state/table.rs` (row cursor + horizontal column scroll, all clamped to
  bounds), an `App` whose `on_key` is the whole input contract (`j`/`k`/`g`/`G`
  move the row cursor, `h`/`l` scroll columns, `q` quits, unknown keys no-op),
  and a thin `tui/view.rs` render (content-fit column widths, cursor highlight,
  no `println!`). Vertical scroll-into-view is delegated to ratatui's stateful
  widget keyed on the cursor. The state machine is unit-tested ratatui-free
  (`tests/tui_app_tests.rs`); the render is smoke-tested through `TestBackend`
  (`tests/tui_view_tests.rs`). The live event loop wires in with the one-shot /
  interactive mode (#7).
- **Phase 0 — SQLite driver (#5):** the `Driver` port sketch (`connect` /
  `query` / `kind`) and its first and only impl, `SqliteDriver` (sqlx, bundled
  libsqlite3, in-process — no system dependency). Maps SQLite's five storage
  classes (NULL / INTEGER / REAL / TEXT / BLOB) onto the normalised `Value`
  and reports the `Backend::Sqlite` tag. The read path is read-only by
  construction: the input is validated with `sqlparser` (one `SELECT`-style
  statement only — DML/DDL, `CREATE TEMP`, and multi-statement payloads are
  refused) and connections open `SQLITE_OPEN_READONLY` as a backstop, so a
  write through `query()` can't run — intentional writes await the gated
  write/diff path. The port stays minimal on purpose; it freezes with the
  richer capabilities/introspection in Phase 1, when Postgres / MySQL become
  the 2nd/3rd impls.
- **Phase 0 — domain model (#4):** the pure, cross-database `Value` enum
  (`Null` / `Bool` / `Int` / `Float` / `Text` / `Bytes` / `Json` / `Timestamp`)
  with a total `Value::kind() → TypeKind` mapping and a canonical `Display`,
  plus the row-oriented `QueryResult { columns, rows, affected }` container
  (`Column`, `Row`). Conservative for SQLite (no I/O); `Decimal` / `Uuid` /
  `Array` and a parsed JSON payload land with Postgres.
- **Phase 0 — binary skeleton (#3):** async entry point on a `tokio` runtime
  (`#[tokio::main]`), a typed `VellumError` surface with `Io` / `Arg` /
  `Driver` categories (`thiserror`), and the unknown-flag exit-code contract
  pinned by an e2e test. The one-shot `--db <file> "<sql>"` argument surface
  and its TUI/one-shot dispatch land with the SQLite driver (#5, #7).
- Initial project scaffold: Cargo `bin` + `lib`, 2-space rustfmt, CI
  (fmt / clippy / test matrix / hook-smoke / audit), release + pre-release
  workflows, Homebrew tap template, dependabot, issue / PR templates, opt-in
  pre-commit hook, Makefile, house rules (CLAUDE.md / AGENTS.md), and a green
  TDD harness (`tests/cli_binary.rs` canary). Pre-Phase-0.

### Changed

- **MSRV raised to 1.88** (was 1.86): ratatui 0.30 — the Phase 0 TUI stack —
  declares rust-version 1.88, so the crate floor follows. The README badge and
  the CONTRIBUTING prerequisite are updated to match; CI enforces it via
  `clippy::incompatible_msrv` against `@stable`. (#6)

## Past releases

_None yet._
