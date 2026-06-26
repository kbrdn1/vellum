//! The multi-DB port. **Sketch** — deliberately minimal (`connect` / `query`
//! / `kind`) while SQLite is the only impl. It freezes into the richer port
//! (capabilities, introspect, streaming, transactional execute —
//! ARCHITECTURE §4) in Phase 1, once Postgres is the second impl. No
//! speculative abstraction now (YAGNI).

pub mod sqlite;

pub use sqlite::SqliteDriver;

use async_trait::async_trait;

use crate::error::Result;
use crate::model::{Backend, QueryResult};

#[async_trait]
pub trait Driver: Send + Sync {
  /// Open a connection from a backend-specific DSN. For SQLite this is a
  /// `sqlite:` URL (e.g. `sqlite::memory:` or `sqlite:path/to/file.db`).
  async fn connect(dsn: &str) -> Result<Self>
  where
    Self: Sized;

  /// Run a single SQL statement and collect the full result into memory.
  /// Streaming by batch is a later-phase concern (ARCHITECTURE §4).
  async fn query(&self, sql: &str) -> Result<QueryResult>;

  /// Which engine this driver talks to.
  fn kind(&self) -> Backend;
}
