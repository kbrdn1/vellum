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

- **Phase 1 — schema introspection model (#12):** a pure `Catalog → Database →
  Schema → Relation(table|view) → Column` tree plus `ForeignKey` — zero I/O,
  the shape `Driver::introspect()` returns and the sidebar / autocomplete / ERD
  read from. Navigation is by name at each level (`database` / `schema` /
  `relation` / `column`), insertion order is preserved (deterministic for
  diffs/tests), and `Database::resolve` follows a foreign key (same- or
  cross-schema) to its target relation. All four levels are kept even where an
  engine collapses them; the per-engine populator (#13) maps each backend onto
  the tree. Only `Catalog` is re-exported flat; nested types stay under
  `model::catalog::` (a `catalog::Column` would clash with `result::Column`).
- **Phase 1 — connection secrets core (#9):** secrets never live in
  `.vellum.toml`. A `SecretStore` port (set / get / delete a password by
  connection name) with an in-memory `MemoryStore` impl, and a `resolve` rule
  a driver consumes: a `VELLUM_DSN_<NAME>` environment override (frozen
  transform: uppercase, non-alphanumeric → `_`) wins, otherwise the stored
  password, else nothing. A present-but-unreadable override **fails closed**
  (never a silent fall-back to the store), and config load **rejects two
  connection names that collide under the override transform** so a
  `VELLUM_DSN_*` can never mis-route a secret. In-memory secrets are
  `secrecy::SecretString` — zeroized on drop and redacted in `Debug`, guarded
  by a regression test. Env precedence is tested through an injected reader (no
  `set_var` data race). The OS keyring backend and the `vellum connect` command
  that populates it are scoped to follow-up #72 (built with their consumer,
  verifiable against a real keychain — not provisionable in CI).
- **Phase 1 — `.vellum.toml` connection manager (#8):** parse the config file
  into a typed `Config` — `[connections.<name>]` (backend, host, port, user,
  database, path, sslmode) plus a `[ui]` block (`page_size` default 200,
  `theme` default "vellum"). The `backend` field resolves to the canonical
  `Backend` tag, now extended with `Postgres` / `MySql` (a variant names a
  *valid backend*, not a wired driver). The schema we freeze for 1.0 is
  deliberately strict: `deny_unknown_fields` turns a typo'd key into a hard
  error, and a plaintext `password` is **refused on presence** with a message
  pointing at the system keyring / `VELLUM_DSN_*` (secrets never live in the
  file — keyring + env resolution land in #9). Pure parse, no I/O;
  `tests/config_tests.rs` pins multi-connection parsing, `[ui]` defaults, the
  closed backend set, unknown-key rejection, and the password gate.
- **Phase 0 — one-shot CLI + TUI launch (#7):** `vellum --db <FILE> "<SQL>"`
  connects the SQLite driver, runs the read-only query, and prints the rows to
  stdout as tab-separated values (header first) — exit `0` on success, exit `1`
  on a query/driver error (invalid SQL or a refused write). Add `-i` /
  `--interactive` to render the same result in the scrollable TUI table (vim
  navigation, arrow-key aliases, `q`/`Esc` to quit) via a thin crossterm event
  loop (`tui/runtime.rs`). Shared e2e fixtures land in `tests/common/mod.rs` (a
  seeded SQLite tempfile), and `tests/cli_binary.rs` pins the one-shot
  contract: rows + exit 0, invalid SQL → exit 1, refused write → exit 1, `--db`
  without a query → usage error. The `help` canary now tracks the `--db` /
  `--interactive` surface.
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
