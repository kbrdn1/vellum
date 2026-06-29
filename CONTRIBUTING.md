# Contributing to vellum

Thanks for your interest in `vellum` — a Rust TUI/CLI SQL client. This file
describes the conventions used here. They mirror [`gwm`](https://github.com/kbrdn1/gwm-cli)
so the muscle memory is the same across the projects.

## Table of contents

- [About this repository](#about-this-repository)
- [Project layout](#project-layout)
- [Development](#development)
- [Testing](#testing)
- [Branches](#branches)
- [Commits](#commits)
- [Labels](#labels)
- [Pull Requests](#pull-requests)
- [Merge strategy](#merge-strategy)
- [Releases](#releases)

## About this repository

`vellum` is a single-binary Rust crate (`bin` + reusable `lib`):

- **bin** `vellum` — entry point: dispatches to subcommands (CLI) or opens the TUI.
- **lib** `vellum` — modules exposed publicly so integration tests in `tests/`
  can drive them directly.

The architecture is layered/hexagonal: a pure domain core (no I/O), ports
(traits) such as `Driver`, adapters behind Cargo features, a TEA-style
application/state layer, and an async runtime shell. The full map and the
phased roadmap live in the private `.roadmap/` corpus (gitignored).

## Project layout

> Planned layout — modules land phase by phase (the tree below is the target
> from `.roadmap/ARCHITECTURE.md`, not all present yet).

```
vellum/
├── Cargo.toml
├── CHANGELOG.md           # in-progress index (per-version files in changelogs/)
├── CONTRIBUTING.md
├── LICENSE.md
├── README.md
├── .gwm.toml              # worktree bootstrap (managed by gwm)
├── .roadmap/              # private planning corpus (gitignored)
├── src/
│   ├── lib.rs             # public re-exports
│   ├── main.rs            # bin entry point
│   ├── error.rs           # VellumError
│   ├── cli.rs             # clap subcommands
│   ├── model/             # pure domain: Value, Catalog, QueryResult
│   ├── query/             # SQL splitting / highlight / autocomplete (sqlparser)
│   ├── write/             # changeset → diff → dry-run → apply engine
│   ├── ports/             # traits (Driver, …) — the I/O boundary
│   ├── drivers/           # Driver impls (pg / mysql / sqlite; opt-in: duckdb …)
│   ├── adapters/          # config, secrets (keyring), export/import
│   ├── app/               # TEA: Model + Message + update() (pure)
│   ├── runtime/           # async shell: event loop, effects (the only impure bit)
│   └── tui/               # ratatui views + state machines
└── tests/                 # one `*_tests.rs` / `*_integration.rs` per module
    ├── common/            # shared helpers
    ├── cli_binary.rs      # assert_cmd end-to-end (canary)
    ├── config_tests.rs
    ├── driver_tests.rs    # against in-process SQLite
    ├── write_tests.rs     # diff / dry-run / apply
    └── tui_app_tests.rs   # ratatui-free state machines
```

All tests live under `tests/` — no inline `#[cfg(test)] mod tests` blocks inside `src/`.

## Development

### Prerequisites

- Rust toolchain (stable, 1.88+ — the MSRV declared in `Cargo.toml`).

### Build & run

```bash
git clone https://github.com/kbrdn1/vellum.git
cd vellum

cargo build              # builds bin + lib
cargo run -- --help      # smoke test the CLI
cargo install --path .   # install vellum into ~/.cargo/bin
```

### Code style

- **Indentation**: 2 spaces (`rustfmt.toml` → `tab_spaces = 2`).
- **Formatter**: `cargo fmt`.
- **Linter**: `cargo clippy --all-targets -- -D warnings`.
- Run `cargo fmt && cargo clippy` before opening a PR.

### Local hooks (recommended, opt-in)

A POSIX `pre-commit` script lives under [`.githooks/`](.githooks/). It is **not
installed automatically** — opt in with:

```bash
git config core.hooksPath .githooks
```

Once enabled, the hook re-runs the suite under a stripped `PATH` when staged
`tests/*.rs` hunks reference ambient state (`assert_cmd`, `std::env::var`,
`which::which`, `dirs::`, `Command::cargo_bin`) — catching tests that pass in
your rich dev shell but fail on a minimal CI runner. It short-circuits in O(1)
when no staged paths match. **Bypass** a single safe commit with
`git commit --no-verify`. CI runs `shellcheck` + a smoke test on the hook.

## Testing

```bash
cargo test                          # run everything
cargo test --test config_tests      # one file
cargo test -- --nocapture           # see println from tests
```

### 🔴 TDD is mandatory — non-negotiable

**Test-Driven Development is the primary contribution rule of this repo.** No
production code lands without a failing test that pinned the behaviour down
first. PRs that add or change behaviour without tests are sent back, full stop.

The loop is **red → green → refactor**:

1. **Red** — write a failing test capturing the new behaviour (or the bug you
   are fixing). Run it. It MUST fail for the right reason (assertion mismatch,
   not a compile error in unrelated code).
2. **Green** — write the minimum production code that turns the test green. No
   speculative abstractions.
3. **Refactor** — clean up under green tests. Re-run the suite after each step.

Where the test lives:

- **pure domain logic** (value normalisation, catalog model, query splitting) →
  unit tests against the pure core, no I/O.
- **public CLI surface** → end-to-end test in `tests/cli_binary.rs` via `assert_cmd`.
- **`Driver` / query ops** → `tests/driver_tests.rs` against in-process SQLite.
- **write/diff engine** → `tests/write_tests.rs` (changeset → diff → dry-run → apply).
- **config (`.vellum.toml`)** → `tests/config_tests.rs`.
- **TUI state transitions** → ratatui-free tests in `tests/tui_app_tests.rs`.

#### Exceptions (must be argued in the PR description)

The bar to skip a test is "observably untestable from the public surface":
pure formatting / typo fixes in incidental strings, dependency bumps with no
behaviour change (CI green is the test), comment-only changes. Everything else
needs a test. "I tested it manually" is not an exception — codify it.

#### Enforcement

- Reviewers run `git log --stat <branch>..HEAD -- tests/`. No companion test
  diff (outside the exceptions) → blocked.
- The `## Tests` checklist in the PR template is binding.
- `tests/cli_binary.rs::help_prints_subcommands` is the canary — update it
  whenever a new CLI subcommand is added.

## Branches

- `main` — what ships. Direct commits allowed only for trivial maintenance
  (typos, docs, dep bumps). Anything user-visible goes through a PR.
- `dev` — integration branch; RCs are cut from here.
- Feature branches: `<type>/#<issue-number>-<short-description>` — e.g.
  `feat/#12-sqlite-driver`, `fix/#45-null-render`.

This repo carries a `.gwm.toml`, so use `gwm create feat 12 sqlite-driver` to
bootstrap the worktree + branch in one go.

## Commits

Format: `<emoji> <type>(<scope>)<!>: <subject>` (Gitmoji + Conventional Commits).

| Type       | When                                     | Emoji |
|:-----------|:-----------------------------------------|:------|
| `feat`     | new feature                              | ✨    |
| `fix`      | bug fix                                  | 🐛    |
| `hotfix`   | critical production bug fix              | 🚑️   |
| `refactor` | restructuring, no behaviour change       | ♻️    |
| `docs`     | documentation only                       | 📝    |
| `test`     | adding / fixing tests                    | ✅    |
| `perf`     | performance improvement                  | ⚡    |
| `chore`    | repo maintenance (deps, config, scripts) | 🔧    |
| `ci`       | CI / GitHub Actions                      | 👷    |
| `build`    | build system, Cargo manifest             | 🏗️    |

Scopes (optional): `cli`, `tui`, `config`, `driver`, `query`, `write`,
`export`, `tests`, `docs`, `ci`.

### Breaking changes

Suffix the type with `!` and add a `BREAKING CHANGE:` footer.

## Labels

See [`.github/LABELS.md`](.github/LABELS.md). Quick reference:

- **type**: `feature`, `fix`, `hotfix`, `docs`, `test`, `refactor`, `chore`, `perf`, `ci`, `build`
- **domain**: `cli`, `tui`, `config`, `driver`, `query`, `write`, `export`, `security`, `dependencies`

## Pull Requests

Before opening a PR: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`
(all green), `CHANGELOG.md` updated under `## [Unreleased]`. Use the PR
template.

## Merge strategy

- **Never squash.** Use a regular merge commit so the atomic history is preserved.
- **Never delete the source branch** after merge.

```bash
gh pr merge <num> --merge   # NOT --squash, NOT --delete-branch
```

## Releases

Versioning is SemVer (`MAJOR.MINOR.PATCH`), with `-rc.N` / `-alpha.N` /
`-beta.N` suffixes for pre-releases cut from `dev`.

### Step 0 — Reconcile open PRs (every tag)

Run `gh pr list --state open`; every open PR must be in the changeset,
intentionally deferred, or closed as stale before tagging.

### Pre-release (from `dev`)

1. Step 0 first. Stay on `dev`.
2. Write per-RC notes in `changelogs/pre-releases/<version>-rc.N.md` — heading
   `# [<version>-rc.N] - YYYY-MM-DD`, body describing only the **delta** vs the
   previous RC (or previous stable, for `rc.1`).
3. Tag: `git tag -a v0.x.y-rc.N -m "v0.x.y-rc.N" && git push --tags`.
4. `pre-release.yml` builds the 5 targets and publishes a **prerelease**, with
   the body sourced from the per-RC file.

### Stable release (from `main`)

1. Step 0 first.
2. Bump `Cargo.toml` `version`.
3. Move `## [Unreleased]` out of `CHANGELOG.md` into `changelogs/<version>.md`
   (heading `# [<version>] - YYYY-MM-DD`), leaving the root index empty with a
   one-line pointer under **Past releases**.
4. Merge `dev` → `main` (regular merge).
5. Tag: `git tag -a v0.x.y -m "v0.x.y" && git push --tags`.
6. `release.yml` builds + publishes the stable release (body from the
   per-version file) and refreshes the Homebrew tap.

| Tag pattern      | Workflow          | `prerelease` |
|:-----------------|:------------------|:-------------|
| `v0.x.y`         | `release.yml`     | `false`      |
| `v0.x.y-rc.N`    | `pre-release.yml` | `true`       |
| `v0.x.y-alpha.N` | `pre-release.yml` | `true`       |
| `v0.x.y-beta.N`  | `pre-release.yml` | `true`       |

### Homebrew tap

Stable releases refresh [`kbrdn1/homebrew-tap`](https://github.com/kbrdn1/homebrew-tap)
(`Formula/vellum.rb`) via the `homebrew-tap-update` job in `release.yml`. The
canonical source is [`packaging/homebrew/vellum.rb.template`](packaging/homebrew/vellum.rb.template).

The job needs a `HOMEBREW_TAP_TOKEN` secret (a fine-grained PAT with
`contents: write` scoped to the tap repo). It is `continue-on-error: true`
until the first successful sync — flip it to `false` afterwards so failures
block loudly.

---

By contributing, you agree your changes are licensed under the MIT License (see `LICENSE.md`).
