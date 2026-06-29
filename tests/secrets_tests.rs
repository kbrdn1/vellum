//! Unit tests for `vellum::secrets`. The secret-handling contract: a password
//! round-trips through the `SecretStore` port, `VELLUM_DSN_<NAME>` overrides
//! the store, and an in-memory secret never leaks through `Debug`. Tests use
//! the process-local `MemoryStore` — never the OS keyring (no global state, no
//! real keychain in CI).

use vellum::secrets::{ExposeSecret, MemoryStore, SecretStore, SecretString};

#[test]
fn memory_store_round_trips_set_get_delete() {
  let store = MemoryStore::default();
  assert!(
    store.get("conn-a").unwrap().is_none(),
    "an unknown connection has no stored secret"
  );

  store
    .set("conn-a", &SecretString::from("s3cr3t".to_string()))
    .unwrap();
  let got = store
    .get("conn-a")
    .unwrap()
    .expect("the secret is present right after set");
  assert_eq!(got.expose_secret(), "s3cr3t");

  store.delete("conn-a").unwrap();
  assert!(
    store.get("conn-a").unwrap().is_none(),
    "the secret is gone after delete"
  );
}
