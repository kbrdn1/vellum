# vellum — roadmap

This document tracks where `vellum` is heading. It complements
[CHANGELOG.md](CHANGELOG.md) (what already shipped) and the
[open issues](https://github.com/kbrdn1/vellum/issues) (the source of truth for
scope details).

Each item below links to its GitHub issue. The scope, acceptance criteria, and
the TDD test plan live there — this file is the map, not the spec. It is updated
to reflect what ships in each release.

## Current state — pre-v0.1.0 (scaffold)

Nothing has shipped yet. The repository carries the project scaffold (README,
CI, release workflows, issue/PR templates, house rules) and this plan. The first
publishable release, **v0.1.0**, lands at the end of **Phase 1** — a TUI SQL
client at LazySQL parity on PostgreSQL, MySQL and SQLite.

**What vellum is:** a single-binary TUI SQL client — browse, query and edit
databases in the terminal, with a **GitHub-like diff for every write**. Rust +
ratatui, MIT, local-first. No subscription, no account, no SaaS, no telemetry.

**The aim:** *the fastest, most ergonomic, safest SQL client in the terminal* —
the useful subset of DB Pro / TablePlus, in the open. We will not match a GUI
feature-for-feature; we win on instant startup, low RAM, lazygit + vim
ergonomics, multi-DB breadth, and engineering discipline.

