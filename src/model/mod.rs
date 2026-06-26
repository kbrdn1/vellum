//! Pure domain model — zero I/O, 100% unit-testable. The seam between the
//! database drivers and the rest of the app.
//!
//! Phase 0 ships the `Value` contract and the `QueryResult` container.
//! `catalog` (the schema tree) and `backend` (the engine tag) land with the
//! `Driver` trait in later Phase 0 / Phase 1 work — see ARCHITECTURE §4.

pub mod result;
pub mod value;

pub use result::{Column, QueryResult, Row};
pub use value::{TypeKind, Value};
