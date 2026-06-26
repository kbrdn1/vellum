//! vellum — TUI SQL client (lib root).
//!
//! Used both by the `vellum` binary (`src/main.rs`) and by integration tests
//! under `tests/`. Module surface is intentionally `pub` for testability.
//! Modules are added phase by phase — see `.roadmap/ARCHITECTURE.md` for the
//! frozen layer map (model / query / write / ports / drivers / app / runtime
//! / tui).

pub mod cli;
pub mod error;
pub mod model;
