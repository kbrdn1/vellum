# vellum — house rules for AI assistants

This file is the project-level CLAUDE.md (mirrored verbatim in
[AGENTS.md](AGENTS.md)). Anything stated here OVERRIDES defaults and applies
to every contribution made via an AI assistant in this repository.

## 🔴 Primordial rule — Test-Driven Development is mandatory

**No production code lands without a failing test that pinned the
behaviour down first.** This is not a guideline, it is a hard merge
requirement. PRs that add or change behaviour without tests are sent
back, full stop.

### The TDD loop (red → green → refactor)

1. **Red** — write a failing test that captures the new behaviour or
   the bug you are fixing. Run it. It MUST fail for the right reason
   (assertion mismatch, not a compile error in unrelated code). Commit
   the test alone if it helps reviewers see the contract.
2. **Green** — write the minimum production code to make the test pass.
   No extra branches, no speculative abstractions.
3. **Refactor** — clean up while the tests are green. Re-run the full
   suite after every refactor step.

### What counts as "behaviour"

Anything observable from outside the function under test. The test
taxonomy mirrors the layered architecture (see
`.roadmap/ARCHITECTURE.md` for the full module map):

- A new CLI subcommand, flag, or output format → end-to-end test in
  `tests/cli_binary.rs` via `assert_cmd`.
- A new public function in `src/<module>.rs` → unit test in
  `tests/<module>_tests.rs`.
- A pure domain transform (value normalisation, catalog model, query
  splitting) → unit test against the pure core — no I/O.
- A `Driver` / query operation → integration test in
  `tests/driver_tests.rs`, exercised against an in-process SQLite
  (no external service required, CI-friendly).
- A write/diff changeset (data edit, DDL, schema-compare) →
  `tests/write_tests.rs` covering the changeset → diff → dry-run →
  apply path, including the destructive-query guard.
- A `.vellum.toml` parse / schema change → `tests/config_tests.rs`.
- A TUI state transition → state-machine test in
  `tests/tui_app_tests.rs` (ratatui-free — assert state, not pixels).

### Exceptions (narrow, must be argued in the PR description)

The bar to skip a test is "the change is observably untestable from
the public surface". Concretely:

- **Pure formatting / typo fixes** in user-facing strings → no test
  required if the string is incidental (a log line, a help blurb). If
  the string is asserted somewhere, update the assertion.
- **Dependency bumps** without behaviour change → CI green is the test.
- **Comments-only changes** → no test required.

Everything else needs a test. "I tested it manually" is not an
exception; codify the manual test as an integration test.

### Enforcement

- PR template ships with a `cargo test` checkbox under **Tests**. Do
  not tick it unless the suite actually ran green locally.
- Reviewers will run `git log --stat <branch>..HEAD -- tests/` and
  block the PR if the touched module has no companion test diff.
- `tests/cli_binary.rs::help_prints_subcommands` should be updated
  every time a new subcommand is added — treat this as the canary.

## 🔴 The write path is sacred — safe by construction

`vellum` edits live databases. The single biggest correctness/safety
surface is the write/diff engine. Non-negotiable rules:

- **Never auto-commit.** Every data/DDL change is staged into a
  changeset, rendered as a diff, and applied only on explicit confirm,
  inside a transaction with a dry-run available first.
- **Always parametrise.** Generated SQL uses bound parameters — never
  string interpolation of values. PK-targeted updates only; refuse to
  apply a row edit without a stable row identity.
- **Safe-mode for prod.** Connections flagged production are read-only
  by default and visually marked; destructive statements warn loudly.
- Any AI-generated SQL flows through the same diff gate — no exceptions,
  no shortcut path that bypasses confirmation.

## Other house rules

- **Reconcile open PRs before any tag.** Before cutting an RC or a
  stable, run `gh pr list --state open` and account for every open PR:
  in the changeset, intentionally deferred, or closed as stale.
- **Release notes are per-version, never the index.** The release
  workflows (`release.yml` / `pre-release.yml`) source their
  `body_path` from `changelogs/<version>.md` (stable) or
  `changelogs/pre-releases/<version>.md` (rc/alpha/beta), NOT from the
  top-level `CHANGELOG.md` (the in-progress index). Verify the
  per-version file exists and contains the release contents before
  tagging — the workflow hard-fails if it is missing.
- **Keep root `CHANGELOG.md` as in-progress only.** PRs add entries
  under `[Unreleased]`; do not reintroduce bullets already moved into
  the latest `changelogs/pre-releases/<previous-rc>.md`. The guard ships
  as `.github/scripts/check-rc-changelog-dupes.sh` and runs on every
  pre-release tag. Run it locally before cutting an RC.
- **Do not stack deep PR chains.** Keep at most 2-3 PRs open for a
  decomposition; merge each as soon as review + CI are green, let `dev`
  settle, then branch the next.
- **Parallel agents only when file ownership is disjoint.** Sub-agents
  work well for independent surfaces. If multiple tasks all touch shared
  files (the TUI app state, config plumbing, the `Driver` trait),
  dispatch them sequentially to avoid avoidable merge conflicts.
- **Follow-up issues beat scope creep.** If review uncovers a design bug
  whose fix changes the shape of the implementation, file a focused
  follow-up issue rather than hiding it in the current PR.
- **Verify MSRV against the whole codebase, not just the feature you
  added.** Before declaring or changing MSRV, run `cargo clippy
  --all-targets -- -W clippy::incompatible_msrv`; prefer `cargo msrv
  verify` when available.
- **Pre-validate environment-dependent tests.** Any test reading `$PATH`,
  the home directory, or other ambient state must be pre-validated
  locally against a stripped environment before push — CI runners don't
  have your installed tooling:

  ```bash
  PATH="$(dirname "$(command -v cargo)"):/usr/bin:/bin" cargo test
  ```

- **Indentation**: 2 spaces. `cargo fmt` is run on every commit; CI
  enforces `cargo fmt --check`.
- **Linter**: `cargo clippy --all-targets -- -D warnings` must pass. Do
  not `#[allow(...)]` warnings without a comment explaining why.
- **No `unwrap()` on user-facing paths**: return a `VellumError` variant
  instead. `unwrap()` is acceptable inside tests and genuinely
  infallible spots (e.g. `.lock()` on a never-poisoned mutex), but it
  must be a deliberate choice, not a shortcut.
- **No `println!` in TUI render code**: the status bar is the only
  channel for runtime feedback inside the TUI.
- **Branch convention**: `<type>/#<issue>-<description>`. Use
  `gwm create <type> <issue> <description>` — it bootstraps the worktree
  and creates the branch in one go (this repo carries a `.gwm.toml`).
- **Commit format**: Gitmoji + Conventional Commits. See
  [CONTRIBUTING.md](CONTRIBUTING.md#commits).
- **Merge strategy**: regular merge commit, never squash, never delete
  the source branch. The atomic commit history is the artefact.

## Where to look for the rest

- Branch / commit / PR conventions → [CONTRIBUTING.md](CONTRIBUTING.md)
- Community standards → [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
- Architecture & roadmap → `.roadmap/` (private planning corpus,
  gitignored — not published)
