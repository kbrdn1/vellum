//! TUI state modules — pure, ratatui-free state machines that sit behind the
//! view. The result-table state shipped in Phase 0; the schema sidebar (#14)
//! is the second pane; the diff view lands in a later phase.

pub mod sidebar;
pub mod table;
