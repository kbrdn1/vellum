//! Terminal UI (TEA-style). The pure `app` and `state` modules hold the model
//! and every transition; `view` is a thin ratatui render with no logic; the
//! `runtime` event loop is the only piece that touches a real terminal. The
//! split keeps each state transition unit-testable without a terminal (CLAUDE.md
//! TUI test taxonomy) — only the render path needs `TestBackend`.

pub mod app;
pub mod runtime;
pub mod state;
pub mod view;

pub use runtime::{browse, run};
