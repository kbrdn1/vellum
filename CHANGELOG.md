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

- **Phase 1 — frozen `Driver` port + Postgres introspection (#11):** with three
  real impls justifying it, the port is frozen to `connect` / `query` /
  `introspect` / `backend` / `capabilities` — the contract the TUI codes against
  (`Box<dyn Driver>`, guarded by an object-safety test). `introspect()` moves
  onto the trait (all three back it), and **Postgres introspection** lands here
  (deferred from #10): `information_schema` + `pg_catalog`, **multi-schema** (a
  `Database` with every user schema), composite-safe foreign keys via
  `unnest(conkey, confkey) WITH ORDINALITY`. A new `Capabilities` record gates
  UI per backend — `explain` / `schemas` / `foreign_keys`: Postgres has real
  schemas (`schemas: true`), SQLite and MySQL collapse database and schema
  (`false`). The write path (`execute`) and streaming (`query_stream`) are
  deliberately **not** frozen onto the port — `execute`'s shape depends on the
  changeset model (#64) and streaming has no Phase-1 consumer; freezing either
  as a stub would be speculative (YAGNI). `Driver::kind()` is renamed
  `backend()`.
- **Phase 1 — MySQL driver (#11):** the third `Driver` impl, `MySqlDriver`
  (sqlx, rustls). Read-only by construction in two layers: the shared parser
  guard — now hardened to reject `SELECT … INTO OUTFILE`/`DUMPFILE`, a *file*
  write a read-only transaction does NOT stop (MySQL's guard-passing write
  vector, the analogue of Postgres's data-modifying CTE) — and a session
  `transaction_read_only = ON` set on every connection, so each autocommit
  statement runs as a READ ONLY transaction and a write (incl. a writing
  function called via `SELECT`) errors. Unlike Postgres's session option this is
  not bypassable: MySQL has no `set_config`-style function to flip it from a
  `SELECT`, and the guard refuses a bare `SET` (the PG `BEGIN` + `SET
  TRANSACTION READ ONLY` pattern does not port — MySQL errors 1568 — and `START
  TRANSACTION` is rejected by the prepared protocol, 1295). Conservative type
  mapping: int family / float·double / text family / blob family decode to their
  `Value`; `json` → `Json`; `datetime`·`timestamp`·`date`·`time` → `Timestamp`;
  the long tail (decimal, unsigned 64-bit, bit, enum/set, geometry) → an honest
  `<typename>` marker (#76). Introspection reads `information_schema` for the
  current database (`CONVERT(_ USING utf8mb4)` around its binary-collation
  columns) into the `Catalog`; MySQL's database = schema collapses to one
  `Database`/`Schema`. Integration tests run behind `it-db` against a MySQL
  service in CI (default `cargo test` stays SQLite-only).
- **Phase 1 — PostgreSQL driver (#10):** the second `Driver` impl, `PostgresDriver`
  (sqlx, rustls — `sslmode` honoured from the DSN, no OpenSSL system dep). Read-only
  by construction with **two layers**: the shared single-`SELECT` parser guard
  (now `driver::ensure_single_read_query`, parametrised by the engine dialect) and
  an explicit transaction-level `READ ONLY` around every query. The latter is the
  load-bearing boundary for PG: the parser guard passes a data-modifying CTE
  (`WITH t AS (INSERT … RETURNING *) SELECT * FROM t`, which writes), and a SELECT
  can flip the session read-only default (`set_config`) that a reused pooled
  connection inherits — but a transaction-level `READ ONLY` can't be undone by a
  single statement (the session default is kept only as defence in depth; the
  driver uses a single connection). Type mapping is conservative: bool / int2·4·8 / float4·8 / text
  family / bytea decode to their `Value`; `json`·`jsonb` → `Json`; `uuid` → `Text`;
  `timestamptz`·`timestamp`·`date`·`time` → `Timestamp`. The long tail (numeric,
  arrays, enums, …) maps to an honest non-data marker `<typename>` — never a faked
  value — with faithful decode tracked by #76. Integration tests live behind a new
  `it-db` Cargo feature (Postgres service in CI; default `cargo test` stays on
  in-memory SQLite, no Docker), seeded through a separate writable pool.
- **Phase 1 — SQLite introspection (#13):** `SqliteDriver::introspect()`
  populates the pure `Catalog` (#12) from a live SQLite database — reading
  `sqlite_master` (tables + views, internal `sqlite_*` excluded) and the
  `pragma_table_xinfo` / `pragma_foreign_key_list` table-valued functions
  (bound parameters), all on a single read transaction for a consistent
  snapshot. Columns keep ordinal order, the declared type verbatim, faithful
  nullability (`notnull == 0`, never "PK implies not-null" — a SQLite
  `PRIMARY KEY` can admit NULL), and the PK flag; generated columns are
  included. Multi-column foreign keys fold by id, and an implicit FK target
  (`references parent` with no columns) resolves to the parent's primary key.
  Internal tables are excluded by the literal `sqlite_` prefix (`GLOB`, not a
  `LIKE` whose `_` is a wildcard).
  SQLite's single schema maps to one `main` database / `main` schema. Tested
  against an in-process seeded DB (no external service). Postgres / MySQL
  introspection lands with their drivers (#10/#11), behind `it-db`.
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