**The moat:** the write path. Every data or schema mutation is staged into a
changeset, rendered as a git-style diff with the exact generated SQL, and
applied only on explicit confirmation — inside a transaction, with a dry-run
available. Parametrised SQL only, PK-targeted, read-only-safe for prod. See
[The write path](#the-write-path--safe-by-construction) below.

## Milestones

| Phase | Milestone | Exit gate | Target |
|:--|:--|:--|:--|
| 0 | [Fondations & spike](https://github.com/kbrdn1/vellum/milestone/1) | Open a SQLite DB, show 10 navigable rows, quit cleanly | pre-0.1.0 |
| 1 | [MVP browse](https://github.com/kbrdn1/vellum/milestone/2) | Connect via `.vellum.toml`, schema sidebar, paginated browse, raw SQL, CSV/JSON export+import — on PG+MySQL+SQLite | **v0.1.0** |
| 2 | [Workflow requête pro](https://github.com/kbrdn1/vellum/milestone/3) | Highlight + schema-aware autocomplete, persistent history, snippets, tabs, EXPLAIN, explicit transactions | v0.2.x–0.3.x |
| 3 | [Édition sûre](https://github.com/kbrdn1/vellum/milestone/4) | Edit a cell/row/structure → see generated SQL + diff → transactional apply with dry-run; no write without confirmation | v0.4.0 |
| 4 | [Pro tier](https://github.com/kbrdn1/vellum/milestone/5) | ERD, exotic drivers (DuckDB/ClickHouse/MSSQL), SSH tunnel, Parquet, schema-diff→migration, opt-in AI | v0.5.0+ |
| 5 | [Écosystème & contrat machine](https://github.com/kbrdn1/vellum/milestone/6) | JSON-RPC daemon, frozen `--format=json`, PTY overlay, presets, statusline, doctor | continuous → 1.0 |

> **Golden rule:** ship Phase 1 fast and publish it. Validate daily use before
> investing in Phase 4. Phases 0–3 are sequential; Phase 4 items are independent
> and prioritised by real usage; Phase 5 is continuous, post-1.0.

The `⛓` marker shows the issues a task is **blocked by** (dependency order).

## Next up — Phase 0 · Foundations & spike

> Stand up the binary + the `Driver` trait on a single engine (SQLite), with the
> gwm-grade TDD harness. No product feature — just the
> connection → query → table-render loop and a testable architecture.

| Issue | Surface | Scope |
|:--|:--|:--|
| [#3](https://github.com/kbrdn1/vellum/issues/3) | `build` `cli` | bootstrap binary skeleton — clap, tokio, typed VellumError |
| [#4](https://github.com/kbrdn1/vellum/issues/4) | `driver` | QueryResult + normalised cross-DB Value enum ⛓ #3 |
| [#5](https://github.com/kbrdn1/vellum/issues/5) | `driver` | Driver trait sketch + SQLite impl (query → QueryResult) ⛓ #4 |
| [#6](https://github.com/kbrdn1/vellum/issues/6) | `tui` | scrollable result table + vim nav (j/k/g/G, horizontal scroll) ⛓ #5 |
| [#7](https://github.com/kbrdn1/vellum/issues/7) | `cli` `test` | one-shot mode `vellum --db <f> "<sql>"` + e2e TDD harness ⛓ #5 |

## Phase 1 — MVP browse (= LazySQL parity) → v0.1.0

> A daily-driver TUI client to read data and run queries, multi-DB. The `Driver`
> trait freezes here (3 real impls). First publishable, dogfoodable release.

| Issue | Surface | Scope |
|:--|:--|:--|
| [#8](https://github.com/kbrdn1/vellum/issues/8) | `config` | `.vellum.toml` connection manager ([connections.*]) ⛓ #3 |
| [#9](https://github.com/kbrdn1/vellum/issues/9) | `config` `security` | keyring secrets — never plaintext passwords ⛓ #8 |
| [#10](https://github.com/kbrdn1/vellum/issues/10) | `driver` | PostgreSQL driver (sqlx) ⛓ #5 |
| [#11](https://github.com/kbrdn1/vellum/issues/11) | `driver` | MySQL driver + freeze the Driver trait (3 impls) ⛓ #10 |
| [#12](https://github.com/kbrdn1/vellum/issues/12) | `driver` `query` | schema introspection model (Database→Schema→Relation→Column) ⛓ #11 |
| [#13](https://github.com/kbrdn1/vellum/issues/13) | `driver` | per-engine introspection (information_schema / pragma) ⛓ #12 |
| [#14](https://github.com/kbrdn1/vellum/issues/14) | `tui` | schema sidebar tree ⛓ #13 |
| [#15](https://github.com/kbrdn1/vellum/issues/15) | `tui` `query` | paginated data browse + virtualised table ⛓ #14 |
| [#16](https://github.com/kbrdn1/vellum/issues/16) | `tui` `query` | multiline SQL editor + run (Ctrl-Enter) ⛓ #15 |
| [#17](https://github.com/kbrdn1/vellum/issues/17) | `export` | CSV/JSON export (CLI + TUI) ⛓ #11 |
| [#18](https://github.com/kbrdn1/vellum/issues/18) | `export` | CSV/JSON import (column mapping, PK upsert) ⛓ #17 |
| [#19](https://github.com/kbrdn1/vellum/issues/19) | `tui` `query` | column sort on browse ⛓ #15 |
| [#20](https://github.com/kbrdn1/vellum/issues/20) | `config` `security` | connection safe mode (prod colour + read_only flag) ⛓ #8 |

## Phase 2 — Workflow requête pro (= beyond LazySQL) → v0.2.x–0.3.x

> Turn the raw editor into a real query desk: this is where vellum overtakes
> LazySQL and approaches a pro client. `complete.rs` / `plan.rs` are pure → fully
> testable without ratatui or a DB.

| Issue | Surface | Scope |
|:--|:--|:--|
| [#21](https://github.com/kbrdn1/vellum/issues/21) | `query` `tui` | SQL syntax highlight (sqlparser tokenizer) ⛓ #16 |
| [#22](https://github.com/kbrdn1/vellum/issues/22) | `query` | schema-aware autocomplete ⛓ #21 |
| [#23](https://github.com/kbrdn1/vellum/issues/23) | `query` | persistent query history (local SQLite, fuzzy search, replay) ⛓ #16 |
| [#24](https://github.com/kbrdn1/vellum/issues/24) | `query` | saved queries / snippets ⛓ #16 |
| [#25](https://github.com/kbrdn1/vellum/issues/25) | `tui` | multi-result tabs ⛓ #15 |
| [#26](https://github.com/kbrdn1/vellum/issues/26) | `query` | EXPLAIN plan viewer (parse → indented tree) ⛓ #16 |
| [#27](https://github.com/kbrdn1/vellum/issues/27) | `query` `security` | explicit transaction mode (BEGIN/COMMIT/ROLLBACK + status) ⛓ #16 |
| [#28](https://github.com/kbrdn1/vellum/issues/28) | `query` `tui` | advanced filter builder (→ WHERE) ⛓ #15 |
| [#29](https://github.com/kbrdn1/vellum/issues/29) | `tui` | fuzzy command palette (open anything) ⛓ #14 |
| [#30](https://github.com/kbrdn1/vellum/issues/30) | `tui` | row inspector (detail + relations) ⛓ #15 |
| [#31](https://github.com/kbrdn1/vellum/issues/31) | `tui` `query` | follow foreign key (jump to referenced row) ⛓ #30 |
| [#32](https://github.com/kbrdn1/vellum/issues/32) | `query` | SQL reformatter / beautifier ⛓ #21 |
| [#33](https://github.com/kbrdn1/vellum/issues/33) | `query` `security` | destructive-query warnings (DROP/TRUNCATE/DELETE-UPDATE without WHERE) ⛓ #21 |
| [#34](https://github.com/kbrdn1/vellum/issues/34) | `tui` | themes / dark mode (role-based) ⛓ #6 |

## Phase 3 — Édition sûre (the diff gate) → v0.4.0

> From read-only to editing — **without ever surprising the user**. Safety
> outranks brevity here. `changeset.rs` is the critical module: edit intents →
> parametrised, PK-targeted SQL, proven injection-safe by test.

| Issue | Surface | Scope |
|:--|:--|:--|
| [#35](https://github.com/kbrdn1/vellum/issues/35) | `write` `security` | primary-key / uniqueness detection (no PK → read-only) ⛓ #13 |
| [#36](https://github.com/kbrdn1/vellum/issues/36) | `write` `security` | changeset model → parametrised SQL (anti-injection core) ⛓ #35 |
| [#37](https://github.com/kbrdn1/vellum/issues/37) | `write` `tui` | data diff view (unified / side-by-side) ⛓ #36 |
| [#38](https://github.com/kbrdn1/vellum/issues/38) | `write` `security` | transactional apply + dry-run ⛓ #36 |
| [#39](https://github.com/kbrdn1/vellum/issues/39) | `write` `tui` | inline cell editing (typed buffer) ⛓ #37, #38 |
| [#40](https://github.com/kbrdn1/vellum/issues/40) | `write` `tui` | row insert / delete (PK-targeted) ⛓ #39 |
| [#41](https://github.com/kbrdn1/vellum/issues/41) | `write` `security` `tui` | confirm gate + safe-mode countdown ⛓ #38, #20 |
| [#42](https://github.com/kbrdn1/vellum/issues/42) | `write` | DDL diff + schema edit (same diff engine) ⛓ #37 |
| [#43](https://github.com/kbrdn1/vellum/issues/43) | `write` | table ops (create/duplicate/truncate/drop) + indexes/constraints/FK ⛓ #42 |

## Phase 4 — Pro tier / differentiation → v0.5.0+

> The real "DB-Pro-tier" arguments — but with no subscription, no hosted service,
> no telemetry. Also vellum's analytics dimension (DuckDB/ClickHouse + Parquet).
> Every item is independent and shippable on its own; attack only after Phases 1–2
> are validated by real use (otherwise scope creep). Likely order: DuckDB + ERD
> before AI.

| Issue | Surface | Scope |
|:--|:--|:--|
| [#44](https://github.com/kbrdn1/vellum/issues/44) | `query` | ERD diagram (terminal layout + mermaid/dot export) ⛓ #13 |
| [#45](https://github.com/kbrdn1/vellum/issues/45) | `driver` | DuckDB driver (analytics, feature-gated) ⛓ #11 |
| [#46](https://github.com/kbrdn1/vellum/issues/46) | `driver` | ClickHouse driver (feature-gated) ⛓ #11 |
| [#47](https://github.com/kbrdn1/vellum/issues/47) | `driver` | MSSQL driver (tiberius, feature-gated) ⛓ #11 |
| [#48](https://github.com/kbrdn1/vellum/issues/48) | `driver` `config` | SSH tunnel (russh, no external ssh) ⛓ #20 |
| [#49](https://github.com/kbrdn1/vellum/issues/49) | `export` | Parquet / Arrow export (feature-gated) ⛓ #17 |
| [#50](https://github.com/kbrdn1/vellum/issues/50) | `write` | schema compare (2 connections) → migration .sql ⛓ #42 |
| [#51](https://github.com/kbrdn1/vellum/issues/51) | `export` | SQL dump export/import (DB migration) ⛓ #18 |
| [#52](https://github.com/kbrdn1/vellum/issues/52) | `query` `docs` | NL→SQL via external machine contract (skill/CLI) ⛓ #38 |
| [#53](https://github.com/kbrdn1/vellum/issues/53) | `query` | optional in-app AI (feature `ai`, BYO-key/Ollama, diff-gated) ⛓ #52 |
| [#54](https://github.com/kbrdn1/vellum/issues/54) | `tui` | reduced charts/dashboards (BarChart/Sparkline) ⛓ #15 |

## Phase 5 — Écosystème & machine contract (continuous → 1.0)

> Make vellum a composable brick (like gwm): integrable into the editor, the
> statusline, scripts — not just an interactive app. Most of this is ported from
> gwm, adapted to the SQL domain (connections ≠ worktrees). `--format=json` is
> marked *experimental* until the data model stabilises, then frozen at 1.0.

| Issue | Surface | Scope |
|:--|:--|:--|
| [#55](https://github.com/kbrdn1/vellum/issues/55) | `cli` | machine contract `--format=json` + SCHEMA_VERSION (frozen) ⛓ #17 |
| [#56](https://github.com/kbrdn1/vellum/issues/56) | `cli` | JSON-RPC daemon (unix socket) ⛓ #55 |
| [#57](https://github.com/kbrdn1/vellum/issues/57) | `cli` | `vellum statusline` (daemon consumer) ⛓ #56 |
| [#58](https://github.com/kbrdn1/vellum/issues/58) | `tui` | PTY overlay → drop into psql/usql/mysql ⛓ #16 |
| [#59](https://github.com/kbrdn1/vellum/issues/59) | `cli` | `vellum doctor` (config / connection checks) ⛓ #8 |
| [#60](https://github.com/kbrdn1/vellum/issues/60) | `config` `cli` | `.vellum.toml` presets (`vellum init --preset`) ⛓ #59 |

## The write path — safe by construction

The single biggest correctness/safety surface, anchored in Phase 3 and extended
in Phase 4. Every mutation (data **or** structure) follows one pipeline:

```
intention(s) → changeset (local staging) → git-like DIFF → SQL review
             → dry-run (optional) → transactional apply → rollup
```

Non-negotiable guard-rails, each proven by test:

1. **Never auto-commit.** Diff + explicit confirmation are mandatory.
2. **Parametrised SQL only** — values are bound, never interpolated (anti-injection).
3. **PK-targeted** — no detectable identity → editing refused (no `WHERE *=*`).
4. **Atomicity** — partial failure → full `ROLLBACK`.
5. **Dry-run** — execute then always rollback, to validate without persisting.
6. **Prod safe-mode** — a `production` connection demands a reinforced confirm
   (countdown) and can be `read_only` (writes blocked upstream of the diff).
7. **Destructive warnings** — `DROP` / `TRUNCATE` / unguarded `DELETE`·`UPDATE`
   flagged red before apply.
8. **AI never bypasses the gate** — any AI-generated write flows through the same
   diff + confirmation.

Issues: [#35](https://github.com/kbrdn1/vellum/issues/35)
[#36](https://github.com/kbrdn1/vellum/issues/36)
[#37](https://github.com/kbrdn1/vellum/issues/37)
[#38](https://github.com/kbrdn1/vellum/issues/38)
[#41](https://github.com/kbrdn1/vellum/issues/41)
[#42](https://github.com/kbrdn1/vellum/issues/42)
[#50](https://github.com/kbrdn1/vellum/issues/50) ·
data + schema diff (Phase 3), schema-compare→migration (Phase 4).

## How to contribute

1. Pick an item that interests you and read its issue for scope + the TDD plan.
2. Comment on the issue if you intend to work on it (avoids duplication).
3. `gwm create <type> <issue> <slug>` to spin up an isolated worktree on a
   `<type>/#<issue>-<slug>` branch (this repo carries a `.gwm.toml`).
4. **TDD is a hard merge requirement** — a failing test pins the behaviour
   first. Each issue names its test file from the taxonomy
   (`driver_tests.rs`, `tui_app_tests.rs`, `write_tests.rs`, `config_tests.rs`,
   `cli_binary.rs`, or a pure unit test).
5. Open a PR targeting `dev`, filled from the PR template, following
   [CONTRIBUTING.md](CONTRIBUTING.md) (Gitmoji + Conventional Commits, tests
   required, regular merge — never squash, never delete the source branch).

The issue is the source of truth — this roadmap is updated to reflect what ships
in each release.

## Out of scope (for now)

Consequences of the **TUI + OSS + local-first** choice, not gaps:

- **Web workspaces / team collaboration** — implies account + cloud, contradicts
  local-first. OSS substitute: share `.vellum.toml` + snippets via git.
- **Visual workflow builder / drag-drop ERD designer** — GUI paradigm. Substitute:
  DDL edit + diff (Phase 3) and the `--format=json` contract → scripts.
- **Hosted / subscription AI** — never a vellum endpoint; AI is opt-in behind the
  `ai` feature, BYO-key or local Ollama.
- **Cross-machine worktree/connection sync, shared dashboards** — too much surface
  (state, conflict, networking) against a local-responsiveness tool.
- **NoSQL (Redis / MongoDB)** — the `Driver` trait is SQL-centric; key-value /
  document is a separate abstraction, a post-1.0 chantier to isolate.
- **Multi-cursor editor** — high TUI cost, low value.
- **GUI front-end** (egui/iced/tauri) — a separate track, outside this roadmap.

That can change if a concrete use case shows up — open a feature-request issue
with the rationale.

## Shipped highlights

Nothing shipped yet. As releases land, each version's highlights move here (per
the gwm model — one row per closing PR), and the consolidated notes live in
`changelogs/<version>.md`. The first row arrives with **v0.1.0** (end of Phase 1).

| Issue | Shipped in | Feature |
|:--|:--|:--|
| _—_ | _—_ | _first release pending (v0.1.0 = Phase 1 gate)_ |
