//! TUI state modules — pure, ratatui-free state machines that sit behind the
//! view. Phase 0 ships only the result-table state; more panes (schema sidebar,
//! diff view) land in later phases as sibling modules.

pub mod table;
