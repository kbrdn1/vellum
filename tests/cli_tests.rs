//! Unit coverage for the `cli` module's testable cores. The `vellum connect`
//! command wires a no-echo prompt and the OS keyring, neither exercisable in CI
//! — so its logic lives in `connect_with`, with the password source and the
//! secret store injected. These tests drive the real path (read → wrap → store
//! under the connection name) with a fake prompt and an in-process store.

use vellum::cli::connect_with;
use vellum::error::VellumError;
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
