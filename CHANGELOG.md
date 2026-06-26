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

## Past releases

_None yet._
