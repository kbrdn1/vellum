//! Unit coverage for the `cli` module's testable cores. The `vellum connect`
//! command wires a no-echo prompt and the OS keyring, neither exercisable in CI
//! — so its logic lives in `connect_with`, with the password source and the
//! secret store injected. These tests drive the real path (read → wrap → store
//! under the connection name) with a fake prompt and an in-process store.

use vellum::cli::{connect_with, resolve_credential};
use vellum::error::{Result, VellumError};
use vellum::model::Backend;
use vellum::secrets::{ExposeSecret, MemoryStore, SecretStore, SecretString};

#[test]
fn connect_with_stores_the_prompted_password_under_the_name() {
  let store = MemoryStore::default();
  connect_with(&store, "prod-pg", || Ok("hunter2".to_string())).unwrap();
  assert_eq!(
    store
      .get("prod-pg")
      .unwrap()
      .expect("a secret is stored")
      .expose_secret(),
    "hunter2",
    "connect reads the password and stores it under the connection name"
  );
}

#[test]
fn connect_with_stores_nothing_when_the_prompt_fails() {
  // A failed/aborted prompt propagates and must not leave a half-written entry.
  let store = MemoryStore::default();
  let result = connect_with(&store, "c", || Err(VellumError::Secret("no tty".into())));
  assert!(result.is_err(), "a prompt error propagates");
  assert!(
    store.get("c").unwrap().is_none(),
    "nothing is stored when the prompt fails"
  );
}

/// A keyring that errors on every operation — simulates an unavailable OS
/// keyring (no session keyutils on a headless Linux, a locked keychain), which
/// surfaces as an OS error, not `NoEntry`.
struct ErroringStore;

impl SecretStore for ErroringStore {
  fn set(&self, _: &str, _: &SecretString) -> Result<()> {
    Err(VellumError::Secret("keyring unavailable".into()))
  }
  fn get(&self, _: &str) -> Result<Option<SecretString>> {
    Err(VellumError::Secret("keyring unavailable".into()))
  }
  fn delete(&self, _: &str) -> Result<()> {
    Err(VellumError::Secret("keyring unavailable".into()))
  }
}

#[test]
fn resolve_credential_skips_the_keyring_for_sqlite() {
  // SQLite needs no password — an unavailable keyring must not block a local
  // file browse. The erroring store is never consulted; with no env override for
  // this unique name, the credential is simply `None`.
  let cred = resolve_credential(Backend::Sqlite, "vellum-test-sqlite-skip-xyz", &ErroringStore).unwrap();
  assert!(cred.is_none(), "sqlite resolves without touching the keyring");
}

#[test]
fn resolve_credential_uses_the_keyring_for_network_backends() {
  // Postgres / MySQL do need a secret, so the keyring is consulted — and its
  // error propagates rather than being swallowed.
  let err = resolve_credential(Backend::Postgres, "vellum-test-pg-xyz", &ErroringStore).unwrap_err();
  assert!(
    matches!(err, VellumError::Secret(_)),
    "the keyring error propagates for a network backend: {err:?}"
  );
}
