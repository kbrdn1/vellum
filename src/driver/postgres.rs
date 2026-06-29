//! The PostgreSQL `Driver` — the second impl (Phase 1, #10), behind the same
//! port as SQLite. Stub: the real `connect` / `query` / type mapping land in
//! the green step.

use async_trait::async_trait;

use crate::driver::Driver;
use crate::error::{Result, VellumError};
use crate::model::{Backend, QueryResult};

/// A connection to a PostgreSQL database, backed by a sqlx pool.
pub struct PostgresDriver {}

#[async_trait]
impl Driver for PostgresDriver {
  async fn connect(_dsn: &str) -> Result<Self> {
    Err(VellumError::Driver(
      "PostgresDriver::connect not implemented yet".into(),
    ))
  }

  async fn query(&self, _sql: &str) -> Result<QueryResult> {
    Err(VellumError::Driver("PostgresDriver::query not implemented yet".into()))
  }

  fn kind(&self) -> Backend {
    Backend::Postgres
  }
}
