//! Shared e2e fixtures: deterministic on-disk SQLite databases the compiled
//! `vellum` binary can open read-only. Seeded in-process with sqlx so the
//! tests need no external service and no committed binary fixture.

use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::Executor as _;
use tempfile::NamedTempFile;

/// Run `statements` against a fresh writable SQLite database at `path` (created
/// if missing), then close the connection. Opens via `.filename(path)` rather
/// than a `sqlite:` DSN, so any path — including one with URL-structural
/// characters like `?` or `%` — is opened literally.
pub fn seed_sql(path: &Path, statements: &[&str]) {
  // These e2e tests are sync (they spawn the binary); seed on a one-off
  // current-thread runtime.
  let rt = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .expect("build seeding runtime");
  rt.block_on(async {
    let pool = SqlitePool::connect_with(SqliteConnectOptions::new().filename(path).create_if_missing(true))
      .await
      .expect("open writable connection for seeding");
    for stmt in statements {
      pool.execute(*stmt).await.expect("run seed statement");
    }
    pool.close().await;
  });
}

/// A tempfile SQLite database seeded with `items(id integer, label text)` and
/// rows `(1,'alpha'), (2,'beta'), (3,'gamma')`. The returned guard must outlive
/// every use of the path — dropping it deletes the backing file.
pub fn seeded_db() -> NamedTempFile {
  let file = NamedTempFile::new().expect("create temp db file");
  seed_sql(
    file.path(),
    &[
      "create table items (id integer, label text)",
      "insert into items (id, label) values (1, 'alpha'), (2, 'beta'), (3, 'gamma')",
    ],
  );
  file
}
