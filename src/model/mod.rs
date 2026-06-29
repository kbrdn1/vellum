//! Pure domain model — zero I/O, 100% unit-testable. The seam between the
//! database drivers and the rest of the app.
//!
//! Phase 0 ships the `Value` contract, the `QueryResult` container, and the
//! `Backend` engine tag. `catalog` (the schema tree) lands with the `Driver`
//! introspection in Phase 1 — see ARCHITECTURE §4.

pub mod backend;
pub mod catalog;
pub mod result;
pub mod value;

pub use backend::Backend;
// Only the `Catalog` root is re-exported flat; its nested types live under
// `catalog::` (a `catalog::Column` would clash with `result::Column`).
pub use catalog::Catalog;
pub use result::{Column, QueryResult, Row};
pub use value::{TypeKind, Value};
