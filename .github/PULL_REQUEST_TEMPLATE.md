## Description

<!-- Brief summary of the change -->

Closes #<!-- issue number -->

## Type of change

- [ ] ✨ Feature (new functionality)
- [ ] 🐛 Fix (bug fix)
- [ ] 🚑️ Hotfix (critical production fix)
- [ ] ♻️ Refactor (no behaviour change)
- [ ] 📝 Docs
- [ ] ✅ Tests
- [ ] ⚡ Perf
- [ ] 🔧 Chore / CI / build

## Changes

<!-- Bullet list of the main modifications -->

-
-
-

## Tests

- [ ] `cargo test` passes locally
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] New tests added under `tests/` (TDD: a PR adding behaviour without tests is sent back)

## Screenshots / TUI captures

<!-- For TUI changes, paste a recording or `script(1)` capture -->

## Checklist

- [ ] Branch follows `<type>/#<issue>-<description>`
- [ ] Commits follow Gitmoji + Conventional Commits (see CONTRIBUTING.md)
- [ ] CHANGELOG.md updated under `## [Unreleased]`
- [ ] README updated if the CLI surface changed
- [ ] `.vellum.toml` schema doc updated if config changed
- [ ] No `unwrap()` on user-facing paths (return `VellumError` instead)
- [ ] No `println!` in TUI render code (status bar only)

## Linked issues / docs

- Issue: #
- Related PR: #

## Notes for reviewers

<!-- Anything reviewers should focus on -->
