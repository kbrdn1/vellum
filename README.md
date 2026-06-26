# vellum

> **TUI SQL client** — browse, query, and safely edit databases in the
> terminal, with a **GitHub-like diff** for every write. Rust + ratatui,
> single static binary, instant start. **MIT · local-first · zero
> subscription.**

> ⚠️ **Early WIP.** The project scaffold is in place; Phase 0 (first driver +
> query + table render) is next. **Not yet usable** — there are no
> subcommands beyond `--help` / `--version` yet.

## Why

Paid desktop clients (DB Pro, TablePlus) are powerful but heavy, GUI-only,
and behind a licence. `vellum` is the opposite bet: the fastest, most
ergonomic SQL client **in the terminal**, fully open-source, no account, no
telemetry — everything local.

## Status

Pre-`0.1`, built in the open with the same engineering discipline as
[`gwm`](https://github.com/kbrdn1/gwm-cli): **TDD is mandatory**, contracts
are frozen at `1.0`, every commit is `cargo fmt` + `clippy -D warnings`
clean.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for conventions (branches, Gitmoji +
Conventional Commits, the red → green → refactor loop) and
[CLAUDE.md](CLAUDE.md) / [AGENTS.md](AGENTS.md) for the house rules AI
assistants must follow here.

## Licence

[MIT](LICENSE.md) © Kylian Bardini.
