//! Shared e2e fixtures: a deterministic on-disk SQLite database the compiled
//! `vellum` binary can open read-only. Seeded in-process with sqlx so the
//! tests need no external service and no committed binary fixture.

use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::Executor as _;
use tempfile::NamedTempFile;

/// A tempfile SQLite database seeded with `items(id integer, label text)` and
/// rows `(1,'alpha'), (2,'beta'), (3,'gamma')`. The returned guard must outlive
/// every use of the path — dropping it deletes the backing file.
pub fn seeded_db() -> NamedTempFile {
  let file = NamedTempFile::new().expect("create temp db file");
  let dsn = format!("sqlite:{}", file.path().display());

  // Seed with a throwaway writable connection on a one-off current-thread
  // runtime (these e2e tests are sync — they spawn the binary), then close it.
  let rt = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .expect("build seeding runtime");
  rt.block_on(async {
    let pool = SqlitePool::connect_with(
      SqliteConnectOptions::from_str(&dsn)
        .expect("parse dsn")
        .create_if_missing(true),
    )
    .await
    .expect("open writable connection for seeding");
    pool
      .execute("create table items (id integer, label text)")
      .await
      .expect("create table");
    pool
      .execute("insert into items (id, label) values (1, 'alpha'), (2, 'beta'), (3, 'gamma')")
      .await
      .expect("seed rows");
    pool.close().await;
  });

  file
}
